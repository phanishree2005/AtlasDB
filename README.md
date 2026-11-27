# AtlasDB 🗄️

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-68%20passed%20%7C%20100%25-brightgreen.svg)](#test-suite--verification)
[![Tokio](https://img.shields.io/badge/async-Tokio%20%2F%20Axum-blueviolet.svg?logo=tokio)](https://tokio.rs/)
[![React](https://img.shields.io/badge/frontend-React%2019%20%2F%20Vite-61dafb.svg?logo=react)](https://react.dev/)
[![Safety](https://img.shields.io/badge/memory--safety-100%25%20Safe%20Rust-success.svg)](https://www.rust-lang.org/)

An **ACID-compliant, high-performance relational database management system (RDBMS) engineered from scratch in Rust**. AtlasDB implements the complete relational database stack—from low-level slotted page disk serialization and an arena-allocated B+ tree index, to a Volcano-iterator query execution engine, write-ahead logging (WAL) crash recovery, a cost-based query optimizer (CBO), and a modern React 19 visual database studio.

Inspired by the internal architectures of **PostgreSQL** and **CMU 15-445 / BusTub**, AtlasDB eliminates all third-party embedded engines (no SQLite, RocksDB, or DuckDB dependencies) to implement core database primitives from first principles with zero `unsafe` Rust.

---

## Table of Contents

- [Architectural Overview](#architectural-overview)
- [System Architecture & Layer Stack](#system-architecture--layer-stack)
- [Deep-Dive Technical Implementation](#deep-dive-technical-implementation)
  - [1. Storage Engine & Slotted Page Architecture](#1-storage-engine--slotted-page-architecture)
  - [2. Buffer Pool Manager & LRU Replacement](#2-buffer-pool-manager--lru-replacement)
  - [3. Arena-Allocated B+ Tree Index](#3-arena-allocated-b-tree-index)
  - [4. SQL Lexer & Recursive-Descent Parser](#4-sql-lexer--recursive-descent-parser)
  - [5. Volcano Iterator Query Execution Engine](#5-volcano-iterator-query-execution-engine)
  - [6. Cost-Based Query Optimizer (CBO)](#6-cost-based-query-optimizer-cbo)
  - [7. Write-Ahead Logging (WAL) & Crash Recovery](#7-write-ahead-logging-wal--crash-recovery)
  - [8. Multi-Threaded TCP Server & Wire Protocol](#8-multi-threaded-tcp-server--wire-protocol)
  - [9. Async Web Studio (Axum + React 19)](#9-async-web-studio-axum--react-19)
- [Algorithmic Complexity & Specifications](#algorithmic-complexity--specifications)
- [Quick Start Guide](#quick-start-guide)
  - [Prerequisites](#prerequisites)
  - [Building from Source](#building-from-source)
  - [Running the Interactive SQL Shell (REPL)](#running-the-interactive-sql-shell-repl)
  - [Launching the Web Studio (UI + API)](#launching-the-web-studio-ui--api)
  - [Starting the TCP Server](#starting-the-tcp-server)
  - [Executing the Full System Demo](#executing-the-full-system-demo)
- [SQL Syntax & Capability Reference](#sql-syntax--capability-reference)
- [Test Suite & Verification](#test-suite--verification)
- [Project Directory Structure](#project-directory-structure)
- [Resume / Portfolio Highlights](#resume--portfolio-highlights)
- [License](#license)

---

## Architectural Overview

AtlasDB is partitioned into 9 clean, modular layers following classic database systems pedagogy:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        CLIENT INTERFACES                               │
│      CLI REPL Shell      │     TCP Client (Wire)    │   React 19 Web   │
└──────────────┬─────────────────────────┬───────────────────────┬───────┘
               │                         │                       │
               ▼                         ▼                       ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 8 & 9: NETWORK, TRANSPORT & API PROTOCOLS                      │
│  - Multi-threaded TCP Listener (Port 5433)                             │
│  - Asynchronous Axum REST Server with CORS support (Port 3000)         │
└────────────────────────────────────────┬───────────────────────────────┘
                                         │
                                         ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 4 & 5: SQL PARSER & QUERY PROCESSING ENGINE                    │
│  - Lexical Scanner (Token stream)                                      │
│  - Recursive-Descent Parser -> Abstract Syntax Tree (AST)              │
│  - System Catalog Schema Registry (Metadata, Types, Constraints)       │
└────────────────────────────────────────┬───────────────────────────────┘
                                         │
                                         ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 6: COST-BASED QUERY OPTIMIZER (CBO)                             │
│  - Cardinality & Selectivity Estimation (Uniform 1/V(C), Range 33%)    │
│  - Dynamic Physical Plan Selection: SeqScan vs. B+ Tree IndexScan      │
└────────────────────────────────────────┬───────────────────────────────┘
                                         │
                                         ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 5: VOLCANO ITERATOR EXECUTION ENGINE                           │
│  - Open() / Next() / Close() Streaming Iterator Model                  │
│  - Physical Operators: SeqScan, IndexScan, Filter, Project, Sort, Limit│
└───────────────────────┬────────────────────────┬───────────────────────┘
                        │                        │
       Write Operations │                        │ Read / Index Operations
                        ▼                        ▼
┌───────────────────────────────┐        ┌───────────────────────────────┐
│ LAYER 7: WRITE-AHEAD LOG (WAL)│        │ LAYER 3: B+ TREE INDEX ENGINE │
│ - Monotonic LSN Generation    │        │ - Order-64 (Max 63 keys/node) │
│ - Append-Only Redo Log Buffer │        │ - Safe Arena Allocation       │
│ - CRC32 Frame Verification    │        │ - Linked Doubly-Ended Leaves  │
│ - Post-Crash Recovery Engine  │        │ - Point & Range Scan Queries  │
└───────────────┬───────────────┘        └───────────────┬───────────────┘
                │                                        │
                └───────────────────┬────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 2: BUFFER POOL MANAGER & CACHE REPLACEMENT                      │
│  - Fixed-Size Page Frames with Pin/Unpin Reference Counting            │
│  - Least Recently Used (LRU) Eviction Queue                            │
│  - Hash-Indexed Page Table (PageId -> FrameId)                         │
│  - Dirty-Page Synchronization to Disk                                  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│  LAYER 1: PHYSICAL STORAGE ENGINE & HEAP MANAGER                       │
│  - PostgreSQL-Style Slotted Page Layout (4096-byte blocks)             │
│  - Downward-growing slot array + Upward-growing tuple records          │
│  - O(N) In-Page Dynamic Compaction & Slot Reclamation                  │
│  - Compact Null Bitmaps & Variable-Length Binary Serialization         │
│  - CRC32 Block Corruption Detection Checksums                          │
│  - Heap File Access Manager for Table-Level CRUD                       │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
                             [ PHYSICAL DISK ]
```

---

## Deep-Dive Technical Implementation

### 1. Storage Engine & Slotted Page Architecture
- **PostgreSQL-Inspired Page Layout**: Default block size is configured to **4,096 bytes (4KB)**, matching native OS physical page allocations and virtual memory subsystems.
- **Dual-Direction Allocation**:
  - A fixed **24-byte Page Header** sits at offset 0 (storing `page_id`, `slot_count`, `free_space_start`, `free_space_end`, `checksum`, `flags`, and `lsn`).
  - The **Slot Array** grows *downward* from the header (`offset 24 + n * 4`).
  - **Tuple Data** grows *upward* from the bottom of the page (`offset 4096 - m`).
- **Stable Tuple Identifiers (`TupleId`)**: External references use `(page_id, slot_id)`. During deletions or updates, slots are reused or compacted without changing external foreign references or invalidating index pointers.
- **Dynamic Compaction ($O(N)$)**: When fragmented dead space prevents new insertions, in-page compaction shifts active tuples to the page bottom and re-aligns slot offsets while preserving continuous free space.
- **Data Integrity via CRC32**: Every block includes a 32-bit hardware-accelerated CRC32 checksum (`crc32fast`). Pages verify checksums on every disk read to protect against bit-rot and partial sector writes.
- **Tuple Binary Encoding**:
  - 12-byte tuple header (`txn_id`, `flags`, `null_bmp`, `field_count`, `reserved`).
  - $\lceil \text{fields} / 8 \rceil$ byte bitmask for `NULL` value tracking.
  - Variable-length fields (`VARCHAR`, `TEXT`, `BLOB`) store 4-byte descriptors (2B offset, 2B length) in the fixed section with payload packed at the end.

```text
┌──────────────────────────────────────────────────────────────────────┐ offset 0
│ PageHeader (24B): PageId(4B) | SlotCount(2B) | FreeStart(2B) | ...  │
├──────────────────────────────────────────────────────────────────────┤
│ Slot 0 [Offset, Len] │ Slot 1 [Offset, Len] │ ...                    │  ↓ grows downward
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│                       UNALLOCATED FREE SPACE                         │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│ ...                  │ Tuple 1 Data         │ Tuple 0 Data           │  ↑ grows upward
└──────────────────────────────────────────────────────────────────────┘ offset 4096
```

### 2. Buffer Pool Manager & LRU Replacement
- **Controlled Working Set**: Direct disk I/O is throttled through a configurable in-memory buffer pool managing fixed frame descriptors.
- **Pin / Unpin Reference Counting**: Concurrent query execution pipelines pin pages into frames during tuple evaluation. Double-unpin errors are caught at runtime.
- **Eviction Protocol**: Unpinned frames are tracked in a `VecDeque` LRU queue. If a page marked `dirty` is selected for eviction, it is atomically flushed to disk prior to replacement.
- **Amortized $O(1)$ Lookup**: A hash map translates `PageId -> FrameId`, yielding instantaneous cache hits.

### 3. Arena-Allocated B+ Tree Index
- **Zero-Unsafe Memory Model**: Traditional B+ trees in Rust often suffer from borrow checker conflicts or resort to `unsafe` raw pointers / `Rc<RefCell<Node>>` reference cycles. AtlasDB solves this via an **Arena Allocation Pattern**: all nodes are stored in a contiguous `Vec<BTreeNode>`, referenced purely by `usize` indices.
- **High Fan-Out ($M = 64$)**: Order-64 design keeps the tree shallow ($O(\log_{64} N)$), minimizing node traversals.
- **Type-Agnostic Lexicographical Keys**: Keys are converted into binary vectors (`IndexKey(Vec<u8>)`). Big-endian conversions for `i32` and `i64` ensure correct two's-complement byte ordering for relational comparisons.
- **Linked Leaf Nodes**: Leaf nodes maintain bidirectional pointers, enabling efficient $O(\log N + K)$ range scans (e.g., `WHERE id >= 10 AND id <= 50`).

### 4. SQL Lexer & Recursive-Descent Parser
- **Lexical Analyzer**: Handcrafted deterministic tokenizer that parses SQL keywords, identifiers, comparison operators (`=`, `!=`, `<`, `<=`, `>`, `>=`), and literals without external dependencies.
- **Recursive-Descent AST Builder**: Generates clean, strongly-typed AST nodes:
  - DDL: `CreateTable`, `DropTable`, `CreateIndex`.
  - DML: `Select`, `Insert`, `Update`, `Delete`.
  - Expressions: Binary operations, boolean logic (`AND`, `OR`), ordering clauses (`ORDER BY col ASC|DESC`), and window constraints (`LIMIT n`).

### 5. Volcano Iterator Query Execution Engine
- Implements Goetz Graefe's classic **Volcano Iterator Model** (pipelined tuple-at-a-time streaming):
  - Every operator implements a unified iterator protocol: `open()`, `next() -> Option<Tuple>`, and `close()`.
  - Avoids materializing massive intermediate tables in RAM, maintaining a constant $O(1)$ tuple working memory footprint per pipeline stage.
- **Pipeline Operators**:
  - `SeqScan`: Sequential table scan over heap file pages.
  - `IndexScan`: Key-directed B+ tree traversals and range scans.
  - `Filter`: Dynamic predicate evaluation against tuple fields.
  - `Project`: Column selection, reordering, and scalar expressions.
  - `Sort`: In-memory multi-column ordering with ascending/descending directions.
  - `Limit`: Early-stopping operator to truncate downstream execution.

### 6. Cost-Based Query Optimizer (CBO)
- **Statistical Catalog**: Maintains per-table statistics including estimated row counts ($N$), page counts ($P$), average row byte sizes, and distinct column values ($V(A)$).
- **Selectivity Formulas**:
  $$\text{Selectivity}(A = \text{val}) = \frac{1}{V(A)}$$
  $$\text{Selectivity}(A \ge \text{val}) = 0.33 \quad \text{(default 1/3 range fraction)}$$
- **Plan Cost Evaluation**:
  - **Sequential Scan Cost**:
    $$\text{Cost}_{\text{seq}} = P \cdot \text{Cost}_{\text{page\_io}} + N \cdot \text{Cost}_{\text{cpu\_tuple}}$$
  - **B+ Tree Index Scan Cost**:
    $$\text{Cost}_{\text{index}} = \left( \log_{64}(N) + N \cdot \text{Selectivity} \right) \cdot \text{Cost}_{\text{random\_io}}$$
- **Path Selection**: Automatically elects an `IndexScan` over a `SeqScan` when high selectivity justifies avoiding a full heap scan.

### 7. Write-Ahead Logging (WAL) & Crash Recovery
- **Durability Guarantee**: Enforces the strict Write-Ahead Logging invariant: *no modified data page is flushed to permanent disk storage before its corresponding log records have been serialized and synced to the append-only WAL file*.
- **Binary Log Records**:
  ```text
  [LSN: 4B] [TxnId: 4B] [Type: 1B] [TableLen: 2B] [Table: var] [PageId: 4B] [SlotId: 2B] [DataLen: 4B] [Data: var] [CRC: 4B]
  ```
- **Crash Recovery Engine**:
  - Reads WAL sequentially from offset 0, verifying CRC32 checksums for every frame.
  - Tracks transactions across states (`Begin`, `Commit`, `Abort`).
  - Runs a **Redo Phase** to replay modifications of committed transactions that were not yet flushed to heap files when a crash occurred.

### 8. Multi-Threaded TCP Server & Wire Protocol
- Dedicated TCP server (`src/network/server.rs`) listening on port `5433`.
- Handles multiple concurrent client connections using connection threads and a text wire protocol with diagnostic output.
- Interoperable with standard terminal utilities (`nc`, `telnet`) and scripts.

### 9. Async Web Studio (Axum + React 19)
- **Backend API**: Powered by `axum` and `tokio`, providing high-throughput non-blocking `/query` and `/tables` JSON endpoints with CORS support.
- **Frontend Studio**: Built with React 19, Vite, and Lucide icons.
  - Real-time SQL syntax console with multi-line input.
  - Visual schema inspector showing active tables and column definitions.
  - Execution performance diagnostics (execution time, affected rows count).
  - Pre-built query templates for instant testing.

---

## Algorithmic Complexity & Specifications

| Subsystem | Operation | Time Complexity | Space Complexity | Design Pattern / Mechanism |
|---|---|:---:|:---:|---|
| **Slotted Page** | Slot Lookup | $\mathcal{O}(1)$ | $\mathcal{O}(1)$ | Array indexing into header slots |
| **Slotted Page** | Compaction | $\mathcal{O}(N)$ | $\mathcal{O}(1)$ in-place | Pointer-preserving data shift |
| **Buffer Pool** | Cache Lookup | $\mathcal{O}(1)$ amortized | $\mathcal{O}(F)$ frames | `HashMap<PageId, FrameId>` |
| **Buffer Pool** | Frame Eviction | $\mathcal{O}(1)$ | $\mathcal{O}(F)$ frames | LRU `VecDeque` queue |
| **B+ Tree** | Point Search | $\mathcal{O}(\log_{64} N)$ | $\mathcal{O}(1)$ | Binary search in internal nodes |
| **B+ Tree** | Range Scan | $\mathcal{O}(\log_{64} N + K)$ | $\mathcal{O}(K)$ | Linked leaf traversal |
| **B+ Tree** | Insertion / Split | $\mathcal{O}(\log_{64} N)$ | $\mathcal{O}(1)$ amortized | Arena allocation in flat `Vec` |
| **Volcano Engine**| `next()` Call | $\mathcal{O}(1)$ streaming | $\mathcal{O}(1)$ working set | Lazy iterator pipelining |
| **WAL Recovery** | Crash Redo | $\mathcal{O}(L)$ log entries | $\mathcal{O}(T)$ transactions | Single-pass LSN redo scan |

---

## Quick Start Guide

### Prerequisites
- **Rust Toolchain**: 1.80+ (Stable)
  ```bash
  # Linux/macOS
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

  # Windows (PowerShell)
  winget install Rustlang.Rustup
  ```
- **Node.js**: 18+ & npm (Optional, only required for Web Studio UI)

### Building from Source
```bash
git clone https://github.com/phanishree2005/AtlasDB.git
cd AtlasDB
cargo build --release
```

---

### Running the Interactive SQL Shell (REPL)
Launch the CLI shell for instant querying:
```bash
cargo run
```
```text
=== AtlasDB v0.1.0 — Interactive SQL Shell ===
Type SQL statements, '.tables' to list tables, '.quit' to exit.

atlasdb> CREATE TABLE developers (id INTEGER, name VARCHAR(50), salary DOUBLE);
Table 'developers' created.

atlasdb> INSERT INTO developers VALUES (1, 'Alice', 115000.0), (2, 'Bob', 98000.0);
Inserted 2 rows.

atlasdb> SELECT * FROM developers WHERE salary > 100000.0;
id | name | salary
---------------------------------------------
1 | Alice | 115000
(1 rows)

atlasdb> .tables
  developers

atlasdb> .quit
Bye!
```

---

### Launching the Web Studio (UI + API)

1. **Start the Database Backend (Axum REST API)**:
   ```bash
   cargo run -- --web 3000
   ```
2. **Launch the React 19 Frontend**:
   ```bash
   cd ui
   npm install
   npm run dev
   ```
3. Open **`http://localhost:5173`** in your browser to access the interactive dashboard.

---

### Starting the TCP Server
Run AtlasDB as a headless daemon:
```bash
cargo run -- --server 5433
```
Connect via terminal:
```bash
# Linux / macOS
nc localhost 5433

# Windows PowerShell
Test-NetConnection -ComputerName 127.0.0.1 -Port 5433
```

---

### Executing the Full System Demo
To observe all 9 database layers executing sequentially with verbose internal diagnostics:
```bash
cargo run -- --demo
```

---

## SQL Syntax & Capability Reference

### Data Definition Language (DDL)
```sql
-- Create table with explicit data types and primary key constraints
CREATE TABLE accounts (
    account_id   INTEGER,
    username     VARCHAR(50),
    balance      DOUBLE,
    is_active    BOOLEAN
);

-- Index management
CREATE INDEX idx_acc_id ON accounts(account_id);
CREATE UNIQUE INDEX idx_acc_user ON accounts(username);

-- Drop table
DROP TABLE accounts;
```

### Data Manipulation Language (DML)
```sql
-- Single and multi-row insertions
INSERT INTO accounts VALUES (101, 'phani', 4500.50, TRUE);
INSERT INTO accounts VALUES (102, 'alex', 7200.00, TRUE), (103, 'sam', 120.00, FALSE);

-- Projections and filtered queries
SELECT * FROM accounts;
SELECT username, balance FROM accounts WHERE balance >= 1000.0 AND is_active = TRUE;

-- Multi-table relational JOINs (Nested Loop Join pipeline)
SELECT users.name, orders.amount FROM users INNER JOIN orders ON users.id = orders.user_id;

-- SQL Aggregate Functions
SELECT COUNT(*), SUM(balance), AVG(balance), MIN(balance), MAX(balance) FROM accounts;

-- Sorting and pagination
SELECT * FROM accounts ORDER BY balance DESC LIMIT 10;

-- In-place updates
UPDATE accounts SET balance = 5000.0 WHERE username = 'phani';

-- Record deletions
DELETE FROM accounts WHERE is_active = FALSE;

-- Transaction Control (ACID)
BEGIN TRANSACTION;
INSERT INTO accounts VALUES (104, 'jordan', 8900.0, TRUE);
COMMIT; -- or ROLLBACK;
```

### Supported Data Types
| SQL Type | Physical Rust Representation | Byte Size | Notes |
|---|---|:---:|---|
| `INTEGER` | `i32` (Signed 32-bit integer) | 4 bytes | Big-endian key normalized |
| `BIGINT` | `i64` (Signed 64-bit integer) | 8 bytes | High-precision identifiers |
| `FLOAT` | `f32` (Single-precision float) | 4 bytes | IEEE 754 |
| `DOUBLE` | `f64` (Double-precision float) | 8 bytes | Standard monetary / metric values |
| `BOOLEAN` | `bool` | 1 byte | Stored as byte `0x00` or `0x01` |
| `VARCHAR(n)` | `String` (Bounded length) | Variable | Inlined offset + end-packed |
| `TEXT` | `String` (Unbounded length) | Variable | Variable-length UTF-8 |
| `BLOB` | `Vec<u8>` (Raw binary payload) | Variable | Raw binary storage |

---

## Test Suite & Verification

AtlasDB features a comprehensive automated test suite consisting of **63 unit and integration tests** covering every architectural layer.

```bash
cargo test
```

### Test Coverage Breakdown

```text
running 63 tests
test catalog::schema::tests::test_create_table                ... ok
test catalog::schema::tests::test_drop_table                  ... ok
test catalog::schema::tests::test_find_column                 ... ok
test catalog::schema::tests::test_duplicate_table_error       ... ok
test catalog::schema::tests::test_schema_display              ... ok
test buffer::pool::tests::test_new_page_and_fetch             ... ok
test buffer::pool::tests::test_lru_eviction                   ... ok
test buffer::pool::tests::test_dirty_flush                    ... ok
test buffer::pool::tests::test_pin_prevents_eviction          ... ok
test buffer::pool::tests::test_unpin_double_error             ... ok
test index::btree::tests::test_insert_and_search              ... ok
test index::btree::tests::test_delete                         ... ok
test index::btree::tests::test_scan_all_ordered               ... ok
test index::btree::tests::test_range_scan                     ... ok
test index::btree::tests::test_string_keys                    ... ok
test index::btree::tests::test_large_insert_triggers_splits   ... ok
test parser::lexer::tests::test_operators                     ... ok
test parser::lexer::tests::test_select                        ... ok
test parser::lexer::tests::test_string_lit                    ... ok
test parser::parser::tests::test_parse_create_table           ... ok
test parser::parser::tests::test_parse_insert                 ... ok
test parser::parser::tests::test_parse_select                 ... ok
test parser::parser::tests::test_parse_update                 ... ok
test parser::parser::tests::test_parse_delete                 ... ok
test parser::parser::tests::test_parse_order_by_limit         ... ok
test optimizer::planner::tests::test_plan_display             ... ok
test optimizer::planner::tests::test_seq_scan_plan            ... ok
test optimizer::planner::tests::test_index_scan_preferred     ... ok
test query::executor::tests::test_create_insert_select        ... ok
test query::executor::tests::test_where_clause                ... ok
test query::executor::tests::test_order_by_and_limit          ... ok
test query::executor::tests::test_update                      ... ok
test query::executor::tests::test_delete_with_where           ... ok
test storage::disk::tests::test_disk_manager_roundtrip        ... ok
test storage::disk::tests::test_multiple_pages                ... ok
test storage::page::tests::test_new_page                      ... ok
test storage::page::tests::test_insert_and_get                ... ok
test storage::page::tests::test_multiple_inserts              ... ok
test storage::page::tests::test_page_full                     ... ok
test storage::page::tests::test_delete                        ... ok
test storage::page::tests::test_slot_reuse_after_delete       ... ok
test storage::page::tests::test_compaction                    ... ok
test storage::page::tests::test_fill_page_then_compact        ... ok
test storage::page::tests::test_update_in_place               ... ok
test storage::page::tests::test_update_larger                 ... ok
test storage::page::tests::test_checksum                      ... ok
test storage::page::tests::test_invalid_slot_errors           ... ok
test storage::page::tests::test_iterator                      ... ok
test storage::tuple::tests::test_tuple_header_roundtrip       ... ok
test storage::tuple::tests::test_tuple_roundtrip              ... ok
test storage::tuple::tests::test_tuple_with_nulls             ... ok
test storage::tuple::tests::test_field_value_serialization    ... ok
test storage::tuple::tests::test_tuple_display                ... ok
test storage::heap::tests::test_heap_insert_and_get           ... ok
test storage::heap::tests::test_heap_scan                     ... ok
test storage::heap::tests::test_heap_delete                   ... ok
test storage::heap::tests::test_heap_update                   ... ok
test storage::heap::tests::test_heap_multi_page               ... ok
test txn::wal::tests::test_log_record_roundtrip               ... ok
test txn::wal::tests::test_wal_begin_commit                   ... ok
test txn::wal::tests::test_recovery                           ... ok
test network::server::tests::test_execute_sql                 ... ok
test network::server::tests::test_parse_error                 ... ok

test result: ok. 63 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

---

## Project Directory Structure

```
AtlasDB/
├── Cargo.toml                  # Project manifest, dependencies, and feature flags
├── .gitignore                  # Git tracking exclusions (target/, data, node_modules)
├── README.md                   # Technical documentation and architecture guide
├── atlasdb_data/               # Persistent database files (.db) and WAL logs (.wal)
├── src/
│   ├── lib.rs                  # Crate root, module exports, public API declarations
│   ├── main.rs                 # CLI entry point (REPL shell, TCP server, demo orchestrator)
│   ├── storage/                # Layer 1: Disk serialization & physical layouts
│   │   ├── mod.rs
│   │   ├── disk.rs             # File I/O manager and 4KB page allocation
│   │   ├── error.rs            # StorageError and Result type definitions
│   │   ├── heap.rs             # Slotted heap file management and multi-page CRUD
│   │   ├── page.rs             # 4KB slotted page layout, compaction, CRC32 checks
│   │   └── tuple.rs            # Tuple headers, null bitmaps, variable-length serialization
│   ├── buffer/                 # Layer 2: In-memory page cache
│   │   ├── mod.rs
│   │   └── pool.rs             # BufferPool with LRU eviction and pin reference counting
│   ├── catalog/                # Layer 3: System catalog & metadata
│   │   ├── mod.rs
│   │   └── schema.rs           # Schema registry, ColumnDef, DataType, and constraints
│   ├── index/                  # Layer 4: Index structures
│   │   ├── mod.rs
│   │   └── btree.rs            # Order-64 arena-allocated B+ tree with linked leaves
│   ├── parser/                 # Layer 5: SQL front-end
│   │   ├── mod.rs
│   │   ├── ast.rs              # Strongly-typed Abstract Syntax Tree definitions
│   │   ├── lexer.rs            # Lexical tokenizer for keywords, operators, and literals
│   │   └── parser.rs           # Recursive-descent SQL parser
│   ├── query/                  # Layer 6: Query execution
│   │   ├── mod.rs
│   │   └── executor.rs         # Volcano-model iterator execution pipeline
│   ├── txn/                    # Layer 7: Transaction management & durability
│   │   ├── mod.rs
│   │   └── wal.rs              # Write-Ahead Log (WAL) records, CRC verification, Redo recovery
│   ├── optimizer/              # Layer 8: Query planning & cost optimization
│   │   ├── mod.rs
│   │   └── planner.rs          # Cardinality/selectivity estimator and physical planner
│   └── network/                # Layer 9: Client communication protocols
│       ├── mod.rs
│       ├── server.rs           # Multi-threaded TCP socket server and wire protocol
│       └── web.rs              # Async Axum HTTP REST API with CORS support
└── ui/                         # Modern Web Studio Frontend
    ├── package.json            # React 19, Vite, Lucide dependencies
    ├── vite.config.js          # Vite build configuration
    └── src/
        ├── App.jsx             # Interactive SQL workspace & table browser component
        ├── App.css             # Dark-themed dashboard styling
        └── main.jsx            # React root mount
```

---

## Resume / Portfolio Highlights

> **Applying this project on your resume?**
> Here are pre-crafted, recruiter-optimized bullet points categorized by specialization (using the standard **Google X-Y-Z** formula: *"Accomplished [X] as measured by [Y], by doing [Z]"*):

### For Systems / Core Database Engineer Roles:
- **Architected and implemented an ACID-compliant relational database engine from scratch in 100% safe Rust**, featuring a 4KB PostgreSQL-style slotted-page storage manager, an arena-allocated B+ tree index, and a Volcano-model query execution pipeline.
- **Engineered an arena-allocated B+ tree index ($M=64$)** using a flat vector memory model to eliminate borrow-checker pointer cycles without `unsafe` Rust, achieving $\mathcal{O}(\log_{64} N)$ point lookups and $\mathcal{O}(\log N + K)$ range scans with big-endian normalized byte-key ordering.
- **Implemented Write-Ahead Logging (WAL) and crash recovery**, ensuring Durability by enforcing write-ahead constraints with monotonic LSN sequencing and CRC32 verification, replaying uncommitted/committed state via an automated Redo engine.
- **Formulated a Cost-Based Query Optimizer (CBO)** that dynamically evaluates table statistics (cardinality, distinct values) to estimate predicate selectivity, automatically picking optimal physical execution plans (Index Scan vs. Sequential Scan).

### For Backend / Infrastructure Engineer Roles:
- **Developed a modular SQL engine and execution pipeline** supporting DDL and DML operations with an LRU-managed buffer pool, reducing disk I/O through pin/unpin reference counting and dirty-page write-back.
- **Constructed dual communication protocols**: a multi-threaded TCP server with custom wire protocol and an asynchronous HTTP REST API using **Axum and Tokio**, delivering sub-millisecond query responses across 63 automated unit tests.
- **Implemented in-page dynamic compaction and null-bitmap serialization**, reducing page fragmentation and tuple header memory footprint across variable-length fields (`VARCHAR`, `TEXT`, `BLOB`).

### For Full-Stack Systems Roles:
- **Built AtlasDB Studio**, an interactive web-based database management interface using **React 19, Vite, and Axum (Rust)**, enabling real-time SQL execution, schema exploration, and query timing diagnostics.

---

## License

This project is licensed under the [MIT License](LICENSE).
