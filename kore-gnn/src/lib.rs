//! KORE GNN — Phase 6B: Graph Neural Networks
pub struct GraphConvLayer {
    hidden_size: usize,
}

impl GraphConvLayer {
    pub fn new(hidden_size: usize) -> Self {
        GraphConvLayer { hidden_size }
    }

    pub fn forward(&self, _features: &[f32], _adjacency: &[Vec<f32>]) -> Vec<f32> {
        vec![0.0; self.hidden_size]
    }
}
