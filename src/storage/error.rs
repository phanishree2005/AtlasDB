//! Storage layer error types.
//!
//! We use `thiserror` for ergonomic, zero-cost error definitions that integrate
//! naturally with Rust's `?` operator and `Result` type.

use thiserror::Error;

/// Result type alias for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur in the storage layer.
#[derive(Debug, Error)]
pub enum StorageError {
    /// The page has insufficient free space for the requested operation.
    #[error("page full: need {needed} bytes but only {available} available")]
    PageFull { needed: usize, available: usize },

    /// The requested slot does not exist or has been deleted.
    #[error("invalid slot id: {0}")]
    InvalidSlot(u16),

    /// The tuple data exceeds the maximum size that can fit in a single page.
    /// In PostgreSQL, this triggers TOAST (The Oversized-Attribute Storage Technique).
    /// AtlasDB will support TOAST in a future phase.
    #[error("tuple too large: {size} bytes exceeds maximum {max_size} bytes")]
    TupleTooLarge { size: usize, max_size: usize },

    /// Data corruption detected (e.g., checksum mismatch).
    #[error("page corruption detected: {0}")]
    Corruption(String),

    /// Disk I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The requested page was not found.
    #[error("page not found: page_id={0}")]
    PageNotFound(u32),

    /// Attempted to modify a page that is pinned or locked.
    #[error("page is pinned and cannot be evicted: page_id={0}")]
    PagePinned(u32),

    /// Invalid tuple data.
    #[error("invalid tuple data: {0}")]
    InvalidTuple(String),

    /// The heap file is full (no more pages can be allocated).
    #[error("heap file full: max pages reached")]
    HeapFull,

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    SerializationError(String),
}
