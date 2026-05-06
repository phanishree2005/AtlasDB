//! # Query Engine
//!
//! Volcano-model iterator-based query execution engine.
//! Each operator implements a `next()` interface that pulls
//! one tuple at a time through the pipeline.

pub mod executor;
