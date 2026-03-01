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

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use std::str::FromStr;

    fn serialize(value: &BigDecimal) -> String {
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut buf);
        serialize_bigdecimal_as_string(value, &mut ser).unwrap();
        let s = String::from_utf8(buf).unwrap();
        serde_json::from_str::<String>(&s).unwrap()
    }

    fn deserialize(s: &str) -> Result<BigDecimal, serde_json::Error> {
        let json = format!("\"{s}\"");
        let mut de = serde_json::Deserializer::from_str(&json);
        deserialize_bigdecimal_from_string(&mut de)
    }

    #[test]
    fn test_serialize_integer_like_bigdecimal() {
        let v = BigDecimal::from(42);
        assert_eq!(serialize(&v), "42");
    }

    #[test]
    fn test_serialize_zero_bigdecimal() {
        let v = BigDecimal::from(0);
        assert_eq!(serialize(&v), "0");
    }

    #[test]
    fn test_serialize_four_decimal_places_preserved() {
        let v = BigDecimal::from_str("12345.6789").unwrap();
        assert_eq!(serialize(&v), "12345.6789");
    }

    #[test]
    fn test_serialize_trailing_zeros_preserved() {
        let v = BigDecimal::from_str("10.0000").unwrap();
        assert_eq!(serialize(&v), "10.0000");
    }

    #[test]
    fn test_deserialize_valid_decimal_string() {
        let result = deserialize("49.9900").unwrap();
        assert_eq!(result, BigDecimal::from_str("49.9900").unwrap());
    }

    #[test]
    fn test_deserialize_integer_string() {
        let result = deserialize("100").unwrap();
        assert_eq!(result, BigDecimal::from(100));
    }

    #[test]
    fn test_deserialize_zero_string() {
        let result = deserialize("0").unwrap();
        assert_eq!(result, BigDecimal::from(0));
    }

    #[test]
    fn test_deserialize_invalid_string_returns_error() {
        assert!(deserialize("not-a-number").is_err());
    }

    #[test]
    fn test_deserialize_empty_string_returns_error() {
        assert!(deserialize("").is_err());
    }

    #[test]
    fn test_roundtrip_high_precision_value() {
        let original = BigDecimal::from_str("12345.6789").unwrap();
        let serialized = serialize(&original);
        let deserialized = deserialize(&serialized).unwrap();
        assert_eq!(deserialized, original);
    }
}
