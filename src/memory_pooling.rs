/// Memory pooling for efficient allocation and reuse
///
/// Provides buffer pools and object pools to reduce allocation overhead
/// in inner loops and large result processing.

use std::sync::Arc;
use std::sync::Mutex;

/// Configuration for memory pooling
#[derive(Clone)]
pub struct PoolConfig {
    pub buffer_pool_size: usize,
    pub buffer_size: usize,
    pub row_pool_size: usize,
    pub enable_reuse: bool,
}

impl PoolConfig {
    pub fn new() -> Self {
        Self {
            buffer_pool_size: 100,
            buffer_size: 8192,
            row_pool_size: 10000,
            enable_reuse: true,
        }
    }

    pub fn with_buffer_count(mut self, count: usize) -> Self {
        self.buffer_pool_size = count;
        self
    }

    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    pub fn with_row_pool_size(mut self, size: usize) -> Self {
        self.row_pool_size = size;
        self
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A reusable buffer in the pool
#[derive(Clone, Debug, PartialEq)]
pub struct PooledBuffer {
    pub buffer_id: usize,
    pub data: Vec<u8>,
    pub used_bytes: usize,
}

impl PooledBuffer {
    pub fn new(id: usize, capacity: usize) -> Self {
        Self {
            buffer_id: id,
            data: vec![0u8; capacity],
            used_bytes: 0,
        }
    }

    pub fn reset(&mut self) {
        self.used_bytes = 0;
    }

    pub fn available(&self) -> usize {
        self.data.len() - self.used_bytes
    }

    pub fn write(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > self.available() {
            return false;
        }
        self.data[self.used_bytes..self.used_bytes + bytes.len()]
            .copy_from_slice(bytes);
        self.used_bytes += bytes.len();
        true
    }

    pub fn utilization_percent(&self) -> f64 {
        (self.used_bytes as f64) / (self.data.len() as f64) * 100.0
    }
}

/// Buffer pool for managing reusable buffers
pub struct BufferPool {
    buffers: Arc<Mutex<Vec<PooledBuffer>>>,
    available_buffers: Arc<Mutex<Vec<usize>>>,
    config: PoolConfig,
}

impl BufferPool {
    pub fn new(config: PoolConfig) -> Self {
        let mut buffers = Vec::new();
        let mut available = Vec::new();

        for i in 0..config.buffer_pool_size {
            buffers.push(PooledBuffer::new(i, config.buffer_size));
            available.push(i);
        }

        Self {
            buffers: Arc::new(Mutex::new(buffers)),
            available_buffers: Arc::new(Mutex::new(available)),
            config,
        }
    }

    /// Acquire a buffer from the pool
    /// Returns Some(buffer_id) if available, None if pool exhausted
    pub fn acquire(&self) -> Option<usize> {
        if !self.config.enable_reuse {
            return None;
        }

        let mut available = self.available_buffers.lock().ok()?;
        available.pop()
    }

    /// Release a buffer back to the pool
    pub fn release(&self, buffer_id: usize) {
        if !self.config.enable_reuse {
            return;
        }

        if let Ok(mut buffers) = self.buffers.lock() {
            if buffer_id < buffers.len() {
                buffers[buffer_id].reset();
            }
        }

        if let Ok(mut available) = self.available_buffers.lock() {
            available.push(buffer_id);
        }
    }

    /// Get current pool utilization
    pub fn utilization(&self) -> PoolUtilization {
        let available = self
            .available_buffers
            .lock()
            .map(|v| v.len())
            .unwrap_or(0);

        let buffers_in_use = self.config.buffer_pool_size - available;
        let utilization_percent =
            (buffers_in_use as f64) / (self.config.buffer_pool_size as f64)
                * 100.0;

        PoolUtilization {
            total_buffers: self.config.buffer_pool_size,
            buffers_in_use,
            available_buffers: available,
            utilization_percent,
        }
    }

    /// Get buffer capacity
    pub fn buffer_capacity(&self) -> usize {
        self.config.buffer_size
    }

    /// Total memory managed by pool
    pub fn total_memory_bytes(&self) -> usize {
        self.config.buffer_pool_size * self.config.buffer_size
    }
}

/// Row object for pooling
#[derive(Clone, Debug, PartialEq)]
pub struct PooledRow {
    pub row_id: usize,
    pub columns: Vec<String>,
    pub values: Vec<String>,
}

impl PooledRow {
    pub fn new(id: usize, num_columns: usize) -> Self {
        Self {
            row_id: id,
            columns: Vec::with_capacity(num_columns),
            values: Vec::with_capacity(num_columns),
        }
    }

    pub fn reset(&mut self) {
        self.columns.clear();
        self.values.clear();
    }

    pub fn set_value(&mut self, col: String, val: String) {
        self.columns.push(col);
        self.values.push(val);
    }
}

/// Row pool for managing reusable row objects
pub struct RowPool {
    rows: Arc<Mutex<Vec<PooledRow>>>,
    available_rows: Arc<Mutex<Vec<usize>>>,
    config: PoolConfig,
}

impl RowPool {
    pub fn new(config: PoolConfig) -> Self {
        let mut rows = Vec::new();
        let mut available = Vec::new();

        for i in 0..config.row_pool_size {
            rows.push(PooledRow::new(i, 16));
            available.push(i);
        }

        Self {
            rows: Arc::new(Mutex::new(rows)),
            available_rows: Arc::new(Mutex::new(available)),
            config,
        }
    }

    /// Acquire a row from the pool
    pub fn acquire(&self) -> Option<usize> {
        if !self.config.enable_reuse {
            return None;
        }

        let mut available = self.available_rows.lock().ok()?;
        available.pop()
    }

    /// Release a row back to the pool
    pub fn release(&self, row_id: usize) {
        if !self.config.enable_reuse {
            return;
        }

        if let Ok(mut rows) = self.rows.lock() {
            if row_id < rows.len() {
                rows[row_id].reset();
            }
        }

        if let Ok(mut available) = self.available_rows.lock() {
            available.push(row_id);
        }
    }

    /// Get current pool utilization
    pub fn utilization(&self) -> PoolUtilization {
        let available = self
            .available_rows
            .lock()
            .map(|v| v.len())
            .unwrap_or(0);

        let rows_in_use = self.config.row_pool_size - available;
        let utilization_percent =
            (rows_in_use as f64) / (self.config.row_pool_size as f64)
                * 100.0;

        PoolUtilization {
            total_buffers: self.config.row_pool_size,
            buffers_in_use: rows_in_use,
            available_buffers: available,
            utilization_percent,
        }
    }
}

/// Pool utilization statistics
#[derive(Clone, Debug, PartialEq)]
pub struct PoolUtilization {
    pub total_buffers: usize,
    pub buffers_in_use: usize,
    pub available_buffers: usize,
    pub utilization_percent: f64,
}

/// Memory pool manager coordinating all pools
pub struct MemoryPoolManager {
    buffer_pool: Arc<BufferPool>,
    row_pool: Arc<RowPool>,
    config: PoolConfig,
}

impl MemoryPoolManager {
    pub fn new(config: PoolConfig) -> Self {
        let buffer_pool = Arc::new(BufferPool::new(config.clone()));
        let row_pool = Arc::new(RowPool::new(config.clone()));

        Self {
            buffer_pool,
            row_pool,
            config,
        }
    }

    pub fn get_buffer_pool(&self) -> Arc<BufferPool> {
        Arc::clone(&self.buffer_pool)
    }

    pub fn get_row_pool(&self) -> Arc<RowPool> {
        Arc::clone(&self.row_pool)
    }

    /// Get overall memory utilization
    pub fn overall_utilization(&self) -> OverallUtilization {
        let buffer_util = self.buffer_pool.utilization();
        let row_util = self.row_pool.utilization();

        let total_memory = self.buffer_pool.total_memory_bytes();
        let buffer_memory_in_use = buffer_util.buffers_in_use
            * self.buffer_pool.buffer_capacity();

        OverallUtilization {
            total_managed_memory_bytes: total_memory,
            buffer_utilization: buffer_util,
            row_utilization: row_util,
            memory_in_use_bytes: buffer_memory_in_use,
        }
    }

    /// Estimate memory savings from pooling
    pub fn estimate_allocation_savings(&self, allocations_avoided: usize) -> u64 {
        (allocations_avoided as u64) * 100
    }
}

/// Overall memory pool utilization
#[derive(Clone, Debug, PartialEq)]
pub struct OverallUtilization {
    pub total_managed_memory_bytes: usize,
    pub buffer_utilization: PoolUtilization,
    pub row_utilization: PoolUtilization,
    pub memory_in_use_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.buffer_pool_size, 100);
        assert_eq!(config.buffer_size, 8192);
        assert_eq!(config.row_pool_size, 10000);
        assert!(config.enable_reuse);
    }

