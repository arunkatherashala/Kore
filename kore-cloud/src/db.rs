/// PostgreSQL database integration for file metadata persistence
/// Provides connection pooling and CRUD operations

#[cfg(feature = "postgres")]
use sqlx::postgres::PgPoolOptions;
#[cfg(feature = "postgres")]
use sqlx::{FromRow, Row};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Database connection pool type
#[cfg(feature = "postgres")]
pub type DbPool = sqlx::PgPool;

/// File metadata in database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg(feature = "postgres")]
pub struct FileRecord {
    pub id: String,
    pub file_id: String,
    pub filename: String,
    pub size_bytes: i64,
    pub compressed_bytes: i64,
    pub compression_ratio: f64,
    pub compression_method: String,
    pub storage_backend: String,
    pub uploaded_at: DateTime<Utc>,
    pub etag: Option<String>,
}

/// Upload session tracking
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg(feature = "postgres")]
pub struct UploadSession {
    pub id: String,
    pub file_id: String,
    pub status: String, // "pending", "completed", "failed"
    pub total_chunks: i32,
    pub uploaded_chunks: i32,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Statistics record
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg(feature = "postgres")]
pub struct Stats {
    pub total_files: i64,
    pub total_bytes: i64,
    pub total_compressed: i64,
    pub updated_at: DateTime<Utc>,
}

/// Database error types
#[derive(Debug)]
pub enum DbError {
    #[cfg(feature = "postgres")]
    QueryError(sqlx::Error),
    NotConfigured,
    NotFound(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "postgres")]
            DbError::QueryError(e) => write!(f, "Query error: {}", e),
            DbError::NotConfigured => write!(f, "Database not configured (feature not enabled)"),
            DbError::NotFound(msg) => write!(f, "Not found: {}", msg),
        }
    }
}

impl std::error::Error for DbError {}

/// Database operations
#[cfg(feature = "postgres")]
pub struct Database {
    pool: DbPool,
}

#[cfg(feature = "postgres")]
impl Database {
    /// Create new database connection pool
    pub async fn new(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(DbError::QueryError)?;

        Ok(Database { pool })
    }

