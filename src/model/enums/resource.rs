use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Wood,
    Brick,
    Wool,
    Wheat,
    Ore,
}

impl Resource {
    pub const ALL: [Resource; 5] = [
        Resource::Wood,
        Resource::Brick,
        Resource::Wool,
        Resource::Wheat,
        Resource::Ore,
    ];
}
