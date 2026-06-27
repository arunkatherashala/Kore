-- Initialize PostgreSQL schema for Kore Cloud MVP

-- Kore Files Table
CREATE TABLE kore_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket VARCHAR(255) NOT NULL,
    key VARCHAR(1024) NOT NULL,
    size BIGINT NOT NULL,
    compressed_ratio DECIMAL(5, 2),
    row_count BIGINT,
    column_count INT,
    codec_stats JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by VARCHAR(255),
    metadata JSONB,
    CONSTRAINT unique_s3_file UNIQUE (bucket, key)
);

-- Query Cache Table
CREATE TABLE query_cache (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id UUID REFERENCES kore_files(id) ON DELETE CASCADE,
    query_hash VARCHAR(64) NOT NULL,
    result BYTEA,
    result_size BIGINT,
    ttl INT DEFAULT 3600,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    hit_count INT DEFAULT 0,
    CONSTRAINT unique_query_cache UNIQUE (file_id, query_hash)
);

-- Query Logs Table (for analytics)
CREATE TABLE query_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id UUID REFERENCES kore_files(id) ON DELETE CASCADE,
    query_text TEXT,
    execution_time_ms INT,
    result_rows BIGINT,
    bytes_scanned BIGINT,
    bytes_processed BIGINT,
    status VARCHAR(50),
    error_message TEXT,
    user_id VARCHAR(255),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Performance Metrics Table
CREATE TABLE performance_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    metric_type VARCHAR(50) NOT NULL,
    value DECIMAL(15, 2),
    unit VARCHAR(50),
    tags JSONB,
    recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- API Keys Table
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id VARCHAR(255) NOT NULL,
    key_hash VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255),
    last_used_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    revoked BOOLEAN DEFAULT FALSE
);

-- Create indexes for performance
CREATE INDEX idx_kore_files_bucket ON kore_files(bucket);
CREATE INDEX idx_kore_files_created_at ON kore_files(created_at);
CREATE INDEX idx_query_cache_expires_at ON query_cache(expires_at);
CREATE INDEX idx_query_logs_file_id ON query_logs(file_id);
CREATE INDEX idx_query_logs_created_at ON query_logs(created_at);
CREATE INDEX idx_api_keys_user_id ON api_keys(user_id);

-- Create function to update 'updated_at' automatically
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply trigger to kore_files
CREATE TRIGGER update_kore_files_updated_at BEFORE UPDATE
    ON kore_files FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Create function to auto-expire query cache
CREATE OR REPLACE FUNCTION expire_query_cache()
RETURNS void AS $$
BEGIN
    DELETE FROM query_cache WHERE expires_at < CURRENT_TIMESTAMP;
END;
$$ LANGUAGE plpgsql;