    #[test]
    fn test_pool_config_custom() {
        let config = PoolConfig::new()
            .with_buffer_count(50)
            .with_buffer_size(4096)
            .with_row_pool_size(5000);

        assert_eq!(config.buffer_pool_size, 50);
        assert_eq!(config.buffer_size, 4096);
        assert_eq!(config.row_pool_size, 5000);
    }

    #[test]
    fn test_pooled_buffer_creation() {
        let buffer = PooledBuffer::new(0, 8192);
        assert_eq!(buffer.buffer_id, 0);
        assert_eq!(buffer.used_bytes, 0);
        assert_eq!(buffer.available(), 8192);
    }

    #[test]
    fn test_pooled_buffer_write() {
        let mut buffer = PooledBuffer::new(0, 8192);
        let data = vec![1u8, 2, 3, 4, 5];

        assert!(buffer.write(&data));
        assert_eq!(buffer.used_bytes, 5);
        assert_eq!(buffer.available(), 8187);
    }

    #[test]
    fn test_pooled_buffer_reset() {
        let mut buffer = PooledBuffer::new(0, 8192);
        buffer.write(&vec![1u8; 100]);
        assert_eq!(buffer.used_bytes, 100);

        buffer.reset();
        assert_eq!(buffer.used_bytes, 0);
        assert_eq!(buffer.available(), 8192);
    }

