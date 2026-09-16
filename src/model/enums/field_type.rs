use serde::{Deserialize, Serialize};

use super::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    Forest,
    Hills,
    Pasture,
    Fields,
    Mountains,
    Desert,
}

impl FieldType {
    pub fn produces(self) -> Option<Resource> {
        match self {
            FieldType::Forest => Some(Resource::Wood),
            FieldType::Hills => Some(Resource::Brick),
            FieldType::Pasture => Some(Resource::Wool),
            FieldType::Fields => Some(Resource::Wheat),
            FieldType::Mountains => Some(Resource::Ore),
            FieldType::Desert => None,
        }
    }
}
