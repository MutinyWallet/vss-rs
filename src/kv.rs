use core::fmt;
use serde::de::Visitor;
use serde::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: ByteData,
    pub version: i64,
}

impl KeyValue {
    pub fn new(key: String, value: Vec<u8>, version: i64) -> KeyValue {
        KeyValue {
            key,
            value: ByteData(value),
            version,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ByteData(pub Vec<u8>);

impl Serialize for ByteData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ByteData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ByteDataVisitor;

        impl<'de> Visitor<'de> for ByteDataVisitor {
            type Value = ByteData;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a Vec<u8> or a base64 encoded string")
            }

            fn visit_str<E>(self, v: &str) -> Result<ByteData, E>
            where
                E: de::Error,
            {
                let decoded =
                    base64::decode(v).map_err(|err| de::Error::custom(err.to_string()))?;
                Ok(ByteData(decoded))
            }

            fn visit_seq<S>(self, seq: S) -> Result<ByteData, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let vec = Vec::<u8>::deserialize(de::value::SeqAccessDeserializer::new(seq))?;
                Ok(ByteData(vec))
            }
        }

        deserializer.deserialize_any(ByteDataVisitor)
    }
}

// need this for backwards compat for now

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValueOld {
    pub key: String,
    pub value: String,
    pub version: i64,
}

impl From<KeyValue> for KeyValueOld {
    fn from(kv: KeyValue) -> Self {
        KeyValueOld {
            key: kv.key,
            value: base64::encode(kv.value.0),
            version: kv.version,
        }
    }
}
