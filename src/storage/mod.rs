//! # Storage Layer
//!
//! The storage layer is the foundation of AtlasDB. It manages how data is
//! physically organized on disk and in memory.
//!
//! ## Design Decisions
//!
//! **Slotted Page Architecture**: We use a slotted page design (identical to
//! PostgreSQL's approach) where each page has:
//! - A fixed-size header at the top
//! - A slot array growing downward from the header
//! - Tuple data growing upward from the bottom
//!
//! This design allows variable-length tuples and enables tuple movement during
//! compaction without invalidating external references (TIDs).
//!
//! **Page Size**: Default 4KB, matching typical OS page sizes and filesystem
//! block sizes. This is configurable at compile time.
//!
//! **Comparison with PostgreSQL**: PostgreSQL uses 8KB pages by default. We
//! default to 4KB for lower memory overhead during development, but the system
//! is fully parameterized.

pub mod error;
pub mod page;
pub mod tuple;
pub mod disk;
pub mod heap;
