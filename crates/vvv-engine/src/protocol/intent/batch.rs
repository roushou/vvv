use serde::{Deserialize, Serialize};

use super::Intent;

/// Several intents planned in sequence, each against the state the previous
/// one leaves, and applied as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchIntent {
    pub intents: Vec<Intent>,
}

impl BatchIntent {
    pub fn new(intents: impl IntoIterator<Item = Intent>) -> Self {
        Self {
            intents: intents.into_iter().collect(),
        }
    }
}
