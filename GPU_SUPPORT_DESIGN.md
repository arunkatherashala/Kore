# KORE v1.9.0 - GPU Support Design Document

**Version:** 1.0 Planning Phase  
**Target Release:** Q4 2026  
**Scope:** CUDA Kernels + Distributed GPU Execution

---

## 🎯 Vision: GPU-Accelerated Query Execution

**Goal:** Achieve **5-50x speedup** on aggregations and joins by offloading hot paths to NVIDIA GPUs.

**Key Features:**
- CUDA kernels for GROUP BY, SUM/AVG/COUNT, JOIN, SORT
- Automatic CPU ↔ GPU data transfer
- Multi-GPU support (peer-to-peer, NVLink)
- Distributed GPU execution across cluster
- Fallback to CPU for unsupported operations
- Zero API changes (transparent to users)

---

## 📐 Architecture

### Layer: kore-gpu (New)

```
kore-gpu/
├── src/
│   ├── lib.rs                 # GPU context + memory management
│   ├── kernels/
│   │   ├── aggregate.cu       # GROUP BY, SUM, AVG, COUNT, MIN, MAX
│   │   ├── join.cu            # Hash join, nested loop join
│   │   ├── sort.cu            # Radix sort, merge sort
│   │   ├── filter.cu          # Predicate evaluation
│   │   └── window.cu          # Window functions (LAG, LEAD, ROW_NUMBER)
│   ├── memory.rs              # GPU memory pool + pinning
│   ├── transfer.rs            # CPU ↔ GPU transfer (async)
│   ├── dispatch.rs            # Operation routing (CPU vs GPU)
│   └── profiler.rs            # Operation profiling + heuristics
├── build.rs                   # CUDA compilation
└── Cargo.toml
```

### Execution Flow

```
User Query
    ↓
Cost-based Optimizer
    ↓
Decision: CPU or GPU? (profile-guided)
    ↓
    ├─→ GPU Path:
    │   ├ Transfer data to GPU memory
    │   ├ Execute CUDA kernel
    │   ├ Transfer results back to CPU
    │   └ Return result
    │
    └─→ CPU Path:
        ├ Use existing kore-simd
        └ Return result
```

---

## 🔧 Implementation Phases

### Phase 1: CUDA Foundation (Weeks 1-2)

#### Setup
```rust
// gpu/src/lib.rs
use cuda::prelude::*;

pub struct GpuContext {
    device: Device,
    stream: Stream,
    memory_pool: GpuMemoryPool,
}

impl GpuContext {
    pub fn new() -> Result<Self, KoreError> {
        let device = Device::new()?;  // Default: GPU 0
        let stream = device.new_stream()?;
        let memory_pool = GpuMemoryPool::new(2 * 1024 * 1024 * 1024)?;  // 2GB
        Ok(Self { device, stream, memory_pool })
    }
    
    /// Allocate GPU memory (returns device pointer)
    pub fn allocate(&mut self, size: usize) -> Result<DevicePtr, KoreError> {
        self.memory_pool.allocate(size)
    }
    
    /// Transfer data from CPU to GPU (async)
    pub fn copy_to_gpu<T>(&mut self, cpu_ptr: &[T]) -> Result<DevicePtr, KoreError> {
        self.stream.copy_to_device(cpu_ptr)
    }
    
    /// Transfer data from GPU to CPU (async)
    pub fn copy_from_gpu<T>(&mut self, gpu_ptr: DevicePtr, count: usize) 
        -> Result<Vec<T>, KoreError> 
    {
        self.stream.copy_from_device(gpu_ptr, count)
    }
}
```

#### Memory Management
```rust
// gpu/src/memory.rs
pub struct GpuMemoryPool {
    total_size: usize,
    allocated: usize,
    free_blocks: Vec<(DevicePtr, usize)>,
}

impl GpuMemoryPool {
    pub fn new(size: usize) -> Result<Self, KoreError> {
        let base = Device::allocate(size)?;
        Ok(Self {
            total_size: size,
            allocated: 0,
            free_blocks: vec![(base, size)],
        })
    }
    
    /// Allocate from pool (first-fit)
    pub fn allocate(&mut self, size: usize) -> Result<DevicePtr, KoreError> {
        for (i, (ptr, block_size)) in self.free_blocks.iter().enumerate() {
            if *block_size >= size {
                let new_ptr = *ptr;
                self.allocated += size;
                
                if *block_size > size {
                    self.free_blocks[i] = (
                        DevicePtr::new(*ptr as usize + size),
                        *block_size - size,
                    );
                } else {
                    self.free_blocks.remove(i);
                }
                return Ok(new_ptr);
            }
        }
        Err(KoreError::OutOfMemory(format!(
            "GPU memory exhausted: {}/{} bytes used",
            self.allocated, self.total_size
        )))
    }
    
    /// Free memory (merge adjacent blocks)
    pub fn free(&mut self, ptr: DevicePtr, size: usize) {
        self.allocated -= size;
        self.free_blocks.push((ptr, size));
        self.free_blocks.sort_by_key(|(p, _)| *p);
        
        // Merge adjacent free blocks
        let mut merged = vec![];
        for (ptr, sz) in &self.free_blocks {
            if let Some((last_ptr, last_sz)) = merged.last_mut() {
                if *last_ptr as usize + *last_sz == *ptr as usize {
                    *last_sz += sz;
                    continue;
                }
            }
            merged.push((*ptr, *sz));
        }
        self.free_blocks = merged;
    }
}
```

