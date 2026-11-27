//! # Query Executor
//!
//! Executes parsed SQL statements against the storage engine.
//! Implements the Volcano iterator model where each operator
//! produces tuples on demand.

use std::collections::HashMap;
use std::path::Path;

use crate::catalog::schema::{Catalog, ColumnDef, DataType};
use crate::parser::ast::*;
use crate::storage::heap::HeapFile;
use crate::storage::page::TupleId;
use crate::storage::tuple::{FieldValue, Tuple};
use crate::txn::wal::{WAL, TxnId};

/// Result of executing a query.
#[derive(Debug)]
pub enum QueryResult {
    /// Rows returned from SELECT.
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<FieldValue>>,
    },
    /// Row count from INSERT/UPDATE/DELETE.
    Modified {
        count: usize,
        message: String,
    },
    /// DDL result.
    Success(String),
    /// Error.
    Error(String),
}

impl std::fmt::Display for QueryResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryResult::Rows { columns, rows } => {
                // Header
                writeln!(f, "{}", columns.join(" | "))?;
                writeln!(f, "{}", "-".repeat(columns.len() * 15))?;
                for row in rows {
                    let vals: Vec<String> = row.iter().map(|v| format!("{}", v)).collect();
                    writeln!(f, "{}", vals.join(" | "))?;
                }
                write!(f, "({} rows)", rows.len())
            }
            QueryResult::Modified { count, message } => {
                write!(f, "{} ({} rows affected)", message, count)
            }
            QueryResult::Success(msg) => write!(f, "{}", msg),
            QueryResult::Error(msg) => write!(f, "ERROR: {}", msg),
        }
    }
}

/// The query executor.
pub struct Executor {
    catalog: Catalog,
    wal: Option<WAL>,
    current_txn: Option<TxnId>,
}

impl Executor {
    pub fn new(catalog: Catalog) -> Self {
        let wal_path = format!("{}/atlasdb.wal", catalog.db_path);
        let wal = WAL::open(&wal_path).ok();
        Self { catalog, wal, current_txn: None }
    }

    /// Execute a SQL statement.
    pub fn execute(&mut self, stmt: Statement) -> QueryResult {
        match stmt {
            Statement::CreateTable { table_name, columns } => {
                self.exec_create_table(&table_name, columns)
            }
            Statement::DropTable { table_name } => {
                self.exec_drop_table(&table_name)
            }
            Statement::Insert { table_name, columns, values } => {
                self.exec_insert(&table_name, columns, values)
            }
            Statement::Select { columns, from, joins, where_clause, order_by, limit } => {
                self.exec_select(columns, &from, joins, where_clause, order_by, limit)
            }
            Statement::Update { table_name, assignments, where_clause } => {
                self.exec_update(&table_name, assignments, where_clause)
            }
            Statement::Delete { table_name, where_clause } => {
                self.exec_delete(&table_name, where_clause)
            }
            Statement::CreateIndex { index_name, table_name, column_name, unique } => {
                QueryResult::Success(format!(
                    "CREATE INDEX {} ON {}({}) unique={}",
                    index_name, table_name, column_name, unique
                ))
            }
            Statement::BeginTransaction => {
                if let Some(ref mut wal) = self.wal {
                    match wal.begin() {
                        Ok(tid) => {
                            self.current_txn = Some(tid);
                            QueryResult::Success(format!("BEGIN transaction {}", tid))
                        }
                        Err(e) => QueryResult::Error(format!("failed to begin transaction: {}", e)),
                    }
                } else {
                    self.current_txn = Some(1);
                    QueryResult::Success("BEGIN transaction 1".into())
                }
            }
            Statement::Commit => {
                if let Some(tid) = self.current_txn.take() {
                    if let Some(ref mut wal) = self.wal {
                        let _ = wal.commit(tid);
                    }
                    QueryResult::Success(format!("COMMIT transaction {}", tid))
                } else {
                    QueryResult::Error("no active transaction to commit".into())
                }
            }
            Statement::Rollback => {
                if let Some(tid) = self.current_txn.take() {
                    if let Some(ref mut wal) = self.wal {
                        let _ = wal.abort(tid);
                    }
                    QueryResult::Success(format!("ROLLBACK transaction {}", tid))
                } else {
                    QueryResult::Error("no active transaction to rollback".into())
                }
            }
        }
    }

