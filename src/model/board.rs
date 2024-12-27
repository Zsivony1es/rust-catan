use petgraph::graph::{NodeIndex, UnGraph};

struct Board {
    layout: UnGraph::<RoadNode, ()>
}

struct RoadNode {
    node_id: u32,
    node_type: RoadNodeType,
    neighbouring_fields: Vec<FieldType>
}



