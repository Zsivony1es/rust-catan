use petgraph::graph::UnGraph;
use serde::{Deserialize, Serialize};

use super::enums::{FieldType, RoadNodeType};

#[allow(dead_code)]
pub struct Board {
    pub layout: UnGraph<RoadNode, ()>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadNode {
    pub node_id: u32,
    pub node_type: RoadNodeType,
    pub neighbouring_fields: Vec<FieldType>,
}

impl Board {
    pub fn new() -> Self {
        Self {
            layout: UnGraph::new_undirected(),
        }
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}
