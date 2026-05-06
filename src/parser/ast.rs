//! # Abstract Syntax Tree
//!
//! AST nodes representing parsed SQL statements.

use crate::catalog::schema::DataType;

/// A complete SQL statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// CREATE TABLE name (columns...)
    CreateTable {
        table_name: String,
        columns: Vec<ColumnSpec>,
    },
    /// DROP TABLE name
    DropTable {
        table_name: String,
    },
    /// INSERT INTO table (cols...) VALUES (vals...)
    Insert {
        table_name: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
    },
    /// SELECT columns FROM table [WHERE ...] [ORDER BY ...] [LIMIT ...]
    Select {
        columns: Vec<SelectColumn>,
        from: String,
        where_clause: Option<Expr>,
        order_by: Option<Vec<OrderByItem>>,
        limit: Option<usize>,
    },
    /// UPDATE table SET col = val, ... [WHERE ...]
    Update {
        table_name: String,
        assignments: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    },
    /// DELETE FROM table [WHERE ...]
    Delete {
        table_name: String,
        where_clause: Option<Expr>,
    },
    /// CREATE INDEX name ON table(column)
    CreateIndex {
        index_name: String,
        table_name: String,
        column_name: String,
        unique: bool,
    },
}

/// Column specification in CREATE TABLE.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
}

/// A column in a SELECT list.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectColumn {
    /// SELECT *
    Wildcard,
    /// SELECT expr [AS alias]
    Expr { expr: Expr, alias: Option<String> },
}

/// ORDER BY item.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderByItem {
    pub column: String,
    pub ascending: bool,
}

/// Expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal value.
    Literal(LiteralValue),
    /// A column reference.
    ColumnRef(String),
    /// Binary operation: left op right.
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOperator,
        right: Box<Expr>,
    },
    /// Unary NOT.
    Not(Box<Expr>),
    /// IS NULL / IS NOT NULL.
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    /// Function call (COUNT, SUM, etc.)
    Function {
        name: String,
        args: Vec<Expr>,
    },
}

/// Literal values in SQL.
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
}

/// Binary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Plus,
    Minus,
    Multiply,
    Divide,
}
