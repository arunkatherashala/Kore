use axum::{
    extract::{Path, State, Multipart},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;

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
pub struct UploadResponse {
    pub file_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub compressed_bytes: u64,
    pub compression_ratio: f64,
    pub compression_method: String,
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

pub struct FileEntry {
    pub metadata: FileMetadata,
    pub compressed_data: Vec<u8>,
}

pub struct AppState {
    start_time: std::time::Instant,
    files_count: std::sync::atomic::AtomicUsize,
    files: Mutex<HashMap<String, FileEntry>>,
    total_bytes: std::sync::atomic::AtomicU64,
    total_compressed: std::sync::atomic::AtomicU64,
}

impl AppState {
    fn new() -> Self {
        AppState {
            start_time: std::time::Instant::now(),
            files_count: std::sync::atomic::AtomicUsize::new(0),
            files: Mutex::new(HashMap::new()),
            total_bytes: std::sync::atomic::AtomicU64::new(0),
            total_compressed: std::sync::atomic::AtomicU64::new(0),
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

    fn add_bytes(&self, original: u64, compressed: u64) {
        self.total_bytes.fetch_add(original, std::sync::atomic::Ordering::SeqCst);
        self.total_compressed.fetch_add(compressed, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_total_bytes(&self) -> u64 {
        self.total_bytes.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn get_total_compressed(&self) -> u64 {
        self.total_compressed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn store_file(&self, file_id: String, entry: FileEntry) {
        if let Ok(mut files) = self.files.lock() {
            files.insert(file_id, entry);
        }
    }

    fn list_all_files(&self) -> Vec<FileMetadata> {
        if let Ok(files) = self.files.lock() {
            files.values().map(|e| e.metadata.clone()).collect()
        } else {
            vec![]
        }
    }

    fn get_file_metadata(&self, file_id: &str) -> Option<FileMetadata> {
        if let Ok(files) = self.files.lock() {
            files.get(file_id).map(|e| e.metadata.clone())
        } else {
            None
        }
    }
}

// ============ HANDLERS ============

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

async fn list_files(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let files = state.list_all_files();
    Json(ListFilesResponse {
        total: files.len(),
        files,
    })
}

async fn get_file_info(
    Path(file_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.get_file_metadata(&file_id) {
        Some(metadata) => (StatusCode::OK, Json(json!(metadata))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "File not found"}))).into_response(),
    }
}

async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.ok().flatten() {
        if let Ok(bytes) = field.bytes().await {
            let data = bytes.to_vec();
            let file_id = Uuid::new_v4().to_string();
            let original_size = data.len() as u64;
            
            // Placeholder: In production, call kore_compression::compress_hybrid(data)
            // For now, store as-is (0% compression)
            let compressed = data.clone();
            let compressed_size = compressed.len() as u64;
            let compression_ratio = if original_size > 0 {
                1.0 - (compressed_size as f64 / original_size as f64).max(0.0)
            } else {
                0.0
            };

            let metadata = FileMetadata {
                file_id: file_id.clone(),
                filename: format!("upload_{}.kore", file_id),
                size_bytes: original_size,
                compressed_bytes: compressed_size,
                compression_ratio,
                uploaded_at: Utc::now().to_rfc3339(),
                compression_method: "Stored".to_string(),
            };

            let entry = FileEntry {
                metadata: metadata.clone(),
                compressed_data: compressed,
            };

            state.store_file(file_id.clone(), entry);
            state.increment_files();
            state.add_bytes(original_size, compressed_size);

            return (
                StatusCode::CREATED,
                Json(UploadResponse {
                    file_id,
                    filename: metadata.filename,
                    size_bytes: original_size,
                    compressed_bytes: compressed_size,
                    compression_ratio,
                    compression_method: "Stored".to_string(),
                }),
            )
                .into_response();
        }
    }

    (StatusCode::BAD_REQUEST, Json(json!({"error": "No file provided"}))).into_response()
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(StatusResponse {
        status: "healthy".to_string(),
        version: "1.0.0".to_string(),
        files_stored: state.get_files_count(),
        total_bytes: state.get_total_bytes(),
        total_compressed: state.get_total_compressed(),
        uptime_seconds: state.get_uptime(),
    })
}

// ============ MAIN ============

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app_state = AppState::new();

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/files/list", get(list_files))
        .route("/api/v1/files/:file_id/info", get(get_file_info))
        .route("/api/v1/files/upload", post(upload_file))
        .route("/api/v1/status", get(get_status))
        .with_state(Arc::new(app_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000")
        .await
        .expect("Failed to bind to port 8000");

    println!("🚀 Kore Cloud API v1.0.0 running on http://0.0.0.0:8000");
    println!();
    println!("📋 Endpoints:");
    println!("   ✓ GET  /health");
    println!("   ✓ POST /api/v1/files/upload");
    println!("   ✓ GET  /api/v1/files/list");
    println!("   ✓ GET  /api/v1/files/{{file_id}}/info");
    println!("   ✓ GET  /api/v1/status");
    println!();

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_metadata_creation() {
        let meta = FileMetadata {
            file_id: "test-id".to_string(),
            filename: "test.kore".to_string(),
            size_bytes: 1000,
            compressed_bytes: 500,
            compression_ratio: 0.5,
            uploaded_at: "2024-01-01T00:00:00Z".to_string(),
            compression_method: "Zstd".to_string(),
        };
        assert_eq!(meta.size_bytes, 1000);
        assert_eq!(meta.compression_ratio, 0.5);
    }

    #[test]
    fn test_app_state_tracking() {
        let state = AppState::new();
        state.increment_files();
        state.increment_files();
        assert_eq!(state.get_files_count(), 2);
        
        state.add_bytes(1000, 500);
        assert_eq!(state.get_total_bytes(), 1000);
        assert_eq!(state.get_total_compressed(), 500);
    }
}
