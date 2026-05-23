-- Kore Cloud API Database Initialization
-- This script runs automatically when the PostgreSQL container starts

-- Create extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Files table
CREATE TABLE IF NOT EXISTS files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id VARCHAR(36) NOT NULL UNIQUE,
    filename VARCHAR(512) NOT NULL,
    size_bytes BIGINT NOT NULL,
    compressed_bytes BIGINT NOT NULL,
    compression_ratio DOUBLE PRECISION NOT NULL,
    compression_method VARCHAR(50) NOT NULL,
    storage_backend VARCHAR(50) NOT NULL,
    uploaded_at TIMESTAMP WITH TIME ZONE NOT NULL,
    etag VARCHAR(100),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Indexes on files
CREATE INDEX idx_files_file_id ON files(file_id);
CREATE INDEX idx_files_backend ON files(storage_backend);
CREATE INDEX idx_files_date ON files(uploaded_at DESC);
CREATE INDEX idx_files_method ON files(compression_method);

-- Upload sessions table
CREATE TABLE IF NOT EXISTS upload_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id VARCHAR(36) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_chunks INTEGER NOT NULL,
    uploaded_chunks INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMP WITH TIME ZONE
);

-- Indexes on upload_sessions
CREATE INDEX idx_upload_sessions_file_id ON upload_sessions(file_id);
CREATE INDEX idx_upload_sessions_status ON upload_sessions(status);

-- Statistics table
CREATE TABLE IF NOT EXISTS stats (
    id INTEGER PRIMARY KEY DEFAULT 1,
    total_files BIGINT NOT NULL DEFAULT 0,
    total_bytes BIGINT NOT NULL DEFAULT 0,
    total_compressed BIGINT NOT NULL DEFAULT 0,
    avg_compression_ratio DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Insert initial stats row
INSERT INTO stats (id, total_files, total_bytes, total_compressed, avg_compression_ratio)
VALUES (1, 0, 0, 0, 0)
ON CONFLICT (id) DO NOTHING;

-- Create audit log table (optional)
CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action VARCHAR(50) NOT NULL,
    file_id VARCHAR(36),
    details JSONB,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Index on audit log
CREATE INDEX idx_audit_log_created ON audit_log(created_at DESC);
CREATE INDEX idx_audit_log_action ON audit_log(action);

-- Grant permissions to kore user
GRANT USAGE ON SCHEMA public TO kore;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO kore;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO kore;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA public TO kore;

-- Views for analytics (optional)
CREATE VIEW file_statistics AS
SELECT
    COUNT(*) as total_files,
    SUM(size_bytes) as total_bytes_original,
    SUM(compressed_bytes) as total_bytes_compressed,
    AVG(compression_ratio) as avg_compression_ratio,
    MAX(uploaded_at) as latest_upload,
    compression_method,
    storage_backend
FROM files
GROUP BY compression_method, storage_backend;

GRANT SELECT ON file_statistics TO kore;
