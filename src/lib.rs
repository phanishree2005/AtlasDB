//! AtlasDB — A fully-featured relational database engine built from scratch in Rust.
//!
//! # Architecture
//!
//! AtlasDB is organized into modular layers, each responsible for a distinct
//! aspect of database operation:
//!
//! - **storage**: Page-based disk storage with slotted page design
//! - **buffer**: In-memory page cache with LRU replacement policy
//! - **catalog**: System catalog and metadata management
//! - **index**: B+ tree index structures
//! - **parser**: SQL tokenizer, parser, and AST
//! - **query**: Volcano-model query execution engine
//! - **txn**: Transaction management, WAL, and recovery
//! - **optimizer**: Cost-based query optimizer
//! - **network**: TCP server and wire protocol

pub mod storage;
pub mod buffer;
pub mod catalog;
pub mod index;
pub mod parser;
pub mod query;
pub mod txn;
pub mod optimizer;
pub mod network;

// Re-export key types for convenience
pub use storage::page::{Page, PageId, SlotId, PAGE_SIZE};
pub use storage::tuple::{Tuple, TupleHeader, FieldValue};
pub use storage::error::{StorageError, StorageResult};
