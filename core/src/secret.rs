//! `Secret`: a string that is wiped from memory when dropped and never
//! printed. Used for every password field of the message protocol, so a
//! password parsed from JSON goes straight into a `Zeroizing<String>`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::Deref;
use zeroize::Zeroizing;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(s: String) -> Secret {
        Secret(Zeroizing::new(s))
    }

    /// Moves the secret out without copying it.
    pub fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl Deref for Secret {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Secret {
    fn from(s: &str) -> Secret {
        Secret::new(s.to_string())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Secret, D::Error> {
        // The String is created once by the deserializer and moved, not
        // copied, into the zeroizing wrapper.
        String::deserialize(d).map(Secret::new)
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_and_round_trips() {
        let s: Secret = serde_json::from_str(r#""hunter2""#).unwrap();
        assert_eq!(&*s, "hunter2");
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""hunter2""#);
        let z = s.into_zeroizing();
        assert_eq!(z.as_str(), "hunter2");
    }
}