    /// Run migrations
    pub async fn migrate(&self) -> Result<(), DbError> {
        // Create tables if they don't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                file_id VARCHAR NOT NULL UNIQUE,
                filename VARCHAR NOT NULL,
                size_bytes BIGINT NOT NULL,
                compressed_bytes BIGINT NOT NULL,
                compression_ratio DOUBLE PRECISION NOT NULL,
                compression_method VARCHAR NOT NULL,
                storage_backend VARCHAR NOT NULL,
                uploaded_at TIMESTAMP WITH TIME ZONE NOT NULL,
                etag VARCHAR,
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS upload_sessions (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                file_id VARCHAR NOT NULL,
                status VARCHAR NOT NULL,
                total_chunks INTEGER NOT NULL,
                uploaded_chunks INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
                completed_at TIMESTAMP WITH TIME ZONE
            );
            CREATE INDEX IF NOT EXISTS idx_upload_sessions_file_id ON upload_sessions(file_id);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS stats (
                id INTEGER PRIMARY KEY DEFAULT 1,
                total_files BIGINT NOT NULL DEFAULT 0,
                total_bytes BIGINT NOT NULL DEFAULT 0,
                total_compressed BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(())
    }

    /// Insert new file record
    pub async fn insert_file(&self, file: &FileRecord) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO files 
            (file_id, filename, size_bytes, compressed_bytes, compression_ratio, 
             compression_method, storage_backend, uploaded_at, etag)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&file.file_id)
        .bind(&file.filename)
        .bind(file.size_bytes)
        .bind(file.compressed_bytes)
        .bind(file.compression_ratio)
        .bind(&file.compression_method)
        .bind(&file.storage_backend)
        .bind(file.uploaded_at)
        .bind(&file.etag)
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(())
    }

    /// Get file by ID
    pub async fn get_file(&self, file_id: &str) -> Result<FileRecord, DbError> {
        sqlx::query_as::<_, FileRecord>(
            "SELECT id, file_id, filename, size_bytes, compressed_bytes, compression_ratio, 
                    compression_method, storage_backend, uploaded_at, etag 
             FROM files WHERE file_id = $1",
        )
        .bind(file_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| DbError::NotFound(format!("File {} not found", file_id)))
    }

    /// List all files
    pub async fn list_files(&self, limit: i64, offset: i64) -> Result<Vec<FileRecord>, DbError> {
        sqlx::query_as::<_, FileRecord>(
            "SELECT id, file_id, filename, size_bytes, compressed_bytes, compression_ratio,
                    compression_method, storage_backend, uploaded_at, etag
             FROM files ORDER BY uploaded_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(DbError::QueryError)
    }

    /// Delete file record
    pub async fn delete_file(&self, file_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM files WHERE file_id = $1")
            .bind(file_id)
            .execute(&self.pool)
            .await
            .map_err(DbError::QueryError)?;

        Ok(())
    }

    /// Create upload session
    pub async fn create_session(
        &self,
        file_id: &str,
        total_chunks: i32,
    ) -> Result<UploadSession, DbError> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO upload_sessions (id, file_id, status, total_chunks, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(&session_id)
        .bind(file_id)
        .bind("pending")
        .bind(total_chunks)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(UploadSession {
            id: session_id,
            file_id: file_id.to_string(),
            status: "pending".to_string(),
            total_chunks,
            uploaded_chunks: 0,
            created_at: now,
            completed_at: None,
        })
    }

    /// Update session progress
    pub async fn update_session_progress(
        &self,
        session_id: &str,
        uploaded_chunks: i32,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE upload_sessions SET uploaded_chunks = $1 WHERE id = $2",
        )
        .bind(uploaded_chunks)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(())
    }

    /// Complete session
    pub async fn complete_session(&self, session_id: &str) -> Result<(), DbError> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE upload_sessions SET status = $1, completed_at = $2 WHERE id = $3",
        )
        .bind("completed")
        .bind(now)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(())
    }

    /// Get statistics
    pub async fn get_stats(&self) -> Result<Stats, DbError> {
        let result = sqlx::query(
            "SELECT COUNT(*) as total_files, SUM(size_bytes) as total_bytes, 
                    SUM(compressed_bytes) as total_compressed FROM files",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(Stats {
            total_files: result.get("total_files"),
            total_bytes: result.get::<Option<i64>, _>("total_bytes").unwrap_or(0),
            total_compressed: result
                .get::<Option<i64>, _>("total_compressed")
                .unwrap_or(0),
            updated_at: Utc::now(),
        })
    }

    /// Update statistics
    pub async fn update_stats(&self, size_bytes: i64, compressed_bytes: i64) -> Result<(), DbError> {
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO stats (total_files, total_bytes, total_compressed, updated_at)
            VALUES (1, $1, $2, $3)
            ON CONFLICT (id) DO UPDATE SET
                total_files = stats.total_files + 1,
                total_bytes = stats.total_bytes + $1,
                total_compressed = stats.total_compressed + $2,
                updated_at = $3
            "#,
        )
        .bind(size_bytes)
        .bind(compressed_bytes)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(DbError::QueryError)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_record_creation() {
        let record = FileRecord {
            id: Uuid::new_v4().to_string(),
            file_id: "test-id".to_string(),
            filename: "test.kore".to_string(),
            size_bytes: 1000,
            compressed_bytes: 500,
            compression_ratio: 0.5,
            compression_method: "hybrid".to_string(),
            storage_backend: "s3".to_string(),
            uploaded_at: Utc::now(),
            etag: Some("abc123".to_string()),
        };

        assert_eq!(record.file_id, "test-id");
        assert_eq!(record.compression_ratio, 0.5);
    }

    #[test]
    fn test_upload_session_creation() {
        let session = UploadSession {
            id: Uuid::new_v4().to_string(),
            file_id: "test-id".to_string(),
            status: "pending".to_string(),
            total_chunks: 10,
            uploaded_chunks: 0,
            created_at: Utc::now(),
            completed_at: None,
        };

        assert_eq!(session.status, "pending");
        assert_eq!(session.total_chunks, 10);
    }
}
