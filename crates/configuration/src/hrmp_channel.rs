use serde::Serialize;

use crate::shared::types::ParaId;

#[derive(Debug, Clone, Serialize)]
pub struct HrmpChannelConfig {
    sender:           ParaId,
    recipient:        ParaId,
    max_capacity:     u32,
    max_message_size: u32,
}

impl Default for HrmpChannelConfig {
    fn default() -> Self {
        todo!()
    }
}

impl HrmpChannelConfig {
    pub fn with_sender(self, sender: ParaId) -> Self {
        Self { sender, ..self }
    }

    pub fn with_recipient(self, recipient: ParaId) -> Self {
        Self { recipient, ..self }
    }

    pub fn with_max_capacity(self, max_capacity: u32) -> Self {
        Self {
            max_capacity,
            ..self
        }
    }

    pub fn with_max_message_size(self, max_message_size: u32) -> Self {
        Self {
            max_message_size,
            ..self
        }
    }
}
