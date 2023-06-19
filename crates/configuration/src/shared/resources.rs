use std::error::Error;

use lazy_static::lazy_static;
use regex::Regex;

use super::{
    errors::{ConversionError, FieldError},
    helpers::merge_errors,
};

#[derive(Debug, Clone, PartialEq)]
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
                .expect("should compile with success. this is a bug, please report it");
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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu: Option<ResourceQuantity>,
    limit_memory: Option<ResourceQuantity>,
    limit_cpu: Option<ResourceQuantity>,
}

impl Resources {
    pub fn request_memory(&self) -> Option<&ResourceQuantity> {
        self.request_memory.as_ref()
    }

    pub fn request_cpu(&self) -> Option<&ResourceQuantity> {
        self.request_cpu.as_ref()
    }

    pub fn limit_memory(&self) -> Option<&ResourceQuantity> {
        self.limit_memory.as_ref()
    }

    pub fn limit_cpu(&self) -> Option<&ResourceQuantity> {
        self.limit_cpu.as_ref()
    }
}

#[derive(Debug, Default)]
pub struct ResourcesBuilder {
    config: Resources,
    errors: Vec<Box<dyn Error>>,
}

impl ResourcesBuilder {
    pub fn new() -> ResourcesBuilder {
        Self::default()
    }

    fn transition(config: Resources, errors: Vec<Box<dyn Error>>) -> Self {
        Self { config, errors }
    }

    pub fn with_request_memory<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + 'static,
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
                merge_errors(self.errors, FieldError::RequestMemory(error).into()),
            ),
        }
    }

    pub fn with_request_cpu<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + 'static,
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
                merge_errors(self.errors, FieldError::RequestCpu(error).into()),
            ),
        }
    }

    pub fn with_limit_memory<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + 'static,
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
                merge_errors(self.errors, FieldError::LimitMemory(error).into()),
            ),
        }
    }

    pub fn with_limit_cpu<T>(self, quantity: T) -> Self
    where
        T: TryInto<ResourceQuantity>,
        T::Error: Error + 'static,
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
                merge_errors(self.errors, FieldError::LimitCpu(error).into()),
            ),
        }
    }

    pub fn build(self) -> Result<Resources, Vec<Box<dyn Error>>> {
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

    macro_rules! resources_quantity_unit_test_impl {
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
    fn we_should_be_able_to_convert_a_string_a_resource_quantity_without_unit() {
        resources_quantity_unit_test_impl!("1000");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_m_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("100m");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_K_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("50K");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_M_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("100M");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_G_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("1G");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_T_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.01T");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_P_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.00001P");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_E_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.000000001E");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Ki_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("50Ki");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Mi_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("100Mi");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Gi_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("1Gi");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Ti_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.01Ti");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Pi_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.00001Pi");
    }

    #[test]
    fn we_should_be_able_to_convert_a_str_with_Ei_unit_into_a_resource_quantity() {
        resources_quantity_unit_test_impl!("0.000000001Ei");
    }

    #[test]
    fn resources_config_builder_should_returns_a_resources_config_correctly() {
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
    fn resources_config_builder_should_returns_an_error_if_couldnt_parse_request_memory() {
        let resources_builder = ResourcesBuilder::new().with_request_memory("invalid");

        let got = resources_builder.build().err().unwrap();

        assert_eq!(got.len(), 1);
        assert!(matches!(
            got.first()
                .unwrap()
                .downcast_ref::<FieldError<ConversionError>>()
                .unwrap(),
            FieldError::RequestMemory(ConversionError::DoesntMatchRegex { value: _, regex: _ })
        ));
        assert_eq!(
            got.first().unwrap().to_string(),
            r"request_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_returns_an_error_if_couldnt_parse_request_cpu() {
        let resources_builder = ResourcesBuilder::new().with_request_cpu("invalid");

        let got = resources_builder.build().err().unwrap();

        assert_eq!(got.len(), 1);
        assert!(matches!(
            got.first()
                .unwrap()
                .downcast_ref::<FieldError<ConversionError>>()
                .unwrap(),
            FieldError::RequestCpu(ConversionError::DoesntMatchRegex { value: _, regex: _ })
        ));
        assert_eq!(
            got.first().unwrap().to_string(),
            r"request_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_returns_an_error_if_couldnt_parse_limit_memory() {
        let resources_builder = ResourcesBuilder::new().with_limit_memory("invalid");

        let got = resources_builder.build().err().unwrap();

        assert_eq!(got.len(), 1);
        assert!(matches!(
            got.first()
                .unwrap()
                .downcast_ref::<FieldError<ConversionError>>()
                .unwrap(),
            FieldError::LimitMemory(ConversionError::DoesntMatchRegex { value: _, regex: _ })
        ));
        assert_eq!(
            got.first().unwrap().to_string(),
            r"limit_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn resources_config_builder_should_returns_an_error_if_couldnt_parse_limit_cpu() {
        let resources_builder = ResourcesBuilder::new().with_limit_cpu("invalid");

        let got = resources_builder.build().err().unwrap();

        assert_eq!(got.len(), 1);
        assert!(matches!(
            got.first()
                .unwrap()
                .downcast_ref::<FieldError<ConversionError>>()
                .unwrap(),
            FieldError::LimitCpu(ConversionError::DoesntMatchRegex { value: _, regex: _ })
        ));
        assert_eq!(
            got.first().unwrap().to_string(),
            r"limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }
}
