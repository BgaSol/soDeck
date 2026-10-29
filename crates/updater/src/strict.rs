//! serde_json's default object parser accepts duplicate keys. Signed formats must not.
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("JSON without duplicate keys") }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> { Ok(Unique(Value::Bool(v))) }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> { Ok(Unique(v.into())) }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> { Ok(Unique(v.into())) }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Unique, E> { Number::from_f64(v).map(|n|Unique(Value::Number(n))).ok_or_else(||E::custom("invalid number")) }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> { Ok(Unique(v.into())) }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> { Ok(Unique(v.into())) }
            fn visit_none<E: de::Error>(self) -> Result<Unique, E> { Ok(Unique(Value::Null)) }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> { Ok(Unique(Value::Null)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v=Vec::new();while let Some(Unique(x))=a.next_element()? { v.push(x); } Ok(Unique(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v=Map::new();while let Some(k)=a.next_key::<String>()? {
                    if v.contains_key(&k) { return Err(de::Error::custom("duplicate JSON key")); }
                    v.insert(k,a.next_value::<Unique>()?.0);
                } Ok(Unique(Value::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}
pub fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], limit: usize) -> crate::Result<T> {
    if bytes.len()>limit { return Err(crate::error("CAPACITY","更新数据超出容量")); }
    let unique:Unique=serde_json::from_slice(bytes).map_err(|_|crate::error("UPDATE_SOURCE_INVALID","更新数据不是有效的唯一键JSON"))?;
    serde_json::from_value(unique.0).map_err(|_|crate::error("UPDATE_SOURCE_INVALID","更新数据字段或版本无效"))
}

#[cfg(test)]
mod tests {
    #[test] fn rejects_duplicate_nested_fields(){
        assert!(super::parse::<serde_json::Value>(br#"{"a":{"v":1,"v":2}}"#,1024).is_err());
        assert!(super::parse::<serde_json::Value>(br#"{"a":[null,true,1,"x"]}"#,1024).is_ok());
    }
}
