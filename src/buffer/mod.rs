//! # Buffer Manager
//!
//! The buffer manager is an in-memory page cache that sits between the
//! storage layer (disk) and all higher layers. It implements:
//!
//! - **Page caching** with configurable pool size
//! - **Pin/Unpin** reference counting to prevent eviction of in-use pages
//! - **Dirty page tracking** for write-back
//! - **LRU eviction** policy for cache replacement
//!
//! ## Design (PostgreSQL-inspired)
//!
//! PostgreSQL's buffer manager uses a clock-sweep algorithm. We implement
//! classic LRU for simplicity, upgradable to clock-sweep later.

pub mod pool;
