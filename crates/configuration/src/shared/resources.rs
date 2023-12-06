use std::error::Error;

use lazy_static::lazy_static;
use regex::Regex;
use serde::{
    de::{self},
    ser::SerializeStruct,
    Deserialize, Serialize,
};

use super::{
    errors::{ConversionError, FieldError},
    helpers::merge_errors,
};
use crate::shared::constants::{SHOULD_COMPILE, THIS_IS_A_BUG};

/// A resource quantity used to define limits (k8s/podman only).
/// It can be constructed from a `&str` or u64, if it fails, it returns a [`ConversionError`].
/// Possible optional prefixes are: m, K, M, G, T, P, E, Ki, Mi, Gi, Ti, Pi, Ei
///
/// # Examples
///
/// ```
/// use zombienet_configuration::shared::resources::ResourceQuantity;
///
/// let quantity1: ResourceQuantity = "100000".try_into().unwrap();
/// let quantity2: ResourceQuantity = "1000m".try_into().unwrap();
/// let quantity3: ResourceQuantity = "1Gi".try_into().unwrap();
/// let quantity4: ResourceQuantity = 10_000.into();
///
/// assert_eq!(quantity1.as_str(), "100000");
/// assert_eq!(quantity2.as_str(), "1000m");
/// assert_eq!(quantity3.as_str(), "1Gi");
/// assert_eq!(quantity4.as_str(), "10000");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceQuantity(String);

impl ResourceQuantity {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ResourceQuantity {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$")
                .expect(&format!("{}, {}", SHOULD_COMPILE, THIS_IS_A_BUG));
        }

        if !RE.is_match(value) {
            return Err(ConversionError::DoesntMatchRegex {
                value: value.to_string(),
                regex: r"^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$".to_string(),
            });
        }

        Ok(Self(value.to_string()))
    }
}

impl From<u64> for ResourceQuantity {
    fn from(value: u64) -> Self {
        Self(value.to_string())
    }
}

/// Resources limits used in the context of podman/k8s.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu: Option<ResourceQuantity>,
    limit_memory: Option<ResourceQuantity>,
    limit_cpu: Option<ResourceQuantity>,
}

#[derive(Serialize, Deserialize)]
struct ResourcesField {
    memory: Option<ResourceQuantity>,
    cpu: Option<ResourceQuantity>,
}

impl Serialize for Resources {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("Resources", 2)?;

        if self.request_memory.is_some() || self.request_memory.is_some() {
            state.serialize_field(
                "requests",
                &ResourcesField {
                    memory: self.request_memory.clone(),
                    cpu: self.request_cpu.clone(),
                },
            )?;
        } else {
            state.skip_field("requests")?;
        }

        if self.limit_memory.is_some() || self.limit_memory.is_some() {
            state.serialize_field(
                "limits",
                &ResourcesField {
                    memory: self.limit_memory.clone(),
                    cpu: self.limit_cpu.clone(),
                },
            )?;
        } else {
            state.skip_field("limits")?;
        }

        state.end()
    }
}

struct ResourcesVisitor;

impl<'de> de::Visitor<'de> for ResourcesVisitor {
    type Value = Resources;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a resources object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut resources: Resources = Resources::default();

        while let Some((key, value)) = map.next_entry::<String, ResourcesField>()? {
            match key.as_str() {
                "requests" => {
                    resources.request_memory = value.memory;
                    resources.request_cpu = value.cpu;
                },
                "limits" => {
                    resources.limit_memory = value.memory;
                    resources.limit_cpu = value.cpu;
                },
                _ => {
                    return Err(de::Error::unknown_field(
                        &key,
                        &["requests", "limits", "cpu", "memory"],
                    ))
                },
            }
        }
        Ok(resources)
    }
}

impl<'de> Deserialize<'de> for Resources {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(ResourcesVisitor)
    }
}

impl Resources {
    /// Memory limit applied to requests.
    pub fn request_memory(&self) -> Option<&ResourceQuantity> {
        self.request_memory.as_ref()
    }

    /// CPU limit applied to requests.
    pub fn request_cpu(&self) -> Option<&ResourceQuantity> {
        self.request_cpu.as_ref()
    }

    /// Overall memory limit applied.
    pub fn limit_memory(&self) -> Option<&ResourceQuantity> {
        self.limit_memory.as_ref()
    }

    /// Overall CPU limit applied.
    pub fn limit_cpu(&self) -> Option<&ResourceQuantity> {
        self.limit_cpu.as_ref()
    }
}

/// A resources builder, used to build a [`Resources`] declaratively with fields validation.
#[derive(Debug, Default)]
pub struct ResourcesBuilder {
    config: Resources,
    errors: Vec<anyhow::Error>,
}

impl ResourcesBuilder {
    pub fn new() -> ResourcesBuilder {
        Self::default()
    }

    fn transition(config: Resources, errors: Vec<anyhow::Error>) -> Self {
        Self { config, errors }
    }

