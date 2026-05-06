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
}

impl Executor {
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog }
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
            Statement::Select { columns, from, where_clause, order_by, limit } => {
                self.exec_select(columns, &from, where_clause, order_by, limit)
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

        // Sequential scan
        let all_tuples = match heap.scan() {
            Ok(t) => t,
            Err(e) => return QueryResult::Error(format!("scan error: {}", e)),
        };

        // Build column name map
        let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();

        // Filter (WHERE)
        let mut rows: Vec<Vec<FieldValue>> = Vec::new();
        for (_tid, tuple) in &all_tuples {
            if let Some(ref wc) = where_clause {
                let env = self.build_env(&col_names, &tuple.fields);
                if !self.eval_bool(wc, &env) {
                    continue;
                }
            }
            rows.push(tuple.fields.clone());
        }

        // ORDER BY
        if let Some(ref ob) = order_by {
            for item in ob.iter().rev() {
                if let Some(col_idx) = col_names.iter().position(|n| *n == item.column) {
                    let asc = item.ascending;
                    rows.sort_by(|a, b| {
                        let cmp = self.compare_fields(&a[col_idx], &b[col_idx]);
                        if asc { cmp } else { cmp.reverse() }
                    });
                }
            }
        }

        // LIMIT
        if let Some(lim) = limit {
            rows.truncate(lim);
        }

        // Project columns
        let (result_cols, result_rows) = self.project(&columns, &col_names, rows);

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

    fn project(
        &self,
        select_cols: &[SelectColumn],
        all_cols: &[String],
        rows: Vec<Vec<FieldValue>>,
    ) -> (Vec<String>, Vec<Vec<FieldValue>>) {
        if select_cols.len() == 1 && select_cols[0] == SelectColumn::Wildcard {
            return (all_cols.to_vec(), rows);
        }

        let mut result_names = Vec::new();
        let mut indices = Vec::new();

        for sc in select_cols {
            match sc {
                SelectColumn::Wildcard => {
                    for (i, name) in all_cols.iter().enumerate() {
                        result_names.push(name.clone());
                        indices.push(i);
                    }
                }
                SelectColumn::Expr { expr: Expr::ColumnRef(name), alias } => {
                    if let Some(idx) = all_cols.iter().position(|n| n == name) {
                        result_names.push(alias.clone().unwrap_or_else(|| name.clone()));
                        indices.push(idx);
                    }
                }
                _ => {}
            }
        }

        let result_rows: Vec<Vec<FieldValue>> = rows.into_iter().map(|row| {
            indices.iter().map(|&i| row[i].clone()).collect()
        }).collect();

        (result_names, result_rows)
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
}

