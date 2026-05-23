use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app_state = AppState::new();

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/files/list", get(list_files))
        .route("/api/v1/files/:file_id/info", get(get_file_info))
        .route("/api/v1/status", get(get_status))
        .with_state(Arc::new(app_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000")
        .await
        .expect("Failed to bind to port 8000");

    println!("🚀 Kore Cloud API running on http://0.0.0.0:8000");
    println!("   Health check: http://0.0.0.0:8000/health");
    println!("   List files: http://0.0.0.0:8000/api/v1/files/list");
    println!("   Status: http://0.0.0.0:8000/api/v1/status");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}

// ============ MODELS ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub compressed_bytes: u64,
    pub compression_ratio: f64,
    pub uploaded_at: String,
    pub compression_method: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListFilesResponse {
    pub files: Vec<FileMetadata>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub files_stored: usize,
    pub total_bytes: u64,
    pub total_compressed: u64,
    pub uptime_seconds: u64,
}

// ============ STATE ============

pub struct AppState {
    start_time: std::time::Instant,
    files_count: std::sync::atomic::AtomicUsize,
}

impl AppState {
    fn new() -> Self {
        AppState {
            start_time: std::time::Instant::now(),
            files_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn increment_files(&self) {
        self.files_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_files_count(&self) -> usize {
        self.files_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn get_uptime(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

// ============ HANDLERS ============

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

async fn list_files(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(ListFilesResponse {
        files: vec![
            FileMetadata {
                file_id: Uuid::new_v4().to_string(),
                filename: "sample_data.kore".to_string(),
                size_bytes: 1_000_000,
                compressed_bytes: 150_000,
                compression_ratio: 0.85,
                uploaded_at: Utc::now().to_rfc3339(),
                compression_method: "Hybrid".to_string(),
            },
        ],
        total: 1,
    })
}

async fn get_file_info(
    Path(file_id): Path<String>,
    State(_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    Json(FileMetadata {
        file_id,
        filename: "data.kore".to_string(),
        size_bytes: 1_000_000,
        compressed_bytes: 150_000,
        compression_ratio: 0.85,
        uploaded_at: Utc::now().to_rfc3339(),
        compression_method: "Hybrid".to_string(),
    })
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(StatusResponse {
        status: "operational".to_string(),
        version: "0.1.0".to_string(),
        files_stored: state.get_files_count(),
        total_bytes: 5_000_000,
        total_compressed: 750_000,
        uptime_seconds: state.get_uptime(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_metadata_creation() {
        let meta = FileMetadata {
            file_id: "test-id".to_string(),
            filename: "test.kore".to_string(),
            size_bytes: 100,
            compressed_bytes: 20,
            compression_ratio: 0.8,
            uploaded_at: Utc::now().to_rfc3339(),
            compression_method: "Zstd".to_string(),
        };
        assert_eq!(meta.file_id, "test-id");
        assert_eq!(meta.compression_ratio, 0.8);
    }

    #[test]
    fn test_app_state() {
        let state = AppState::new();
        assert_eq!(state.get_files_count(), 0);
        state.increment_files();
        assert_eq!(state.get_files_count(), 1);
    }
}
