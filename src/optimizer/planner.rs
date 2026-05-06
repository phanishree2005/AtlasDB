//! # Query Planner
//!
//! Generates and optimizes query execution plans. Uses table
//! statistics to choose between sequential scan and index scan.

use std::fmt;

/// Statistics about a table, used for cost estimation.
#[derive(Debug, Clone)]
pub struct TableStats {
    pub table_name: String,
    pub row_count: usize,
    pub page_count: usize,
    pub avg_row_size: usize,
    /// Per-column distinct value estimates.
    pub column_distinct: Vec<(String, usize)>,
}

impl TableStats {
    pub fn new(table_name: &str, row_count: usize, page_count: usize, avg_row_size: usize) -> Self {
        Self {
            table_name: table_name.to_string(),
            row_count,
            page_count,
            avg_row_size,
            column_distinct: Vec::new(),
        }
    }

    /// Estimate selectivity for an equality predicate on a column.
    pub fn eq_selectivity(&self, column: &str) -> f64 {
        for (col, distinct) in &self.column_distinct {
            if col == column && *distinct > 0 {
                return 1.0 / *distinct as f64;
            }
        }
        0.1 // default 10% selectivity
    }

    /// Estimate selectivity for a range predicate.
    pub fn range_selectivity(&self) -> f64 {
        0.33 // default 33% selectivity
    }
}

/// Physical plan node types.
#[derive(Debug, Clone)]
pub enum PlanNode {
    /// Sequential scan of a table.
    SeqScan {
        table: String,
        filter: Option<String>,
        estimated_rows: usize,
        estimated_cost: f64,
    },
    /// Index scan using a B+ tree.
    IndexScan {
        table: String,
        index: String,
        key_column: String,
        estimated_rows: usize,
        estimated_cost: f64,
    },
    /// Filter (selection).
    Filter {
        predicate: String,
        child: Box<PlanNode>,
        estimated_rows: usize,
    },
    /// Projection.
    Project {
        columns: Vec<String>,
        child: Box<PlanNode>,
    },
    /// Sort.
    Sort {
        column: String,
        ascending: bool,
        child: Box<PlanNode>,
    },
    /// Limit.
    Limit {
        count: usize,
        child: Box<PlanNode>,
    },
    /// Insert.
    Insert {
        table: String,
        row_count: usize,
    },
    /// Update.
    Update {
        table: String,
        filter: Option<String>,
    },
    /// Delete.
    Delete {
        table: String,
        filter: Option<String>,
    },
}

impl PlanNode {
    pub fn estimated_cost(&self) -> f64 {
        match self {
            PlanNode::SeqScan { estimated_cost, .. } => *estimated_cost,
            PlanNode::IndexScan { estimated_cost, .. } => *estimated_cost,
            PlanNode::Filter { child, .. } => child.estimated_cost(),
            PlanNode::Project { child, .. } => child.estimated_cost(),
            PlanNode::Sort { child, .. } => child.estimated_cost() * 1.5, // sort overhead
            PlanNode::Limit { child, .. } => child.estimated_cost(),
            PlanNode::Insert { .. } => 1.0,
            PlanNode::Update { .. } => 1.0,
            PlanNode::Delete { .. } => 1.0,
        }
    }
}

impl fmt::Display for PlanNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_indented(f, 0)
    }
}

impl PlanNode {
    fn fmt_indented(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let pad = "  ".repeat(indent);
        match self {
            PlanNode::SeqScan { table, estimated_rows, estimated_cost, filter } => {
                write!(f, "{}-> Seq Scan on {} (rows={}, cost={:.1})",
                    pad, table, estimated_rows, estimated_cost)?;
                if let Some(flt) = filter {
                    write!(f, "\n{}   Filter: {}", pad, flt)?;
                }
                Ok(())
            }
            PlanNode::IndexScan { table, index, estimated_rows, estimated_cost, .. } => {
                write!(f, "{}-> Index Scan using {} on {} (rows={}, cost={:.1})",
                    pad, index, table, estimated_rows, estimated_cost)
            }
            PlanNode::Filter { predicate, child, estimated_rows } => {
                writeln!(f, "{}-> Filter: {} (rows={})", pad, predicate, estimated_rows)?;
                child.fmt_indented(f, indent + 1)
            }
            PlanNode::Project { columns, child } => {
                writeln!(f, "{}-> Project [{}]", pad, columns.join(", "))?;
                child.fmt_indented(f, indent + 1)
            }
            PlanNode::Sort { column, ascending, child } => {
                let dir = if *ascending { "ASC" } else { "DESC" };
                writeln!(f, "{}-> Sort ({} {})", pad, column, dir)?;
                child.fmt_indented(f, indent + 1)
            }
            PlanNode::Limit { count, child } => {
                writeln!(f, "{}-> Limit {}", pad, count)?;
                child.fmt_indented(f, indent + 1)
            }
            PlanNode::Insert { table, row_count } => {
                write!(f, "{}-> Insert into {} ({} rows)", pad, table, row_count)
            }
            PlanNode::Update { table, .. } => {
                write!(f, "{}-> Update on {}", pad, table)
            }
            PlanNode::Delete { table, .. } => {
                write!(f, "{}-> Delete from {}", pad, table)
            }
        }
    }
}

