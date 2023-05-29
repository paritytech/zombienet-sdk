use std::marker::PhantomData;

use crate::shared::{macros::states, types::ParaId};

#[derive(Debug, Clone, PartialEq)]
pub struct HrmpChannelConfig {
    sender: ParaId,
    recipient: ParaId,
    max_capacity: u32,
    max_message_size: u32,
}

impl HrmpChannelConfig {
    pub fn sender(&self) -> ParaId {
        self.sender
    }

    pub fn recipient(&self) -> ParaId {
        self.recipient
    }

    pub fn max_capacity(&self) -> u32 {
        self.max_capacity
    }

    pub fn max_message_size(&self) -> u32 {
        self.max_message_size
    }
}

states! {
    Initial,
    WithSender,
    WithRecipient
}

#[derive(Debug)]
pub struct HrmpChannelConfigBuilder<State> {
    config: HrmpChannelConfig,
    _state: PhantomData<State>,
}

impl Default for HrmpChannelConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: HrmpChannelConfig {
                sender: 0,
                recipient: 0,
                max_capacity: 8,
                max_message_size: 512,
            },
            _state: PhantomData,
        }
    }
}

impl<A> HrmpChannelConfigBuilder<A> {
    fn transition<B>(&self, config: HrmpChannelConfig) -> HrmpChannelConfigBuilder<B> {
        HrmpChannelConfigBuilder {
            config,
            _state: PhantomData,
        }
    }
}

impl HrmpChannelConfigBuilder<Initial> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sender(self, sender: ParaId) -> HrmpChannelConfigBuilder<WithSender> {
        self.transition(HrmpChannelConfig {
            sender,
            ..self.config
        })
    }
}

impl HrmpChannelConfigBuilder<WithSender> {
    pub fn with_recipient(self, recipient: ParaId) -> HrmpChannelConfigBuilder<WithRecipient> {
        self.transition(HrmpChannelConfig {
            recipient,
            ..self.config
        })
    }
}

impl HrmpChannelConfigBuilder<WithRecipient> {
    pub fn with_max_capacity(self, max_capacity: u32) -> Self {
        self.transition(HrmpChannelConfig {
            max_capacity,
            ..self.config
        })
    }

    pub fn with_max_message_size(self, max_message_size: u32) -> Self {
        self.transition(HrmpChannelConfig {
            max_message_size,
            ..self.config
        })
    }

    pub fn build(self) -> HrmpChannelConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrmp_channel_config_builder_should_build_a_new_hrmp_channel_config_correctly() {
        let hrmp_channel_config = HrmpChannelConfigBuilder::new()
            .with_sender(1000)
            .with_recipient(2000)
            .with_max_capacity(50)
            .with_max_message_size(100)
            .build();

        assert_eq!(hrmp_channel_config.sender(), 1000);
        assert_eq!(hrmp_channel_config.recipient(), 2000);
        assert_eq!(hrmp_channel_config.max_capacity(), 50);
        assert_eq!(hrmp_channel_config.max_message_size(), 100);
    }
}
