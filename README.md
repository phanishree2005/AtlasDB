# AtlasDB 🗄️

A fully-featured **relational database engine built from scratch in Rust** — implementing all core layers of a production database system inspired by PostgreSQL.

---

## Features

| Phase | Layer | What's Built |
|-------|-------|-------------|
| 1 | **Storage Engine** | Slotted pages, tuple serialization, null bitmap, CRC32 checksums, disk manager, heap file |
| 2 | **Buffer Manager** | LRU page cache, pin/unpin reference counting, dirty write-back |
| 3 | **Catalog System** | In-memory schema registry, table/column/index metadata |
| 4 | **B+ Tree Index** | Point lookup, range scan, linked leaf nodes |
| 5 | **SQL Lexer** | Tokenizer for full SQL keyword/operator/literal set |
| 6 | **SQL Parser + Executor** | Recursive-descent parser, Volcano-model query engine (SELECT/INSERT/UPDATE/DELETE with WHERE, ORDER BY, LIMIT) |
| 7 | **Write-Ahead Log** | Transaction begin/commit/abort, CRC32 log records, crash recovery (redo) |
| 8 | **Query Optimizer** | Cost-based planner, selectivity estimation, index vs sequential scan |
| 9 | **Network Server** | Multi-threaded TCP server, text-based wire protocol, interactive REPL |

