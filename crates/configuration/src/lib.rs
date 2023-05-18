#[derive(Debug, Default)]
struct Network {
    relaychain: Relaychain,
    parachains: Vec<Parachain>,
}

impl Network {
    fn new() -> Self {
        Self::default()
    }

    fn with_relaychain(self, f: fn(Relaychain) -> Relaychain) -> Self {
        Self {
            relaychain: f(Relaychain::default()),
            ..self
        }
    }

    fn with_parachain(self, f: fn(Parachain) -> Parachain) -> Self {
        let mut parachains = self.parachains;
        parachains.push(f(Parachain::default()));

        Self { parachains, ..self }
    }
}

#[derive(Debug, Default)]
struct Relaychain {
    one: String,
    two: String,
    three: String,
    validators: Vec<Validator>,
}

impl Relaychain {
    fn with_option_one(self, one: &str) -> Self {
        Self {
            one: one.into(),
            ..self
        }
    }

    fn with_option_two(self, two: &str) -> Self {
        Self {
            two: two.into(),
            ..self
        }
    }

    fn with_option_three(self, three: &str) -> Self {
        Self {
            three: three.into(),
            ..self
        }
    }

    fn with_validator(self, f: fn(Validator) -> Validator) -> Self {
        let mut validators = self.validators;
        validators.push(f(Validator::default()));

        Self { validators, ..self }
    }
}

#[derive(Debug, Default)]
struct Validator {
    one: String,
    two: String,
    three: String,
}

impl Validator {
    fn with_option_one(self, one: &str) -> Self {
        Self {
            one: one.into(),
            ..self
        }
    }

    fn with_option_two(self, two: &str) -> Self {
        Self {
            two: two.into(),
            ..self
        }
    }

    fn with_option_three(self, three: &str) -> Self {
        Self {
            three: three.into(),
            ..self
        }
    }
}

#[derive(Debug, Default)]
struct Parachain {
    one: String,
    two: String,
    three: String,
    collators: Vec<Collator>,
}

impl Parachain {
    fn with_option_one(self, one: &str) -> Self {
        Self {
            one: one.into(),
            ..self
        }
    }

    fn with_option_two(self, two: &str) -> Self {
        Self {
            two: two.into(),
            ..self
        }
    }

    fn with_option_three(self, three: &str) -> Self {
        Self {
            three: three.into(),
            ..self
        }
    }

    fn with_collator(self, f: fn(Collator) -> Collator) -> Self {
        let mut collators = self.collators;
        collators.push(f(Collator::default()));

        Self { collators, ..self }
    }
}

#[derive(Debug, Default)]
struct Collator {
    one: String,
    two: String,
    three: String,
}

impl Collator {
    fn with_option_one(self, one: &str) -> Self {
        Self {
            one: one.into(),
            ..self
        }
    }

    fn with_option_two(self, two: &str) -> Self {
        Self {
            two: two.into(),
            ..self
        }
    }

    fn with_option_three(self, three: &str) -> Self {
        Self {
            three: three.into(),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        Network::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_option_one("foo")
                    .with_option_two("bar")
                    .with_option_three("baz")
                    .with_validator(|validator| {
                        validator
                            .with_option_one("foo")
                            .with_option_two("bar")
                            .with_option_three("baz")
                    })
                    .with_validator(|validator| {
                        validator
                            .with_option_one("foo")
                            .with_option_two("bar")
                            .with_option_three("baz")
                    })
            })
            .with_parachain(|builder| {
                builder
                    .with_option_one("foo")
                    .with_option_two("bar")
                    .with_option_three("baz")
                    .with_collator(|collator| {
                        collator
                            .with_option_one("foo")
                            .with_option_two("bar")
                            .with_option_three("baz")
                    })
            })
            .with_parachain(|builder| {
                builder
                    .with_option_one("foo")
                    .with_option_two("bar")
                    .with_option_three("baz")
                    .with_collator(|collator| {
                        collator
                            .with_option_one("foo")
                            .with_option_two("bar")
                            .with_option_three("baz")
                    })
                    .with_collator(|collator| {
                        collator
                            .with_option_one("foo")
                            .with_option_two("bar")
                            .with_option_three("baz")
                    })
            });
    }
}