    /// Set the requested memory for a pod. This is the minimum memory allocated for a pod.
    pub fn with_request_memory<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + Send + Sync + 'static,
    {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    request_memory: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::RequestMemory(error.into()).into()),
            ),
        }
    }

    /// Set the requested CPU limit for a pod. This is the minimum CPU allocated for a pod.
    pub fn with_request_cpu<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + Send + Sync + 'static,
    {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    request_cpu: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::RequestCpu(error.into()).into()),
            ),
        }
    }

    /// Set the overall memory limit for a pod. This is the maximum memory threshold for a pod.
    pub fn with_limit_memory<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + Send + Sync + 'static,
    {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    limit_memory: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::LimitMemory(error.into()).into()),
            ),
        }
    }

    /// Set the overall CPU limit for a pod. This is the maximum CPU threshold for a pod.
    pub fn with_limit_cpu<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + Send + Sync + 'static,
    {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    limit_cpu: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::LimitCpu(error.into()).into()),
            ),
        }
    }

    /// Seals the builder and returns a [`Resources`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<Resources, Vec<anyhow::Error>> {
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(self.config)
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use crate::NetworkConfig;

    macro_rules! impl_resources_quantity_unit_test {
        ($val:literal) => {{
            let resources = ResourcesBuilder::new()
                .with_request_memory($val)
                .build()
                .unwrap();

            assert_eq!(resources.request_memory().unwrap().as_str(), $val);
            assert_eq!(resources.request_cpu(), None);
            assert_eq!(resources.limit_cpu(), None);
            assert_eq!(resources.limit_memory(), None);
        }};
    }

    #[test]
    fn converting_a_string_a_resource_quantity_without_unit_should_succeeds() {
        impl_resources_quantity_unit_test!("1000");
    }

    #[test]
    fn converting_a_str_with_m_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("100m");
    }

    #[test]
    fn converting_a_str_with_K_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("50K");
    }

    #[test]
    fn converting_a_str_with_M_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("100M");
    }

    #[test]
    fn converting_a_str_with_G_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("1G");
    }

    #[test]
    fn converting_a_str_with_T_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.01T");
    }

    #[test]
    fn converting_a_str_with_P_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.00001P");
    }

    #[test]
    fn converting_a_str_with_E_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.000000001E");
    }

    #[test]
    fn converting_a_str_with_Ki_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("50Ki");
    }

    #[test]
    fn converting_a_str_with_Mi_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("100Mi");
    }

    #[test]
    fn converting_a_str_with_Gi_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("1Gi");
    }

    #[test]
    fn converting_a_str_with_Ti_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.01Ti");
    }

    #[test]
    fn converting_a_str_with_Pi_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.00001Pi");
    }

    #[test]
    fn converting_a_str_with_Ei_unit_into_a_resource_quantity_should_succeeds() {
        impl_resources_quantity_unit_test!("0.000000001Ei");
    }

    #[test]
    fn resources_config_builder_should_succeeds_and_returns_a_resources_config() {
        let resources = ResourcesBuilder::new()
            .with_request_memory("200M")
            .with_request_cpu("1G")
            .with_limit_cpu("500M")
            .with_limit_memory("2G")
            .build()
            .unwrap();

        assert_eq!(resources.request_memory().unwrap().as_str(), "200M");
        assert_eq!(resources.request_cpu().unwrap().as_str(), "1G");
        assert_eq!(resources.limit_cpu().unwrap().as_str(), "500M");
        assert_eq!(resources.limit_memory().unwrap().as_str(), "2G");
    }

    #[test]
    fn resources_config_toml_import_should_succeeds_and_returns_a_resources_config() {
        let load_from_toml =
            NetworkConfig::load_from_toml("./testing/snapshots/0001-big-network.toml").unwrap();

        let resources = load_from_toml.relaychain().default_resources().unwrap();
        assert_eq!(resources.request_memory().unwrap().as_str(), "500M");
        assert_eq!(resources.request_cpu().unwrap().as_str(), "100000");
        assert_eq!(resources.limit_cpu().unwrap().as_str(), "10Gi");
        assert_eq!(resources.limit_memory().unwrap().as_str(), "4000M");
    }

    #[test]
    fn resources_config_builder_should_fails_and_returns_an_error_if_couldnt_parse_request_memory()
    {
        let resources_builder = ResourcesBuilder::new().with_request_memory("invalid");

        let errors = resources_builder.build().err().unwrap();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"request_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_fails_and_returns_an_error_if_couldnt_parse_request_cpu() {
        let resources_builder = ResourcesBuilder::new().with_request_cpu("invalid");

        let errors = resources_builder.build().err().unwrap();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"request_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_fails_and_returns_an_error_if_couldnt_parse_limit_memory() {
        let resources_builder = ResourcesBuilder::new().with_limit_memory("invalid");

        let errors = resources_builder.build().err().unwrap();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"limit_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_fails_and_returns_an_error_if_couldnt_parse_limit_cpu() {
        let resources_builder = ResourcesBuilder::new().with_limit_cpu("invalid");

        let errors = resources_builder.build().err().unwrap();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_fails_and_returns_multiple_error_if_couldnt_parse_multiple_fields(
    ) {
        let resources_builder = ResourcesBuilder::new()
            .with_limit_cpu("invalid")
            .with_request_memory("invalid");

        let errors = resources_builder.build().err().unwrap();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            r"request_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }
}
