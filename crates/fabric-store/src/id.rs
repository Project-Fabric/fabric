use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::StoreError;

const PREFIX: &str = "blake3:";

/// The identity of a stored object: the BLAKE3 hash of its uncompressed content.
///
/// Rendered as `blake3:<64 lowercase hex chars>`. The algorithm is part of the rendered
/// identity so a future hash change can coexist with existing stores. Only lowercase hex is
/// accepted, so every object has exactly one spelling, and therefore exactly one path, even
/// on case-insensitive filesystems.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId([u8; 32]);

impl ObjectId {
    /// Computes the identity of `content`.
    pub fn of(content: &[u8]) -> Self {
        Self::from(blake3::hash(content))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The 64-character lowercase hex form, without the algorithm prefix.
    pub fn to_hex(&self) -> String {
        blake3::Hash::from(self.0).to_hex().to_string()
    }

    /// Parses the bare 64-character lowercase hex form.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let canonical =
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !canonical {
            return None;
        }
        blake3::Hash::from_hex(hex).ok().map(Self::from)
    }
}

impl From<blake3::Hash> for ObjectId {
    fn from(hash: blake3::Hash) -> Self {
        Self(*hash.as_bytes())
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.to_hex())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({self})")
    }
}

impl FromStr for ObjectId {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.strip_prefix(PREFIX)
            .and_then(Self::from_hex)
            .ok_or_else(|| StoreError::InvalidId(s.to_owned()))
    }
}

impl Serialize for ObjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