    #[test]
    fn test_buffer_pool_acquire_release() {
        let config = PoolConfig::new().with_buffer_count(5);
        let pool = BufferPool::new(config);

        let id1 = pool.acquire();
        assert!(id1.is_some());

        let id2 = pool.acquire();
        assert!(id2.is_some());

        let util = pool.utilization();
        assert_eq!(util.buffers_in_use, 2);

        if let Some(id) = id1 {
            pool.release(id);
        }
        let util = pool.utilization();
        assert_eq!(util.buffers_in_use, 1);
    }

    #[test]
    fn test_buffer_pool_exhaustion() {
        let config = PoolConfig::new().with_buffer_count(2);
        let pool = BufferPool::new(config);

        pool.acquire();
        pool.acquire();
        let id3 = pool.acquire();

        assert!(id3.is_none());
    }

    #[test]
    fn test_pooled_row_creation() {
        let row = PooledRow::new(0, 4);
        assert_eq!(row.row_id, 0);
        assert_eq!(row.columns.capacity(), 4);
        assert_eq!(row.values.capacity(), 4);
    }

    #[test]
    fn test_pooled_row_set_value() {
        let mut row = PooledRow::new(0, 2);
        row.set_value("name".to_string(), "Alice".to_string());
        row.set_value("age".to_string(), "30".to_string());

        assert_eq!(row.columns.len(), 2);
        assert_eq!(row.values.len(), 2);
    }

    #[test]
    fn test_row_pool_acquire_release() {
        let config = PoolConfig::new().with_row_pool_size(5);
        let pool = RowPool::new(config);

        let id1 = pool.acquire();
        assert!(id1.is_some());

        let id2 = pool.acquire();
        assert!(id2.is_some());

        let util = pool.utilization();
        assert_eq!(util.buffers_in_use, 2);

        if let Some(id) = id1 {
            pool.release(id);
        }

        let util = pool.utilization();
        assert_eq!(util.buffers_in_use, 1);
    }

    #[test]
    fn test_memory_pool_manager() {
        let config = PoolConfig::new()
            .with_buffer_count(10)
            .with_row_pool_size(100);
        let manager = MemoryPoolManager::new(config);

        let buffer_pool = manager.get_buffer_pool();
        let row_pool = manager.get_row_pool();

        let buf_id = buffer_pool.acquire();
        assert!(buf_id.is_some());

        let row_id = row_pool.acquire();
        assert!(row_id.is_some());
    }

    #[test]
    fn test_overall_utilization() {
        let config = PoolConfig::new()
            .with_buffer_count(10)
            .with_row_pool_size(100);
        let manager = MemoryPoolManager::new(config);

        let util = manager.overall_utilization();
        assert!(util.total_managed_memory_bytes > 0);
    }

    #[test]
    fn test_estimate_allocation_savings() {
        let config = PoolConfig::default();
        let manager = MemoryPoolManager::new(config);

        let savings = manager.estimate_allocation_savings(1000);
        assert_eq!(savings, 100000);
    }
}
