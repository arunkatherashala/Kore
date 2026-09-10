//! In-memory table store on each worker (Phase 3 — local data).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kore_core::DataBlock;

#[derive(Clone, Default)]
pub struct TableStore {
    inner: Arc<Mutex<HashMap<String, DataBlock>>>,
}

impl TableStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, name: &str, data: DataBlock) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), data);
    }

    pub fn get(&self, name: &str) -> Option<DataBlock> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }

    /// Snapshot every registered `(name, block)` pair — used by task runners
    /// that need multi-table access (e.g. broadcast join: fact + dim on the
    /// same worker).
    pub fn snapshot_all(&self) -> Vec<(String, DataBlock)> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn table_count(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData};

    #[test]
    fn register_and_get() {
        let store = TableStore::new();
        let block = DataBlock {
            num_rows: 1,
            columns: vec![Column {
                name: "x".into(),
                data: ColumnData::Int64(vec![Some(1)]),
            }],
        };
        store.register("t", block.clone());
        assert_eq!(store.get("t").unwrap().num_rows, 1);
    }
}
