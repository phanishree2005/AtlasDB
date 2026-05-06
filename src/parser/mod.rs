//! # SQL Parser
//!
//! Tokenizer, parser, and AST for a subset of SQL including:
//! - CREATE TABLE, DROP TABLE
//! - INSERT INTO, SELECT, UPDATE, DELETE
//! - WHERE clauses with comparison operators
//! - ORDER BY, LIMIT

pub mod ast;
pub mod lexer;
pub mod parser;
