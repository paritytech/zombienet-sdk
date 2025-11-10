use serde::Deserializer;

pub fn default_as_empty_vec<'de, D, T>(_deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Vec::new())
}