/// The query planner / optimizer.
pub struct Planner {
    /// Table statistics for cost estimation.
    stats: std::collections::HashMap<String, TableStats>,
    /// Available indexes.
    indexes: Vec<(String, String, String)>, // (index_name, table, column)
}

impl Planner {
    pub fn new() -> Self {
        Self {
            stats: std::collections::HashMap::new(),
            indexes: Vec::new(),
        }
    }

    /// Update statistics for a table.
    pub fn update_stats(&mut self, stats: TableStats) {
        self.stats.insert(stats.table_name.clone(), stats);
    }

    /// Register an index for the planner.
    pub fn register_index(&mut self, index_name: &str, table: &str, column: &str) {
        self.indexes.push((
            index_name.to_string(),
            table.to_string(),
            column.to_string(),
        ));
    }

    /// Choose the best scan method for a table with an optional filter column.
    pub fn plan_scan(&self, table: &str, filter_column: Option<&str>) -> PlanNode {
        let stats = self.stats.get(table);
        let row_count = stats.map(|s| s.row_count).unwrap_or(1000);
        let page_count = stats.map(|s| s.page_count).unwrap_or(10);

        // Check if an index exists on the filter column
        if let Some(col) = filter_column {
            if let Some((idx_name, _, _)) = self.indexes.iter().find(|(_, t, c)| t == table && c == col) {
                let selectivity = stats.map(|s| s.eq_selectivity(col)).unwrap_or(0.1);
                let estimated_rows = (row_count as f64 * selectivity) as usize;
                let idx_cost = (estimated_rows as f64).log2().max(1.0) + estimated_rows as f64;

                let seq_cost = page_count as f64;

                if idx_cost < seq_cost {
                    return PlanNode::IndexScan {
                        table: table.to_string(),
                        index: idx_name.clone(),
                        key_column: col.to_string(),
                        estimated_rows,
                        estimated_cost: idx_cost,
                    };
                }
            }
        }

        // Default: sequential scan
        PlanNode::SeqScan {
            table: table.to_string(),
            filter: filter_column.map(|c| format!("on {}", c)),
            estimated_rows: row_count,
            estimated_cost: page_count as f64,
        }
    }

    /// Explain a query plan as a string.
    pub fn explain(&self, plan: &PlanNode) -> String {
        format!("{}", plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seq_scan_plan() {
        let planner = Planner::new();
        let plan = planner.plan_scan("users", None);
        assert!(matches!(plan, PlanNode::SeqScan { .. }));
    }

    #[test]
    fn test_index_scan_preferred() {
        let mut planner = Planner::new();
        let mut stats = TableStats::new("users", 100000, 5000, 100);
        stats.column_distinct.push(("id".to_string(), 100000));
        planner.update_stats(stats);
        planner.register_index("idx_users_id", "users", "id");

        let plan = planner.plan_scan("users", Some("id"));
        assert!(matches!(plan, PlanNode::IndexScan { .. }));
    }

    #[test]
    fn test_plan_display() {
        let plan = PlanNode::Limit {
            count: 10,
            child: Box::new(PlanNode::Sort {
                column: "name".to_string(),
                ascending: true,
                child: Box::new(PlanNode::SeqScan {
                    table: "users".to_string(),
                    filter: Some("id > 5".to_string()),
                    estimated_rows: 100,
                    estimated_cost: 10.0,
                }),
            }),
        };
        let display = format!("{}", plan);
        assert!(display.contains("Limit"));
        assert!(display.contains("Sort"));
        assert!(display.contains("Seq Scan"));
    }
}