    fn exec_create_table(&mut self, name: &str, columns: Vec<ColumnSpec>) -> QueryResult {
        let cols: Vec<ColumnDef> = columns.into_iter().map(|c| {
            let mut cd = ColumnDef::new(&c.name, c.data_type, c.nullable);
            if c.primary_key { cd = cd.primary_key(); }
            cd
        }).collect();

        match self.catalog.create_table(name, cols) {
            Ok(_) => {
                // Create the heap file on disk
                let path = format!("{}/{}.db", self.catalog.db_path, name);
                if let Some(parent) = Path::new(&path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match HeapFile::open(&path, name) {
                    Ok(_) => QueryResult::Success(format!("CREATE TABLE {}", name)),
                    Err(e) => QueryResult::Error(format!("failed to create heap: {}", e)),
                }
            }
            Err(e) => QueryResult::Error(e),
        }
    }

    fn exec_drop_table(&mut self, name: &str) -> QueryResult {
        match self.catalog.drop_table(name) {
            Ok(()) => QueryResult::Success(format!("DROP TABLE {}", name)),
            Err(e) => QueryResult::Error(e),
        }
    }

    fn exec_insert(&mut self, table_name: &str, _columns: Option<Vec<String>>, values: Vec<Vec<Expr>>) -> QueryResult {
        let schema = match self.catalog.get_table(table_name) {
            Some(s) => s.clone(),
            None => return QueryResult::Error(format!("table '{}' not found", table_name)),
        };

        let path = schema.heap_file_path.clone();
        let mut heap = match HeapFile::open(&path, table_name) {
            Ok(h) => h,
            Err(e) => return QueryResult::Error(format!("failed to open heap: {}", e)),
        };

        let mut count = 0;
        for row_exprs in &values {
            let mut fields = Vec::new();
            for (i, expr) in row_exprs.iter().enumerate() {
                let fv = self.eval_literal(expr, if i < schema.columns.len() {
                    Some(&schema.columns[i].data_type)
                } else {
                    None
                });
                fields.push(fv);
            }
            let tuple = Tuple::new(0, fields);
            match heap.insert_tuple(&tuple) {
                Ok(_) => count += 1,
                Err(e) => return QueryResult::Error(format!("insert error: {}", e)),
            }
        }
        let _ = heap.sync();

        QueryResult::Modified { count, message: format!("INSERT {}", count) }
    }

    fn exec_select(
        &mut self,
        columns: Vec<SelectColumn>,
        from: &str,
        joins: Vec<JoinClause>,
        where_clause: Option<Expr>,
        order_by: Option<Vec<OrderByItem>>,
        limit: Option<usize>,
    ) -> QueryResult {
        let schema = match self.catalog.get_table(from) {
            Some(s) => s.clone(),
            None => return QueryResult::Error(format!("table '{}' not found", from)),
        };

        let path = schema.heap_file_path.clone();
        let mut heap = match HeapFile::open(&path, from) {
            Ok(h) => h,
            Err(e) => return QueryResult::Error(format!("failed to open heap: {}", e)),
        };

        // Sequential scan outer table
        let all_tuples = match heap.scan() {
            Ok(t) => t,
            Err(e) => return QueryResult::Error(format!("scan error: {}", e)),
        };

        // Outer column names (plain and qualified)
        let mut col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let mut qual_names: Vec<String> = schema.columns.iter().map(|c| format!("{}.{}", from, c.name)).collect();

        // Initial rows from outer table
        let mut current_rows: Vec<Vec<FieldValue>> = all_tuples.into_iter().map(|(_, t)| t.fields).collect();

        // Nested Loop Join across all join clauses
        for join_clause in joins {
            let join_table = &join_clause.table;
            let join_schema = match self.catalog.get_table(join_table) {
                Some(s) => s.clone(),
                None => return QueryResult::Error(format!("table '{}' not found in JOIN", join_table)),
            };

            let mut join_heap = match HeapFile::open(&join_schema.heap_file_path, join_table) {
                Ok(h) => h,
                Err(e) => return QueryResult::Error(format!("failed to open join heap: {}", e)),
            };

            let inner_tuples = match join_heap.scan() {
                Ok(t) => t,
                Err(e) => return QueryResult::Error(format!("scan error in join: {}", e)),
            };

            let inner_col_names: Vec<String> = join_schema.columns.iter().map(|c| c.name.clone()).collect();
            let inner_qual_names: Vec<String> = join_schema.columns.iter().map(|c| format!("{}.{}", join_table, c.name)).collect();

            let mut joined_rows = Vec::new();

            for outer_row in &current_rows {
                for (_, inner_t) in &inner_tuples {
                    let mut combined_fields = outer_row.clone();
                    combined_fields.extend(inner_t.fields.clone());

                    let mut env = HashMap::new();
                    for (i, name) in col_names.iter().enumerate() {
                        if i < outer_row.len() {
                            env.insert(name.clone(), &outer_row[i]);
                        }
                    }
                    for (i, name) in qual_names.iter().enumerate() {
                        if i < outer_row.len() {
                            env.insert(name.clone(), &outer_row[i]);
                        }
                    }
                    for (i, name) in inner_col_names.iter().enumerate() {
                        if i < inner_t.fields.len() {
                            env.insert(name.clone(), &inner_t.fields[i]);
                        }
                    }
                    for (i, name) in inner_qual_names.iter().enumerate() {
                        if i < inner_t.fields.len() {
                            env.insert(name.clone(), &inner_t.fields[i]);
                        }
                    }

                    if self.eval_bool(&join_clause.on, &env) {
                        joined_rows.push(combined_fields);
                    }
                }
            }

            current_rows = joined_rows;
            col_names.extend(inner_col_names);
            qual_names.extend(inner_qual_names);
        }

        // Filter (WHERE)
        let mut rows: Vec<Vec<FieldValue>> = Vec::new();
        for row in current_rows {
            if let Some(ref wc) = where_clause {
                let mut env = HashMap::new();
                for (i, name) in col_names.iter().enumerate() {
                    if i < row.len() {
                        env.insert(name.clone(), &row[i]);
                    }
                }
                for (i, name) in qual_names.iter().enumerate() {
                    if i < row.len() {
                        env.insert(name.clone(), &row[i]);
                    }
                }
                if !self.eval_bool(wc, &env) {
                    continue;
                }
            }
            rows.push(row);
        }

        // ORDER BY
        if let Some(ref ob) = order_by {
            for item in ob.iter().rev() {
                let col_idx = col_names.iter().position(|n| *n == item.column)
                    .or_else(|| qual_names.iter().position(|n| *n == item.column));
                if let Some(idx) = col_idx {
                    let asc = item.ascending;
                    rows.sort_by(|a, b| {
                        let a_field = a.get(idx).unwrap_or(&FieldValue::Null);
                        let b_field = b.get(idx).unwrap_or(&FieldValue::Null);
                        let cmp = self.compare_fields(a_field, b_field);
                        if asc { cmp } else { cmp.reverse() }
                    });
                }
            }
        }

        // LIMIT
        if let Some(lim) = limit {
            rows.truncate(lim);
        }

        // Check for Aggregates (COUNT, SUM, AVG, MIN, MAX)
        let has_aggregate = columns.iter().any(|c| match c {
            SelectColumn::Expr { expr: Expr::Function { name, .. }, .. } => {
                let upper = name.to_uppercase();
                upper == "COUNT" || upper == "SUM" || upper == "AVG" || upper == "MIN" || upper == "MAX"
            }
            _ => false,
        });

        if has_aggregate {
            let (agg_cols, agg_row) = self.eval_aggregates(&columns, &col_names, &qual_names, &rows);
            return QueryResult::Rows { columns: agg_cols, rows: vec![agg_row] };
        }

        // Project columns
        let (result_cols, result_rows) = self.project_joined(&columns, &col_names, &qual_names, rows);

        QueryResult::Rows { columns: result_cols, rows: result_rows }
    }

    fn exec_update(
        &mut self,
        table_name: &str,
        assignments: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    ) -> QueryResult {
        let schema = match self.catalog.get_table(table_name) {
            Some(s) => s.clone(),
            None => return QueryResult::Error(format!("table '{}' not found", table_name)),
        };

        let path = schema.heap_file_path.clone();
        let mut heap = match HeapFile::open(&path, table_name) {
            Ok(h) => h,
            Err(e) => return QueryResult::Error(format!("open error: {}", e)),
        };

        let all_tuples = match heap.scan() {
            Ok(t) => t,
            Err(e) => return QueryResult::Error(format!("scan error: {}", e)),
        };

        let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let mut count = 0;

        let mut updates: Vec<(TupleId, Tuple)> = Vec::new();
        for (tid, tuple) in &all_tuples {
            if let Some(ref wc) = where_clause {
                let env = self.build_env(&col_names, &tuple.fields);
                if !self.eval_bool(wc, &env) {
                    continue;
                }
            }

            let mut new_fields = tuple.fields.clone();
            for (col_name, expr) in &assignments {
                if let Some(idx) = col_names.iter().position(|n| n == col_name) {
                    new_fields[idx] = self.eval_literal(expr, Some(&schema.columns[idx].data_type));
                }
            }
            let new_tuple = Tuple::new(0, new_fields);
            updates.push((*tid, new_tuple));
            count += 1;
        }

        for (tid, new_tuple) in updates {
            if let Err(e) = heap.update_tuple(tid, &new_tuple) {
                return QueryResult::Error(format!("update error: {}", e));
            }
        }
        let _ = heap.sync();

        QueryResult::Modified { count, message: format!("UPDATE {}", count) }
    }

    fn exec_delete(
        &mut self,
        table_name: &str,
        where_clause: Option<Expr>,
    ) -> QueryResult {
        let schema = match self.catalog.get_table(table_name) {
            Some(s) => s.clone(),
            None => return QueryResult::Error(format!("table '{}' not found", table_name)),
        };

        let path = schema.heap_file_path.clone();
        let mut heap = match HeapFile::open(&path, table_name) {
            Ok(h) => h,
            Err(e) => return QueryResult::Error(format!("open error: {}", e)),
        };

        let all_tuples = match heap.scan() {
            Ok(t) => t,
            Err(e) => return QueryResult::Error(format!("scan error: {}", e)),
        };

        let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let mut to_delete = Vec::new();

        for (tid, tuple) in &all_tuples {
            if let Some(ref wc) = where_clause {
                let env = self.build_env(&col_names, &tuple.fields);
                if !self.eval_bool(wc, &env) {
                    continue;
                }
            }
            to_delete.push(*tid);
        }

        let count = to_delete.len();
        for tid in to_delete {
            if let Err(e) = heap.delete_tuple(tid) {
                return QueryResult::Error(format!("delete error: {}", e));
            }
        }
        let _ = heap.sync();

        QueryResult::Modified { count, message: format!("DELETE {}", count) }
    }

    // --- Helpers ---

    fn eval_literal(&self, expr: &Expr, _hint: Option<&DataType>) -> FieldValue {
        match expr {
            Expr::Literal(LiteralValue::Integer(n)) => FieldValue::Int32(*n as i32),
            Expr::Literal(LiteralValue::Float(f)) => FieldValue::Float64(*f),
            Expr::Literal(LiteralValue::String(s)) => FieldValue::String(s.clone()),
            Expr::Literal(LiteralValue::Boolean(b)) => FieldValue::Boolean(*b),
            Expr::Literal(LiteralValue::Null) => FieldValue::Null,
            _ => FieldValue::Null,
        }
    }

    fn build_env<'a>(&self, col_names: &[String], fields: &'a [FieldValue]) -> HashMap<String, &'a FieldValue> {
        let mut env = HashMap::new();
        for (i, name) in col_names.iter().enumerate() {
            if i < fields.len() {
                env.insert(name.clone(), &fields[i]);
            }
        }
        env
    }

    fn eval_bool(&self, expr: &Expr, env: &HashMap<String, &FieldValue>) -> bool {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                match op {
                    BinaryOperator::And => self.eval_bool(left, env) && self.eval_bool(right, env),
                    BinaryOperator::Or => self.eval_bool(left, env) || self.eval_bool(right, env),
                    _ => {
                        let lv = self.eval_field(left, env);
                        let rv = self.eval_field(right, env);
                        self.compare_op(&lv, op, &rv)
                    }
                }
            }
            Expr::Not(inner) => !self.eval_bool(inner, env),
            Expr::IsNull { expr, negated } => {
                let v = self.eval_field(expr, env);
                let is_null = matches!(v, FieldValue::Null);
                if *negated { !is_null } else { is_null }
            }
            Expr::Literal(LiteralValue::Boolean(b)) => *b,
            _ => false,
        }
    }

    fn eval_field(&self, expr: &Expr, env: &HashMap<String, &FieldValue>) -> FieldValue {
        match expr {
            Expr::ColumnRef(name) => {
                env.get(name).map(|v| (*v).clone()).unwrap_or(FieldValue::Null)
            }
            Expr::Literal(lit) => match lit {
                LiteralValue::Integer(n) => FieldValue::Int32(*n as i32),
                LiteralValue::Float(f) => FieldValue::Float64(*f),
                LiteralValue::String(s) => FieldValue::String(s.clone()),
                LiteralValue::Boolean(b) => FieldValue::Boolean(*b),
                LiteralValue::Null => FieldValue::Null,
            },
            _ => FieldValue::Null,
        }
    }

    fn compare_op(&self, left: &FieldValue, op: &BinaryOperator, right: &FieldValue) -> bool {
        let ord = self.compare_fields(left, right);
        match op {
            BinaryOperator::Eq => ord == std::cmp::Ordering::Equal,
            BinaryOperator::NotEq => ord != std::cmp::Ordering::Equal,
            BinaryOperator::Lt => ord == std::cmp::Ordering::Less,
            BinaryOperator::LtEq => ord != std::cmp::Ordering::Greater,
            BinaryOperator::Gt => ord == std::cmp::Ordering::Greater,
            BinaryOperator::GtEq => ord != std::cmp::Ordering::Less,
            _ => false,
        }
    }

    fn compare_fields(&self, a: &FieldValue, b: &FieldValue) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (FieldValue::Int32(x), FieldValue::Int32(y)) => x.cmp(y),
            (FieldValue::Int64(x), FieldValue::Int64(y)) => x.cmp(y),
            (FieldValue::Float64(x), FieldValue::Float64(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (FieldValue::String(x), FieldValue::String(y)) => x.cmp(y),
            (FieldValue::Boolean(x), FieldValue::Boolean(y)) => x.cmp(y),
            (FieldValue::Null, FieldValue::Null) => Ordering::Equal,
            (FieldValue::Null, _) => Ordering::Less,
            (_, FieldValue::Null) => Ordering::Greater,
            // Cross-type: try numeric comparison
            (FieldValue::Int32(x), FieldValue::Float64(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
            (FieldValue::Float64(x), FieldValue::Int32(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
            _ => Ordering::Equal,
        }
    }

    fn project_joined(
        &self,
        select_cols: &[SelectColumn],
        col_names: &[String],
        qual_names: &[String],
        rows: Vec<Vec<FieldValue>>,
    ) -> (Vec<String>, Vec<Vec<FieldValue>>) {
        if select_cols.len() == 1 && select_cols[0] == SelectColumn::Wildcard {
            return (col_names.to_vec(), rows);
        }

        let mut result_names = Vec::new();
        let mut indices = Vec::new();

        for sc in select_cols {
            match sc {
                SelectColumn::Wildcard => {
                    for (i, name) in col_names.iter().enumerate() {
                        result_names.push(name.clone());
                        indices.push(i);
                    }
                }
                SelectColumn::Expr { expr, alias } => {
                    match expr {
                        Expr::ColumnRef(name) => {
                            let idx = col_names.iter().position(|c| c == name)
                                .or_else(|| qual_names.iter().position(|c| c == name));
                            let header = alias.clone().unwrap_or_else(|| name.clone());
                            result_names.push(header);
                            indices.push(idx.unwrap_or(usize::MAX));
                        }
                        _ => {
                            result_names.push(alias.clone().unwrap_or_else(|| "expr".into()));
                            indices.push(usize::MAX);
                        }
                    }
                }
            }
        }

        let result_rows: Vec<Vec<FieldValue>> = rows.into_iter().map(|row| {
            indices.iter().map(|&i| {
                if i < row.len() { row[i].clone() } else { FieldValue::Null }
            }).collect()
        }).collect();

        (result_names, result_rows)
    }

    fn eval_aggregates(
        &self,
        columns: &[SelectColumn],
        col_names: &[String],
        qual_names: &[String],
        rows: &[Vec<FieldValue>],
    ) -> (Vec<String>, Vec<FieldValue>) {
        let mut out_cols = Vec::new();
        let mut out_fields = Vec::new();

        for col in columns {
            match col {
                SelectColumn::Expr { expr: Expr::Function { name, args }, alias } => {
                    let func_name = name.to_uppercase();
                    let header = alias.clone().unwrap_or_else(|| format!("{}(...)", name));
                    out_cols.push(header);

                    match func_name.as_str() {
                        "COUNT" => {
                            if args.is_empty() || matches!(args.first(), Some(Expr::ColumnRef(s)) if s == "*") {
                                out_fields.push(FieldValue::Int32(rows.len() as i32));
                            } else if let Some(Expr::ColumnRef(col_ref)) = args.first() {
                                let idx = col_names.iter().position(|c| c == col_ref)
                                    .or_else(|| qual_names.iter().position(|c| c == col_ref));
                                if let Some(i) = idx {
                                    let cnt = rows.iter().filter(|r| i < r.len() && r[i] != FieldValue::Null).count();
                                    out_fields.push(FieldValue::Int32(cnt as i32));
                                } else {
                                    out_fields.push(FieldValue::Int32(rows.len() as i32));
                                }
                            } else {
                                out_fields.push(FieldValue::Int32(rows.len() as i32));
                            }
                        }
                        "SUM" => {
                            if let Some(Expr::ColumnRef(col_ref)) = args.first() {
                                let idx = col_names.iter().position(|c| c == col_ref)
                                    .or_else(|| qual_names.iter().position(|c| c == col_ref));
                                if let Some(i) = idx {
                                    let mut sum_f = 0.0;
                                    let mut is_float = false;
                                    for r in rows {
                                        if i < r.len() {
                                            match &r[i] {
                                                FieldValue::Int32(n) => sum_f += *n as f64,
                                                FieldValue::Int64(n) => sum_f += *n as f64,
                                                FieldValue::Float32(f) => { sum_f += *f as f64; is_float = true; }
                                                FieldValue::Float64(f) => { sum_f += *f; is_float = true; }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if is_float {
                                        out_fields.push(FieldValue::Float64(sum_f));
                                    } else {
                                        out_fields.push(FieldValue::Int64(sum_f as i64));
                                    }
                                } else {
                                    out_fields.push(FieldValue::Null);
                                }
                            } else {
                                out_fields.push(FieldValue::Null);
                            }
                        }
                        "AVG" => {
                            if let Some(Expr::ColumnRef(col_ref)) = args.first() {
                                let idx = col_names.iter().position(|c| c == col_ref)
                                    .or_else(|| qual_names.iter().position(|c| c == col_ref));
                                if let Some(i) = idx {
                                    let mut sum_f = 0.0;
                                    let mut cnt = 0;
                                    for r in rows {
                                        if i < r.len() {
                                            match &r[i] {
                                                FieldValue::Int32(n) => { sum_f += *n as f64; cnt += 1; }
                                                FieldValue::Int64(n) => { sum_f += *n as f64; cnt += 1; }
                                                FieldValue::Float32(f) => { sum_f += *f as f64; cnt += 1; }
                                                FieldValue::Float64(f) => { sum_f += *f; cnt += 1; }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if cnt > 0 {
                                        out_fields.push(FieldValue::Float64(sum_f / cnt as f64));
                                    } else {
                                        out_fields.push(FieldValue::Null);
                                    }
                                } else {
                                    out_fields.push(FieldValue::Null);
                                }
                            } else {
                                out_fields.push(FieldValue::Null);
                            }
                        }
                        "MIN" => {
                            if let Some(Expr::ColumnRef(col_ref)) = args.first() {
                                let idx = col_names.iter().position(|c| c == col_ref)
                                    .or_else(|| qual_names.iter().position(|c| c == col_ref));
                                if let Some(i) = idx {
                                    let mut min_val: Option<&FieldValue> = None;
                                    for r in rows {
                                        if i < r.len() && r[i] != FieldValue::Null {
                                            if let Some(cur) = min_val {
                                                if self.compare_fields(&r[i], cur).is_lt() {
                                                    min_val = Some(&r[i]);
                                                }
                                            } else {
                                                min_val = Some(&r[i]);
                                            }
                                        }
                                    }
                                    out_fields.push(min_val.cloned().unwrap_or(FieldValue::Null));
                                } else {
                                    out_fields.push(FieldValue::Null);
                                }
                            } else {
                                out_fields.push(FieldValue::Null);
                            }
                        }
                        "MAX" => {
                            if let Some(Expr::ColumnRef(col_ref)) = args.first() {
                                let idx = col_names.iter().position(|c| c == col_ref)
                                    .or_else(|| qual_names.iter().position(|c| c == col_ref));
                                if let Some(i) = idx {
                                    let mut max_val: Option<&FieldValue> = None;
                                    for r in rows {
                                        if i < r.len() && r[i] != FieldValue::Null {
                                            if let Some(cur) = max_val {
                                                if self.compare_fields(&r[i], cur).is_gt() {
                                                    max_val = Some(&r[i]);
                                                }
                                            } else {
                                                max_val = Some(&r[i]);
                                            }
                                        }
                                    }
                                    out_fields.push(max_val.cloned().unwrap_or(FieldValue::Null));
                                } else {
                                    out_fields.push(FieldValue::Null);
                                }
                            } else {
                                out_fields.push(FieldValue::Null);
                            }
                        }
                        _ => {
                            out_fields.push(FieldValue::Null);
                        }
                    }
                }
                SelectColumn::Expr { expr, alias } => {
                    let header = alias.clone().unwrap_or_else(|| format!("{:?}", expr));
                    out_cols.push(header);
                    out_fields.push(FieldValue::Null);
                }
                SelectColumn::Wildcard => {
                    out_cols.push("count".into());
                    out_fields.push(FieldValue::Int32(rows.len() as i32));
                }
            }
        }

        (out_cols, out_fields)
    }

    /// Get reference to the catalog.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Get mutable reference to the catalog.
    pub fn catalog_mut(&mut self) -> &mut Catalog {
        &mut self.catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parser::parse_sql;
    use tempfile::TempDir;

    fn make_executor(tmp: &TempDir) -> Executor {
        let catalog = Catalog::new(tmp.path().to_str().unwrap());
        Executor::new(catalog)
    }

    #[test]
    fn test_create_insert_select() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        let r = exec.execute(parse_sql("CREATE TABLE t (id INTEGER, name TEXT)").unwrap());
        assert!(matches!(r, QueryResult::Success(_)));

        let r = exec.execute(parse_sql("INSERT INTO t VALUES (1, 'Alice')").unwrap());
        assert!(matches!(r, QueryResult::Modified { count: 1, .. }));

        let r = exec.execute(parse_sql("INSERT INTO t VALUES (2, 'Bob')").unwrap());
        assert!(matches!(r, QueryResult::Modified { count: 1, .. }));

        let r = exec.execute(parse_sql("SELECT * FROM t").unwrap());
        match r {
            QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
            _ => panic!("expected rows"),
        }
    }

    #[test]
    fn test_where_clause() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE wh (id INTEGER, val TEXT)").unwrap());
        exec.execute(parse_sql("INSERT INTO wh VALUES (1, 'a'), (2, 'b'), (3, 'c')").unwrap());

        let r = exec.execute(parse_sql("SELECT * FROM wh WHERE id = 2").unwrap());
        match r {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], FieldValue::String("b".into()));
            }
            _ => panic!("expected rows"),
        }
    }

    #[test]
    fn test_delete_with_where() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE del (id INTEGER, val TEXT)").unwrap());
        exec.execute(parse_sql("INSERT INTO del VALUES (1, 'a'), (2, 'b')").unwrap());
        exec.execute(parse_sql("DELETE FROM del WHERE id = 1").unwrap());

        let r = exec.execute(parse_sql("SELECT * FROM del").unwrap());
        match r {
            QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
            _ => panic!("expected rows"),
        }
    }

    #[test]
    fn test_update() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE upd (id INTEGER, name TEXT)").unwrap());
        exec.execute(parse_sql("INSERT INTO upd VALUES (1, 'Alice'), (2, 'Bob')").unwrap());
        exec.execute(parse_sql("UPDATE upd SET name = 'Alicia' WHERE id = 1").unwrap());

        let r = exec.execute(parse_sql("SELECT * FROM upd WHERE id = 1").unwrap());
        match r {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows[0][1], FieldValue::String("Alicia".into()));
            }
            _ => panic!("expected rows"),
        }
    }

    #[test]
    fn test_order_by_and_limit() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE nums (id INTEGER, val INTEGER)").unwrap());
        exec.execute(parse_sql("INSERT INTO nums VALUES (3, 30), (1, 10), (2, 20)").unwrap());

        let r = exec.execute(parse_sql("SELECT * FROM nums ORDER BY id ASC LIMIT 2").unwrap());
        match r {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][0], FieldValue::Int32(1));
                assert_eq!(rows[1][0], FieldValue::Int32(2));
            }
            _ => panic!("expected rows"),
        }
    }

    #[test]
    fn test_join() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE users (id INTEGER, name TEXT)").unwrap());
        exec.execute(parse_sql("CREATE TABLE orders (order_id INTEGER, user_id INTEGER, amount DOUBLE)").unwrap());

        exec.execute(parse_sql("INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')").unwrap());
        exec.execute(parse_sql("INSERT INTO orders VALUES (101, 1, 250.0), (102, 1, 50.0), (103, 2, 99.0)").unwrap());

        let r = exec.execute(parse_sql(
            "SELECT users.name, orders.amount FROM users INNER JOIN orders ON users.id = orders.user_id"
        ).unwrap());

        match r {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 3);
            }
            _ => panic!("expected joined rows"),
        }
    }

    #[test]
    fn test_aggregates() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        exec.execute(parse_sql("CREATE TABLE scores (val INTEGER)").unwrap());
        exec.execute(parse_sql("INSERT INTO scores VALUES (10), (20), (30)").unwrap());

        let r = exec.execute(parse_sql("SELECT COUNT(*), SUM(val), AVG(val), MIN(val), MAX(val) FROM scores").unwrap());
        match r {
            QueryResult::Rows { columns, rows } => {
                assert_eq!(columns.len(), 5);
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], FieldValue::Int32(3));
                assert_eq!(rows[0][1], FieldValue::Int64(60));
                assert_eq!(rows[0][2], FieldValue::Float64(20.0));
                assert_eq!(rows[0][3], FieldValue::Int32(10));
                assert_eq!(rows[0][4], FieldValue::Int32(30));
            }
            _ => panic!("expected aggregate row"),
        }
    }

    #[test]
    fn test_transactions() {
        let tmp = TempDir::new().unwrap();
        let mut exec = make_executor(&tmp);

        let r1 = exec.execute(parse_sql("BEGIN TRANSACTION").unwrap());
        assert!(matches!(r1, QueryResult::Success(_)));

        let r2 = exec.execute(parse_sql("COMMIT").unwrap());
        assert!(matches!(r2, QueryResult::Success(_)));

        let r3 = exec.execute(parse_sql("BEGIN").unwrap());
        assert!(matches!(r3, QueryResult::Success(_)));

        let r4 = exec.execute(parse_sql("ROLLBACK").unwrap());
        assert!(matches!(r4, QueryResult::Success(_)));
    }
}

