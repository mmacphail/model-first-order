use bigdecimal::BigDecimal;
use serde::{self, Deserialize, Deserializer, Serializer};

pub fn serialize_bigdecimal_as_string<S>(value: &BigDecimal, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&value.to_string())
}

pub fn deserialize_bigdecimal_from_string<'de, D>(d: D) -> Result<BigDecimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    s.parse::<BigDecimal>().map_err(serde::de::Error::custom)
}
