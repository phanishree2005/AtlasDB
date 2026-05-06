//! # TCP Server
//!
//! A multi-threaded TCP server that accepts SQL commands
//! from clients over a simple text-based protocol.
//!
//! ## Protocol
//! - Client sends a SQL statement terminated by newline
//! - Server responds with the query result terminated by `\n---END---\n`
//! - Client sends `.quit` to disconnect

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::catalog::schema::Catalog;
use crate::parser::parser::parse_sql;
use crate::query::executor::{Executor, QueryResult};

/// End-of-response marker.
const END_MARKER: &str = "\n---END---\n";

/// AtlasDB TCP server.
pub struct Server {
    addr: String,
    executor: Arc<Mutex<Executor>>,
}

impl Server {
    /// Create a new server (does not start listening).
    pub fn new(addr: &str, catalog: Catalog) -> Self {
        Self {
            addr: addr.to_string(),
            executor: Arc::new(Mutex::new(Executor::new(catalog))),
        }
    }

    /// Start listening and accepting connections.
    /// This blocks the calling thread.
    pub fn start(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(&self.addr)?;
        println!("AtlasDB server listening on {}", self.addr);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let executor = Arc::clone(&self.executor);
                    thread::spawn(move || {
                        if let Err(e) = Self::handle_client(stream, executor) {
                            eprintln!("Client error: {}", e);
                        }
                    });
                }
                Err(e) => eprintln!("Accept error: {}", e),
            }
        }
        Ok(())
    }

    /// Start the server with a non-blocking option (returns the listener).
    pub fn bind(&self) -> std::io::Result<TcpListener> {
        let listener = TcpListener::bind(&self.addr)?;
        Ok(listener)
    }

    /// Handle a single client connection.
    fn handle_client(
        stream: TcpStream,
        executor: Arc<Mutex<Executor>>,
    ) -> std::io::Result<()> {
        let peer = stream.peer_addr()?;
        println!("Client connected: {}", peer);

        let mut reader = BufReader::new(stream.try_clone()?);
        let mut writer = stream;

        // Send welcome banner
        writer.write_all(b"AtlasDB v0.1.0\nReady.\n")?;
        writer.flush()?;

        loop {
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                println!("Client disconnected: {}", peer);
                break;
            }

            let sql = line.trim();
            if sql.is_empty() {
                continue;
            }

            if sql == ".quit" || sql == "\\q" || sql.to_uppercase() == "EXIT" {
                writer.write_all(b"Bye!\n")?;
                writer.flush()?;
                println!("Client disconnected: {}", peer);
                break;
            }

            if sql == ".tables" {
                let exec = executor.lock().unwrap();
                let tables = exec.catalog().list_tables();
                let response = if tables.is_empty() {
                    "No tables.\n".to_string()
                } else {
                    tables.join("\n") + "\n"
                };
                writer.write_all(response.as_bytes())?;
                writer.write_all(END_MARKER.as_bytes())?;
                writer.flush()?;
                continue;
            }

            // Parse and execute
            let result = match parse_sql(sql) {
                Ok(stmt) => {
                    let mut exec = executor.lock().unwrap();
                    exec.execute(stmt)
                }
                Err(e) => QueryResult::Error(format!("Parse error: {}", e)),
            };

            let response = format!("{}\n", result);
            writer.write_all(response.as_bytes())?;
            writer.write_all(END_MARKER.as_bytes())?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Process a single SQL command (for REPL / embedded use).
    pub fn execute_sql(&self, sql: &str) -> String {
        let result = match parse_sql(sql) {
            Ok(stmt) => {
                let mut exec = self.executor.lock().unwrap();
                exec.execute(stmt)
            }
            Err(e) => QueryResult::Error(format!("Parse error: {}", e)),
        };
        format!("{}", result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_server(tmp: &TempDir) -> Server {
        let catalog = Catalog::new(tmp.path().to_str().unwrap());
        Server::new("127.0.0.1:0", catalog)
    }

    #[test]
    fn test_execute_sql() {
        let tmp = TempDir::new().unwrap();
        let server = test_server(&tmp);

        let r = server.execute_sql("CREATE TABLE srv (id INTEGER, name TEXT)");
        assert!(r.contains("CREATE TABLE"));

        let r = server.execute_sql("INSERT INTO srv VALUES (1, 'Alice')");
        assert!(r.contains("INSERT"));

        let r = server.execute_sql("SELECT * FROM srv");
        assert!(r.contains("Alice"));
    }

    #[test]
    fn test_parse_error() {
        let tmp = TempDir::new().unwrap();
        let server = test_server(&tmp);
        let r = server.execute_sql("SELEC broken sql");
        assert!(r.contains("ERROR"));
    }
}
