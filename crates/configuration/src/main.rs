use configuration::{ConfigError, NetworkConfigBuilder};

pub fn main() -> Result<(), ConfigError> {
    let network_config = NetworkConfigBuilder::new()
        .with_global_settings(|g| g)
        .with_relaychain(|r| r)
        .with_parachain(|para| para)
        .build()?;

    println!("{}", network_config.dump()?);
    Ok(())
}
