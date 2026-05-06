//! # Schema and Catalog Definitions
//!
//! Defines table schemas, column definitions, data types, and the
//! in-memory catalog that tracks all database objects.

use std::collections::HashMap;
use std::fmt;

/// Supported SQL data types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataType {
    Boolean,
    Integer,
    BigInt,
    Float,
    Double,
    Varchar(usize),
    Text,
    Blob,
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Boolean => write!(f, "BOOLEAN"),
            DataType::Integer => write!(f, "INTEGER"),
            DataType::BigInt => write!(f, "BIGINT"),
            DataType::Float => write!(f, "FLOAT"),
            DataType::Double => write!(f, "DOUBLE"),
            DataType::Varchar(n) => write!(f, "VARCHAR({})", n),
            DataType::Text => write!(f, "TEXT"),
            DataType::Blob => write!(f, "BLOB"),
        }
    }
}

/// A column definition within a table.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// Column position (0-indexed).
    pub ordinal: usize,
}

impl ColumnDef {
    pub fn new(name: &str, data_type: DataType, nullable: bool) -> Self {
        Self {
            name: name.to_string(),
            data_type,
            nullable,
            is_primary_key: false,
            ordinal: 0,
        }
    }

    pub fn primary_key(mut self) -> Self {
        self.is_primary_key = true;
        self.nullable = false;
        self
    }
}

/// A table schema: name + ordered columns.
#[derive(Debug, Clone)]
pub struct TableSchema {
    pub table_name: String,
    pub columns: Vec<ColumnDef>,
    /// File path for the heap file backing this table.
    pub heap_file_path: String,
    /// Table-level unique ID.
    pub table_id: u32,
}

impl TableSchema {
    pub fn new(table_name: &str, columns: Vec<ColumnDef>, heap_file_path: &str, table_id: u32) -> Self {
        let mut cols = columns;
        for (i, col) in cols.iter_mut().enumerate() {
            col.ordinal = i;
        }
        Self {
            table_name: table_name.to_string(),
            columns: cols,
            heap_file_path: heap_file_path.to_string(),
            table_id,
        }
    }

    /// Find a column by name. Returns (index, &ColumnDef).
    pub fn find_column(&self, name: &str) -> Option<(usize, &ColumnDef)> {
        self.columns.iter().enumerate().find(|(_, c)| c.name == name)
    }

    /// Number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Get the primary key column index, if any.
    pub fn primary_key_index(&self) -> Option<usize> {
        self.columns.iter().position(|c| c.is_primary_key)
    }
}

impl fmt::Display for TableSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TABLE {} (", self.table_name)?;
        for (i, col) in self.columns.iter().enumerate() {
            let pk = if col.is_primary_key { " PRIMARY KEY" } else { "" };
            let null = if col.nullable { "" } else { " NOT NULL" };
            let comma = if i < self.columns.len() - 1 { "," } else { "" };
            writeln!(f, "  {} {}{}{}{}", col.name, col.data_type, null, pk, comma)?;
        }
        write!(f, ")")
    }
}

/// Index metadata stored in the catalog.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub index_name: String,
    pub table_name: String,
    pub column_index: usize,
    pub file_path: String,
    pub is_unique: bool,
}

/// The system catalog — in-memory registry of all database objects.
#[derive(Debug)]
pub struct Catalog {
    /// Table schemas by table name.
    tables: HashMap<String, TableSchema>,
    /// Indexes by index name.
    indexes: HashMap<String, IndexInfo>,
    /// Next table ID.
    next_table_id: u32,
    /// Database directory for heap/index files.
    pub db_path: String,
}

impl Catalog {
    /// Create a new empty catalog.
    pub fn new(db_path: &str) -> Self {
        Self {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            next_table_id: 1,
            db_path: db_path.to_string(),
        }
    }

    /// Create a table and register it in the catalog.
    pub fn create_table(&mut self, name: &str, columns: Vec<ColumnDef>) -> Result<&TableSchema, String> {
        if self.tables.contains_key(name) {
            return Err(format!("table '{}' already exists", name));
        }

        let table_id = self.next_table_id;
        self.next_table_id += 1;

        let heap_path = format!("{}/{}.db", self.db_path, name);
        let schema = TableSchema::new(name, columns, &heap_path, table_id);
        self.tables.insert(name.to_string(), schema);
        Ok(self.tables.get(name).unwrap())
    }

    /// Drop a table.
    pub fn drop_table(&mut self, name: &str) -> Result<(), String> {
        if self.tables.remove(name).is_none() {
            return Err(format!("table '{}' does not exist", name));
        }
        // Also remove any indexes on this table
        self.indexes.retain(|_, idx| idx.table_name != name);
        Ok(())
    }

    /// Get a table schema by name.
    pub fn get_table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(name)
    }

    /// List all table names.
    pub fn list_tables(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }

    /// Register an index.
    pub fn create_index(&mut self, info: IndexInfo) -> Result<(), String> {
        if self.indexes.contains_key(&info.index_name) {
            return Err(format!("index '{}' already exists", info.index_name));
        }
        if !self.tables.contains_key(&info.table_name) {
            return Err(format!("table '{}' does not exist", info.table_name));
        }
        self.indexes.insert(info.index_name.clone(), info);
        Ok(())
    }

    /// Get indexes for a table.
    pub fn get_indexes(&self, table_name: &str) -> Vec<&IndexInfo> {
        self.indexes.values()
            .filter(|idx| idx.table_name == table_name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_table() {
        let mut catalog = Catalog::new("/tmp/test_db");
        let cols = vec![
            ColumnDef::new("id", DataType::Integer, false).primary_key(),
            ColumnDef::new("name", DataType::Varchar(255), false),
            ColumnDef::new("email", DataType::Text, true),
        ];
        let schema = catalog.create_table("users", cols).unwrap();
        assert_eq!(schema.table_name, "users");
        assert_eq!(schema.columns.len(), 3);
        assert_eq!(schema.primary_key_index(), Some(0));
    }

    #[test]
    fn test_duplicate_table_error() {
        let mut catalog = Catalog::new("/tmp/test_db");
        let cols = vec![ColumnDef::new("id", DataType::Integer, false)];
        catalog.create_table("t", cols.clone()).unwrap();
        assert!(catalog.create_table("t", cols).is_err());
    }

    #[test]
    fn test_drop_table() {
        let mut catalog = Catalog::new("/tmp/test_db");
        let cols = vec![ColumnDef::new("id", DataType::Integer, false)];
        catalog.create_table("t", cols).unwrap();
        catalog.drop_table("t").unwrap();
        assert!(catalog.get_table("t").is_none());
    }

    #[test]
    fn test_find_column() {
        let mut catalog = Catalog::new("/tmp/test_db");
        let cols = vec![
            ColumnDef::new("id", DataType::Integer, false),
            ColumnDef::new("name", DataType::Text, true),
        ];
        catalog.create_table("t", cols).unwrap();
        let schema = catalog.get_table("t").unwrap();
        let (idx, col) = schema.find_column("name").unwrap();
        assert_eq!(idx, 1);
        assert_eq!(col.data_type, DataType::Text);
        assert!(schema.find_column("missing").is_none());
    }

    #[test]
    fn test_schema_display() {
        let cols = vec![
            ColumnDef::new("id", DataType::Integer, false).primary_key(),
            ColumnDef::new("name", DataType::Varchar(100), true),
        ];
        let schema = TableSchema::new("users", cols, "/tmp/users.db", 1);
        let display = format!("{}", schema);
        assert!(display.contains("users"));
        assert!(display.contains("INTEGER"));
        assert!(display.contains("VARCHAR(100)"));
    }
}
