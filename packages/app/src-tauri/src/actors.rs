use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub String);

impl ActorId {
    pub fn bridge() -> Self {
        Self("bridge".to_string())
    }

    pub fn system() -> Self {
        Self("system".to_string())
    }
}