**63 unit tests — all passing.**

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows, download and run [rustup-init.exe](https://rustup.rs/).

---

### Build

```bash
cd AtlasDB
cargo build
```

---

### Run

#### Interactive Web UI (New)
A premium dark-themed dashboard for managing schemas and executing queries visually.
```bash
# 1. Start the backend with web flag
cargo run -- --web 3000

# 2. In ui folder, start the frontend
cd ui
npm install
npm run dev
```
Open `http://localhost:5173` to access the dashboard.

#### Interactive SQL REPL
```bash
cargo run
```
```
=== AtlasDB v0.1.0 — Interactive SQL Shell ===
atlasdb> CREATE TABLE users (id INTEGER, name TEXT, age INTEGER);
atlasdb> INSERT INTO users VALUES (1, 'Alice', 30), (2, 'Bob', 25);
atlasdb> SELECT * FROM users WHERE age >= 28;
atlasdb> UPDATE users SET age = 31 WHERE name = 'Alice';
atlasdb> DELETE FROM users WHERE id = 2;
atlasdb> SELECT * FROM users ORDER BY name ASC;
atlasdb> .tables
atlasdb> .quit
```

#### Full System Demo (all 9 phases)
```bash
cargo run -- --demo
```

#### TCP Server
```bash
cargo run -- --server 5433
```
Connect from another terminal:
```bash
# Linux/macOS
nc localhost 5433

# Windows PowerShell
$tcp = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 5433)
```

#### Run All Tests
```bash
cargo test
```

---

## SQL Reference

### DDL

```sql
-- Create a table
CREATE TABLE employees (
    id      INTEGER,
    name    VARCHAR(100),
    salary  DOUBLE,
    active  BOOLEAN
);

-- Drop a table
DROP TABLE employees;

-- Create an index
CREATE INDEX idx_emp_id ON employees(id);
CREATE UNIQUE INDEX idx_emp_name ON employees(name);
```

### DML

```sql
-- Insert rows
INSERT INTO employees VALUES (1, 'Alice', 85000.0, TRUE);
INSERT INTO employees VALUES (1, 'Alice', 85000.0, TRUE), (2, 'Bob', 92000.0, FALSE);

-- Select
SELECT * FROM employees;
SELECT name, salary FROM employees WHERE salary > 80000.0;
SELECT * FROM employees WHERE name = 'Alice' AND active = TRUE;
SELECT * FROM employees ORDER BY salary DESC LIMIT 10;

-- Update
UPDATE employees SET salary = 95000.0 WHERE name = 'Alice';

-- Delete
DELETE FROM employees WHERE id = 2;
```

### Supported Data Types

| SQL Type | Description |
|----------|-------------|
| `INTEGER` | 32-bit signed integer |
| `BIGINT` | 64-bit signed integer |
| `FLOAT` | 32-bit floating point |
| `DOUBLE` | 64-bit floating point |
| `BOOLEAN` | true / false |
| `VARCHAR(n)` | Variable-length string, max n chars |
| `TEXT` | Unlimited string |
| `BLOB` | Binary data |

### REPL Commands

| Command | Action |
|---------|--------|
| `.tables` | List all tables |
| `.quit` or `\q` | Exit the shell |

---

## Architecture

```
┌─────────────────────────────────────────────┐
│              Client (REPL / TCP)            │
├─────────────────────────────────────────────┤
│  Phase 9: Network Layer (TCP server)        │
├─────────────────────────────────────────────┤
│  Phase 5+6: SQL Parser + Query Executor     │
│    Lexer → AST → Volcano Iterator Pipeline  │
├──────────────────┬──────────────────────────┤
│ Phase 8:         │  Phase 3: Catalog        │
│ Query Optimizer  │  (Schema metadata)       │
├──────────────────┼──────────────────────────┤
│ Phase 4:         │  Phase 7: WAL            │
│ B+ Tree Index    │  (Transactions/Recovery) │
├──────────────────┴──────────────────────────┤
│  Phase 2: Buffer Manager (LRU cache)        │
├─────────────────────────────────────────────┤
│  Phase 1: Storage Engine                    │
│    Slotted Pages │ Tuples │ Disk Manager    │
│    Heap File     │ Compaction │ Checksums   │
├─────────────────────────────────────────────┤
│                   Disk                      │
└─────────────────────────────────────────────┘
```

---

## Project Structure

```
AtlasDB/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                  # Library root, module declarations
    ├── main.rs                 # Binary: REPL, server, demo
    ├── storage/
    │   ├── mod.rs
    │   ├── page.rs             # Slotted page (insert/delete/update/compact)
    │   ├── tuple.rs            # Tuple serialization + null bitmap
    │   ├── disk.rs             # Disk manager (file I/O)
    │   ├── heap.rs             # Heap file (table-level CRUD)
    │   └── error.rs            # StorageError types
    ├── buffer/
    │   ├── mod.rs
    │   └── pool.rs             # LRU buffer pool
    ├── catalog/
    │   ├── mod.rs
    │   └── schema.rs           # DataType, ColumnDef, TableSchema, Catalog
    ├── index/
    │   ├── mod.rs
    │   └── btree.rs            # B+ tree index
    ├── parser/
    │   ├── mod.rs
    │   ├── lexer.rs            # SQL tokenizer
    │   ├── ast.rs              # AST node definitions
    │   └── parser.rs           # Recursive-descent SQL parser
    ├── query/
    │   ├── mod.rs
    │   └── executor.rs         # Volcano-model query executor
    ├── txn/
    │   ├── mod.rs
    │   └── wal.rs              # Write-Ahead Log
    ├── optimizer/
    │   ├── mod.rs
    │   └── planner.rs          # Cost-based query planner
    └── network/
        ├── mod.rs
        └── server.rs           # TCP server + wire protocol
```

---

## Key Design Decisions

- **Slotted Page Layout** (PostgreSQL-style): slot array grows downward, tuple data grows upward. Enables compaction without invalidating external `TupleId` references.
- **Null Bitmap**: compact per-tuple bitmap for NULL fields, identical to PostgreSQL's approach.
- **B+ Tree Arena Allocation**: all nodes stored in a flat `Vec<BTreeNode>` indexed by `usize` — avoids Rust lifetime complexity with self-referential pointers.
- **Volcano Iterator Model**: each query operator (`SeqScan`, `Filter`, `Project`, `Sort`) is lazy and composable.
- **WAL Protocol**: log records are flushed before data pages; recovery replays only committed transactions (redo-only for now).
- **Cost-Based Optimizer**: uses per-table statistics (row count, page count, column distinct values) to choose between index scan and sequential scan.

---

## License

MIT
