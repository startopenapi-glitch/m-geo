//! Shared recursive duplicate-key rejection for decoded artifact JSON.
use super::*;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use std::fmt;

// serde_json::Value normally keeps the final duplicate key. Every nested object here
// must instead be unambiguous before any domain field can be trusted.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Unique, M::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, Unique>()? {
                    if result.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(Unique(Value::Object(result)))
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Unique, S::Error> {
                let mut values = vec![];
                while let Some(value) = seq.next_element::<Unique>()? {
                    values.push(value.0);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
pub(super) fn parse(s: &str) -> Result<Value> {
    serde_json::from_str::<Unique>(s)
        .map(|v| v.0)
        .map_err(|_| "JSON 无效或含重复字段".into())
}
