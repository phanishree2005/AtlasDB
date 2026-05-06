//! AtlasDB — A relational database engine from scratch.
//!
//! Usage:
//!   atlasdb                  — Start interactive REPL
//!   atlasdb --server [port]  — Start TCP server
//!   atlasdb --demo           — Run comprehensive demo

use std::io::{self, BufRead, Write};

use atlasdb::catalog::schema::Catalog;
use atlasdb::network::server::Server;
use atlasdb::parser::parser::parse_sql;
use atlasdb::query::executor::Executor;

#[tokio::main]
async fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "--demo" {
        run_demo();
        return;
    }

    if args.len() > 1 && args[1] == "--server" {
        let port = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(5433);
        run_server(port);
        return;
    }

    if args.len() > 1 && args[1] == "--web" {
        let port = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(3000);
        run_web_server(port).await;
        return;
    }

    // Default: interactive REPL
    run_repl();
}

async fn run_web_server(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let db_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("atlasdb_data");
    let _ = std::fs::create_dir_all(&db_path);

    let catalog = Catalog::new(db_path.to_str().unwrap());
    println!("Starting AtlasDB Web UI API on {}...", addr);
    if let Err(e) = atlasdb::network::web::start_web_server(&addr, catalog).await {
        eprintln!("Web server error: {}", e);
    }
}

fn run_repl() {
    println!("=== AtlasDB v0.1.0 — Interactive SQL Shell ===");
    println!("Type SQL statements, '.tables' to list tables, '.quit' to exit.\n");

    let db_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("atlasdb_data");
    let _ = std::fs::create_dir_all(&db_path);

    let catalog = Catalog::new(db_path.to_str().unwrap());
    let mut executor = Executor::new(catalog);

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("atlasdb> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break;
        }

        let sql = line.trim();
        if sql.is_empty() { continue; }
        if sql == ".quit" || sql == "\\q" || sql.to_uppercase() == "EXIT" {
            println!("Bye!");
            break;
        }
        if sql == ".tables" {
            let tables = executor.catalog().list_tables();
            if tables.is_empty() {
                println!("No tables.");
            } else {
                for t in tables { println!("  {}", t); }
            }
            continue;
        }

        match parse_sql(sql) {
            Ok(stmt) => {
                let result = executor.execute(stmt);
                println!("{}\n", result);
            }
            Err(e) => println!("ERROR: {}\n", e),
        }
    }
}

fn run_server(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let db_path = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("atlasdb_data");
    let _ = std::fs::create_dir_all(&db_path);

    let catalog = Catalog::new(db_path.to_str().unwrap());
    let server = Server::new(&addr, catalog);

    println!("Starting AtlasDB TCP server on {}...", addr);
    if let Err(e) = server.start() {
        eprintln!("Server error: {}", e);
    }
}

fn run_demo() {
    println!("=== AtlasDB Full System Demo ===\n");

    // --- Phase 1: Storage Engine ---
    println!("--- Phase 1: Storage Engine ---");
    demo_storage();

    // --- Phase 2: Buffer Manager ---
    println!("\n--- Phase 2: Buffer Manager ---");
    demo_buffer();

    // --- Phase 3: Catalog ---
    println!("\n--- Phase 3: Catalog System ---");
    demo_catalog();

    // --- Phase 4: B+ Tree Index ---
    println!("\n--- Phase 4: B+ Tree Index ---");
    demo_index();

    // --- Phase 5+6: SQL Parser + Query Engine ---
    println!("\n--- Phase 5+6: SQL Parser + Query Execution ---");
    demo_query_engine();

    // --- Phase 7: WAL / Transactions ---
    println!("\n--- Phase 7: Write-Ahead Log ---");
    demo_wal();

    // --- Phase 8: Query Optimizer ---
    println!("\n--- Phase 8: Query Optimizer ---");
    demo_optimizer();

    // --- Phase 9: Network ---
    println!("\n--- Phase 9: Network Server ---");
    demo_network();

    println!("\n=== All 9 Phases Complete ===");
}

fn demo_storage() {
    use atlasdb::storage::page::Page;
    use atlasdb::storage::tuple::{Tuple, FieldValue};

    let mut page = Page::new(0);
    println!("  Created page: {:?}", page);

    let t1 = Tuple::new(1, vec![
        FieldValue::Int32(1),
        FieldValue::String("Alice".into()),
        FieldValue::Float64(95.5),
    ]);
    let mut buf = vec![0u8; t1.serialized_size()];
    t1.serialize(&mut buf).unwrap();
    let s0 = page.insert(&buf).unwrap();
    println!("  Inserted {} -> slot {}", t1, s0);

    println!("  Free space: {} bytes", page.free_space());
    page.compute_checksum();
    println!("  Checksum valid: {}", page.verify_checksum());
}

fn demo_buffer() {
    use atlasdb::buffer::pool::BufferPool;
    use atlasdb::storage::disk::DiskManager;

    let tmp = std::env::temp_dir().join("atlasdb_demo_buffer.db");
    let dm = DiskManager::open(&tmp).unwrap();
    let mut pool = BufferPool::new(dm, 8);

    let (pid, page) = pool.new_page().unwrap();
    page.insert(b"buffered data").unwrap();
    pool.unpin(pid, true).unwrap();
    pool.flush_all().unwrap();

    let page = pool.fetch_page(pid).unwrap();
    println!("  Buffer pool fetch: {:?}", std::str::from_utf8(page.get(0).unwrap()).unwrap());
    pool.unpin(pid, false).unwrap();
    println!("  Buffer pool size: {} pages cached", pool.size());

    let _ = std::fs::remove_file(&tmp);
}