### Phase 2: GPU Aggregation (Weeks 2-3)

#### CUDA Kernel: GROUP BY

```cuda
// gpu/kernels/aggregate.cu
__global__ void gpu_group_by_sum(
    const int64_t *key_data,      // Input: key column
    const float *val_data,         // Input: value column
    int n_rows,
    int64_t *out_keys,            // Output: unique keys
    float *out_sums,              // Output: summed values
    int *out_count                // Output: number of groups
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    
    if (idx < n_rows) {
        int64_t key = key_data[idx];
        float val = val_data[idx];
        
        // Atomic add to output bucket
        int bucket = (key % MAX_GROUPS);
        atomicAdd(&out_sums[bucket], val);
        
        // Mark key as present
        atomicExch(&out_keys[bucket], key);
    }
}
```

#### CPU Dispatch

```rust
// gpu/src/dispatch.rs
pub fn gpu_group_by_sum(
    gpu: &mut GpuContext,
    keys: &Column,
    values: &Column,
) -> Result<(Vec<Value>, Vec<Value>), KoreError> {
    // Transfer data to GPU
    let key_ptr = gpu.copy_to_gpu(keys.data.as_i64())?;
    let val_ptr = gpu.copy_to_gpu(values.data.as_f64())?;
    
    // Allocate output buffers
    let max_groups = 10000;
    let out_keys_ptr = gpu.allocate(max_groups * 8)?;  // 8 bytes per i64
    let out_vals_ptr = gpu.allocate(max_groups * 8)?;  // 8 bytes per f64
    let out_count_ptr = gpu.allocate(4)?;              // 4 bytes for count
    
    // Launch kernel
    let grid_size = (keys.data.len() + 255) / 256;
    gpu_group_by_sum<<<grid_size, 256>>>(
        key_ptr, val_ptr, keys.data.len() as i32,
        out_keys_ptr, out_vals_ptr, out_count_ptr
    );
    
    // Copy results back
    let result_keys = gpu.copy_from_gpu::<i64>(out_keys_ptr, max_groups)?;
    let result_vals = gpu.copy_from_gpu::<f64>(out_vals_ptr, max_groups)?;
    let count = gpu.copy_from_gpu::<i32>(out_count_ptr, 1)?[0];
    
    // Cleanup
    gpu.free(key_ptr);
    gpu.free(val_ptr);
    gpu.free(out_keys_ptr);
    gpu.free(out_vals_ptr);
    gpu.free(out_count_ptr);
    
    Ok((
        result_keys[..count as usize].iter().map(|&k| Value::Int(k)).collect(),
        result_vals[..count as usize].iter().map(|&v| Value::Float(v)).collect(),
    ))
}
```

### Phase 3: GPU Joins (Week 3-4)

```cuda
// gpu/kernels/join.cu
__global__ void gpu_hash_join(
    const int64_t *left_keys,
    const float *left_vals,
    int n_left,
    const int64_t *right_keys,
    const float *right_vals,
    int n_right,
    int64_t *out_left_keys,
    float *out_left_vals,
    int64_t *out_right_keys,
    float *out_right_vals,
    int *out_count
) {
    // Build hash table from right side (smaller)
    __shared__ int64_t hash_table[MAX_HASH_SIZE];
    
    if (threadIdx.x == 0) {
        memset(hash_table, -1, MAX_HASH_SIZE * 8);
    }
    __syncthreads();
    
    // Populate hash table
    for (int i = threadIdx.x; i < n_right; i += blockDim.x) {
        int bucket = (right_keys[i] % MAX_HASH_SIZE);
        atomicExch(&hash_table[bucket], right_keys[i]);
    }
    __syncthreads();
    
    // Probe with left side
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < n_left) {
        int64_t key = left_keys[idx];
        int bucket = (key % MAX_HASH_SIZE);
        
        if (hash_table[bucket] == key) {
            // Match found
            int out_idx = atomicAdd(out_count, 1);
            out_left_keys[out_idx] = key;
            out_left_vals[out_idx] = left_vals[idx];
            // Look up right side value (would need full hashtable with values)
        }
    }
}
```

