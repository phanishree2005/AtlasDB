//! # Web API Layer
//!
//! Provides an HTTP API for AtlasDB, allowing the web UI to interact
//! with the database engine.

use axum::{
    routing::{get, post},
    Json, Router, extract::State,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};
use crate::query::executor::{Executor, QueryResult};
use crate::catalog::schema::Catalog;
use crate::parser::parser::parse_sql;

/// Shared application state.
pub struct AppState {
    pub executor: Arc<Mutex<Executor>>,
}

/// SQL query request.
#[derive(Deserialize)]
pub struct QueryRequest {
    pub sql: String,
}

/// SQL query response.
#[derive(Serialize)]
pub struct QueryResponse {
    pub success: bool,
    pub data: Option<SerializedRows>,
    pub message: String,
}

/// Serialized version of QueryResult for JSON.
#[derive(Serialize)]
pub struct SerializedRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Start the HTTP server.
pub async fn start_web_server(addr: &str, catalog: Catalog) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState {
        executor: Arc::new(Mutex::new(Executor::new(catalog))),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/query", post(handle_query))
        .route("/tables", get(handle_list_tables))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("AtlasDB Web UI API listening on http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn handle_query(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let mut exec = match state.executor.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    
    let result = match parse_sql(&payload.sql) {
        Ok(stmt) => {
            let exec_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                exec.execute(stmt)
            }));
            match exec_res {
                Ok(r) => r,
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Internal execution error".to_string()
                    };
                    QueryResult::Error(format!("Execution panic: {}", msg))
                }
            }
        }
        Err(e) => QueryResult::Error(format!("Parse error: {}", e)),
    };

    match result {
        QueryResult::Rows { columns, rows } => {
            let serialized_rows = rows.into_iter().map(|row| {
                row.into_iter().map(|v| format!("{}", v)).collect()
            }).collect();
            
            Json(QueryResponse {
                success: true,
                data: Some(SerializedRows { columns, rows: serialized_rows }),
                message: "Query executed successfully".to_string(),
            })
        }
        QueryResult::Modified { count, message } => {
            Json(QueryResponse {
                success: true,
                data: None,
                message: format!("{} ({} rows affected)", message, count),
            })
        }
        QueryResult::Success(msg) => {
            Json(QueryResponse {
                success: true,
                data: None,
                message: msg,
            })
        }
        QueryResult::Error(msg) => {
            Json(QueryResponse {
                success: false,
                data: None,
                message: format!("Error: {}", msg),
            })
        }
    }
}

async fn handle_list_tables(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<String>> {
    let exec = match state.executor.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    Json(exec.catalog().list_tables().into_iter().map(|s| s.to_string()).collect())
}
