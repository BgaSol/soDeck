use super::*;
use serde::de::{MapAccess, Visitor};
use std::fmt;

/// serde_json's ordinary maps accept duplicate keys; package inventories must not.
pub(super) fn parse(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    struct Inventory;
    impl<'de> Visitor<'de> for Inventory {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("a unique file inventory") }
        fn visit_map<M: MapAccess<'de>>(self, mut input: M) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = input.next_entry::<String, String>()? {
                if result.insert(key, value).is_some() { return Err(serde::de::Error::custom("Duplicate integrity path")); }
            }
            Ok(result)
        }
    }
    use serde::de::Deserializer;
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let result = decoder.deserialize_map(Inventory)?;
    decoder.end()?;
    Ok(result)
}