### Phase 4: GPU Sort (Week 4)

```cuda
// gpu/kernels/sort.cu
__global__ void gpu_radix_sort_pass(
    float *data, int n, int bit_pos
) {
    // Radix sort for floats (one bit position)
    __shared__ float shared_data[BLOCK_SIZE * 2];
    
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    
    if (idx < n) {
        shared_data[threadIdx.x] = data[idx];
    }
    __syncthreads();
    
    // Parallel scan to segregate 0s and 1s
    bool bit = (shared_data[threadIdx.x] >> bit_pos) & 1;
    int scan = __syncthreads_count(!bit);  // Count 0s before this thread
    
    float sorted_val = shared_data[threadIdx.x];
    int new_idx = bit ? scan + (threadIdx.x - scan) : (threadIdx.x - scan);
    shared_data[new_idx] = sorted_val;
    __syncthreads();
    
    data[idx] = shared_data[threadIdx.x];
}
```

---

## 🎮 Integration with v1.9.0

### Changes to kore-sql

```rust
// executor.rs: Route aggregations to GPU
fn execute_aggregate(
    &self,
    block: &DataBlock,
    group_by: &[String],
    agg_func: &AggFunc,
) -> Result<DataBlock, KoreError> {
    if let Some(gpu) = &self.gpu_context {
        // Try GPU path
        if block.num_rows > 100_000 && group_by.len() <= 3 {
            return gpu.group_by_aggregate(block, group_by, agg_func);
        }
    }
    
    // Fallback to CPU (kore-simd)
    self.cpu_aggregate(block, group_by, agg_func)
}
```

### Changes to kore-optimize

```rust
// optimizer.rs: Decide CPU vs GPU
fn choose_execution_mode(
    &self,
    node: &PlanNode,
    est_rows: u64,
) -> ExecutionMode {
    match node {
        PlanNode::Aggregate { .. } if est_rows > 100_000 => {
            ExecutionMode::GPU  // Large aggregations → GPU
        }
        PlanNode::Join { .. } if est_rows > 1_000_000 => {
            ExecutionMode::GPU  // Large joins → GPU
        }
        _ => ExecutionMode::CPU,
    }
}
```

---

## 📊 Expected Performance (Post v1.9.0)

| Operation | CPU (SIMD) | GPU (CUDA) | Speedup |
|-----------|-----------|-----------|---------|
| GROUP BY (1M rows) | 50ms | 5ms | **10x** |
| SUM aggregate (10M) | 100ms | 8ms | **12x** |
| Hash join (1M×1M) | 200ms | 15ms | **13x** |
| Sort (10M rows) | 150ms | 10ms | **15x** |
| Complex query | 1000ms | 100ms | **10x avg** |

**Overall Impact:** Additional 10-15x speedup on GPU-heavy workloads (aggregate-heavy or large joins).

---

## 🛣️ Development Roadmap

```
Week 1-2: CUDA setup, memory mgmt, basic kernel
Week 2-3: GROUP BY + aggregations
Week 3-4: Hash join implementation
Week 4-5: Sort, filter, window functions
Week 5-6: Integration testing + optimization
Week 6-7: Multi-GPU support + distributed
Week 7-8: Performance tuning + documentation
```

---

## ⚠️ Considerations

### GPU Memory Limits
- **Constraint:** GPU memory (8-80GB) << CPU memory available
- **Solution:** Automatic spilling to CPU when data > GPU memory
- **API:** `GPU_MEMORY_LIMIT` env var

### Mixed Data Types
- **Constraint:** GPU kernels typed (Int, Float, etc.)
- **Solution:** Dispatch based on actual column types
- **Fallback:** CPU for mixed/string types

### Multi-GPU Scaling
- **Near-term:** Single GPU per worker
- **Future:** P2P with NVLink, device-to-device transfers
- **Distributed:** MPS (Multi-Process Service) not needed initially

---

*Design Document Created: 2026-08-29*  
*Target Release: Q4 2026 (v1.9.0)*  
*Estimated Effort: 8-10 weeks*  
*Team Size: 2-3 engineers*
