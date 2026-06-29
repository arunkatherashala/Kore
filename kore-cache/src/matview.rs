//! Materialised Views — named, pre-computed DataBlocks with staleness tracking.
//!
//! A `MatView` stores the last-computed result together with the epoch at which
//! it was refreshed.  `MatViewRegistry` manages a collection of named views and
//! supports manual and conditional (stale-after-N-ticks) refresh.

use std::collections::HashMap;
use kore_core::{DataBlock, KoreError};

/// A single materialised view.
#[derive(Debug, Clone)]
pub struct MatView {
    pub name:            String,
    pub data:            DataBlock,
    pub refresh_epoch:   u64,
    pub stale_after:     Option<u64>,   // None = never auto-stale
    pub query_signature: String,        // Logical query that produced this view
}

impl MatView {
    pub fn new(
        name: &str,
        data: DataBlock,
        epoch: u64,
        stale_after: Option<u64>,
        query_signature: &str,
    ) -> Self {
        Self {
            name: name.into(),
            data,
            refresh_epoch: epoch,
            stale_after,
            query_signature: query_signature.into(),
        }
    }

    pub fn is_stale(&self, current_epoch: u64) -> bool {
        match self.stale_after {
            None    => false,
            Some(s) => current_epoch.saturating_sub(self.refresh_epoch) >= s,
        }
    }
}

/// Registry of named materialised views.
pub struct MatViewRegistry {
    views: HashMap<String, MatView>,
    epoch: u64,
}

impl MatViewRegistry {
    pub fn new() -> Self {
        Self { views: HashMap::new(), epoch: 0 }
    }

    /// Advance the epoch (call this on each table write to track staleness).
    pub fn tick(&mut self) { self.epoch += 1; }

    pub fn current_epoch(&self) -> u64 { self.epoch }

    /// Register or overwrite a materialised view.
    pub fn create_or_replace(
        &mut self,
        name: &str,
        data: DataBlock,
        stale_after: Option<u64>,
        query_sig: &str,
    ) {
        let view = MatView::new(name, data, self.epoch, stale_after, query_sig);
        self.views.insert(name.into(), view);
    }

    /// Read a view; returns `None` if absent or stale.
    pub fn get(&self, name: &str) -> Option<&MatView> {
        let v = self.views.get(name)?;
        if v.is_stale(self.epoch) { None } else { Some(v) }
    }

    /// Read a view's data, ignoring staleness.
    pub fn get_unchecked(&self, name: &str) -> Option<&MatView> {
        self.views.get(name)
    }

    /// Forcefully refresh a view with new data.
    pub fn refresh(&mut self, name: &str, data: DataBlock) -> Result<(), KoreError> {
        let view = self.views.get_mut(name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("view '{}' not found", name)))?;
        view.data          = data;
        view.refresh_epoch = self.epoch;
        Ok(())
    }

    /// Drop a view.
    pub fn drop_view(&mut self, name: &str) -> bool {
        self.views.remove(name).is_some()
    }

    /// List all view names.
    pub fn list(&self) -> Vec<&str> {
        self.views.keys().map(|s| s.as_str()).collect()
    }

    /// Returns views that are currently stale.
    pub fn stale_views(&self) -> Vec<&str> {
        self.views.iter()
            .filter(|(_, v)| v.is_stale(self.epoch))
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

impl Default for MatViewRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};

    fn dummy_block(rows: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::int64("id", (0..rows as i64).map(Some).collect()),
        ]).unwrap()
    }

    #[test]
    fn create_and_get() {
        let mut reg = MatViewRegistry::new();
        reg.create_or_replace("monthly_sales", dummy_block(500), None, "SELECT ...");
        let v = reg.get("monthly_sales").unwrap();
        assert_eq!(v.data.num_rows, 500);
    }

    #[test]
    fn staleness() {
        let mut reg = MatViewRegistry::new();
        reg.create_or_replace("hourly", dummy_block(10), Some(3), "SELECT ...");
        reg.tick(); reg.tick(); reg.tick();
        assert!(reg.get("hourly").is_none());
    }

    #[test]
    fn refresh_view() {
        let mut reg = MatViewRegistry::new();
        reg.create_or_replace("v", dummy_block(5), Some(1), "q");
        reg.tick(); reg.tick(); // now stale
        assert!(reg.get("v").is_none());
        reg.refresh("v", dummy_block(20)).unwrap();
        assert_eq!(reg.get("v").unwrap().data.num_rows, 20);
    }
}