fn demo_catalog() {
    use atlasdb::catalog::schema::{Catalog, ColumnDef, DataType};

    let mut catalog = Catalog::new("/tmp/demo");
    catalog.create_table("students", vec![
        ColumnDef::new("id", DataType::Integer, false).primary_key(),
        ColumnDef::new("name", DataType::Varchar(100), false),
        ColumnDef::new("gpa", DataType::Double, true),
    ]).unwrap();

    let schema = catalog.get_table("students").unwrap();
    println!("  {}", schema);
    println!("  Primary key: column {}", schema.primary_key_index().unwrap());
}

fn demo_index() {
    use atlasdb::index::btree::{BTree, IndexKey};
    use atlasdb::storage::page::TupleId;

    let mut tree = BTree::new("idx_student_id");

    for i in 0..100 {
        tree.insert(
            IndexKey::from_i32(i),
            TupleId { page_id: (i / 50) as u32, slot_id: (i % 50) as u16 },
        );
    }

    println!("  Inserted 100 keys, tree height: {}", tree.height());

    let found = tree.search(&IndexKey::from_i32(42)).unwrap();
    println!("  Search key=42: page={}, slot={}", found.page_id, found.slot_id);

    let range = tree.range_scan(&IndexKey::from_i32(10), &IndexKey::from_i32(20));
    println!("  Range [10,20]: {} results", range.len());
}

fn demo_query_engine() {
    use atlasdb::catalog::schema::Catalog;
    use atlasdb::query::executor::Executor;

    let dir = std::env::temp_dir().join("atlasdb_demo_qe");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("employees.db"));

    let catalog = Catalog::new(dir.to_str().unwrap());
    let mut exec = Executor::new(catalog);

    // CREATE TABLE
    let r = exec.execute(parse_sql("CREATE TABLE employees (id INTEGER, name TEXT, salary FLOAT)").unwrap());
    println!("  {}", r);

    // INSERT
    let r = exec.execute(parse_sql("INSERT INTO employees VALUES (1, 'Alice', 85000.0), (2, 'Bob', 92000.0), (3, 'Charlie', 78000.0)").unwrap());
    println!("  {}", r);

    // SELECT with WHERE
    let r = exec.execute(parse_sql("SELECT * FROM employees WHERE salary >= 80000.0").unwrap());
    println!("  {}", r);

    // UPDATE
    let r = exec.execute(parse_sql("UPDATE employees SET salary = 95000.0 WHERE name = 'Alice'").unwrap());
    println!("  {}", r);

    // DELETE
    let r = exec.execute(parse_sql("DELETE FROM employees WHERE id = 3").unwrap());
    println!("  {}", r);

    // Final SELECT
    let r = exec.execute(parse_sql("SELECT name, salary FROM employees ORDER BY salary DESC").unwrap());
    println!("  {}", r);
}

fn demo_wal() {
    use atlasdb::txn::wal::{WAL, LogRecordType};

    let wal_path = std::env::temp_dir().join("atlasdb_demo.wal");
    let _ = std::fs::remove_file(&wal_path);

    {
        let mut wal = WAL::open(&wal_path).unwrap();

        let t1 = wal.begin().unwrap();
        wal.log_operation(t1, LogRecordType::Insert, "users", 0, 0, b"alice_data").unwrap();
        wal.commit(t1).unwrap();

        let t2 = wal.begin().unwrap();
        wal.log_operation(t2, LogRecordType::Insert, "users", 0, 1, b"bob_data").unwrap();
        wal.abort(t2).unwrap();

        wal.flush().unwrap();
    }

    // Recovery
    let path_str = wal_path.to_str().unwrap();
    let records = WAL::read_all_records(path_str).unwrap();
    println!("  WAL records written: {}", records.len());

    let redo = WAL::recover(path_str).unwrap();
    println!("  Recovery redo operations: {} (aborted txn filtered out)", redo.len());

    let _ = std::fs::remove_file(&wal_path);
}

fn demo_optimizer() {
    use atlasdb::optimizer::planner::{Planner, PlanNode, TableStats};

    let mut planner = Planner::new();

    let mut stats = TableStats::new("employees", 100000, 5000, 120);
    stats.column_distinct.push(("id".to_string(), 100000));
    stats.column_distinct.push(("dept".to_string(), 20));
    planner.update_stats(stats);
    planner.register_index("idx_emp_id", "employees", "id");

    // Index scan chosen for high-selectivity column
    let plan1 = planner.plan_scan("employees", Some("id"));
    println!("  Plan (filter on indexed 'id'):");
    println!("    {}", plan1);

    // Seq scan for no filter
    let plan2 = planner.plan_scan("employees", None);
    println!("  Plan (no filter):");
    println!("    {}", plan2);

    // Full plan tree
    let full_plan = PlanNode::Limit {
        count: 10,
        child: Box::new(PlanNode::Sort {
            column: "name".to_string(),
            ascending: true,
            child: Box::new(plan1),
        }),
    };
    println!("  Full plan:\n{}", full_plan);
}

fn demo_network() {
    println!("  TCP server available via: atlasdb --server 5433");
    println!("  Protocol: send SQL over TCP, receive results");
    println!("  Connect with: nc localhost 5433");

    // Quick test via the embedded executor
    let dir = std::env::temp_dir().join("atlasdb_demo_net");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("net_test.db"));

    let catalog = Catalog::new(dir.to_str().unwrap());
    let server = Server::new("127.0.0.1:0", catalog);
    let r = server.execute_sql("CREATE TABLE net_test (id INTEGER, val TEXT)");
    println!("  Embedded exec: {}", r);
    let r = server.execute_sql("INSERT INTO net_test VALUES (1, 'network_ok')");
    println!("  Embedded exec: {}", r);
    let r = server.execute_sql("SELECT * FROM net_test");
    println!("  Embedded exec: {}", r);
}
