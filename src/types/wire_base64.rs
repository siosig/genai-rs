//! Base64 encoding of binary fields, matching the Python SDK on the wire.
//!
//! Python encodes request bytes with `base64.urlsafe_b64encode` and its
//! pydantic models decode either alphabet, so requests built here use the
//! URL-safe alphabet too, and responses (which proto3 JSON writes with the
//! standard alphabet) are accepted in both.

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::URL_SAFE},
};
use serde::{Deserialize, Deserializer, Serializer};
use serde_with::{DeserializeAs, SerializeAs};

/// Accepts correct padding or none, but rejects stray `=` (mirrors
/// pydantic-core's permissive base64 decoding, which rejects e.g. a full
/// 64-character quad followed by `=`).
const INDIFFERENT: GeneralPurposeConfig =
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
const URL_SAFE_INDIFFERENT: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, INDIFFERENT);
const STANDARD_INDIFFERENT: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, INDIFFERENT);

/// `serde_with` adapter for `Vec<u8>` fields: URL-safe padded on output,
/// either alphabet (padded or not) on input.
pub struct WireBase64;

impl SerializeAs<Vec<u8>> for WireBase64 {
    fn serialize_as<S: Serializer>(source: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE.encode(source))
    }
}

impl<'de> DeserializeAs<'de, Vec<u8>> for WireBase64 {
    fn deserialize_as<D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        // Like pydantic-core: try the URL-safe alphabet, then the standard one.
        URL_SAFE_INDIFFERENT
            .decode(&encoded)
            .or_else(|_| STANDARD_INDIFFERENT.decode(&encoded))
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::WireBase64;

    #[serde_with::serde_as]
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        #[serde_as(as = "WireBase64")]
        data: Vec<u8>,
    }

    const BYTES: [u8; 3] = [0xfb, 0xff, 0xfe];

    #[test]
    fn serializes_with_the_url_safe_alphabet() {
        let json = serde_json::to_string(&Holder {
            data: BYTES.to_vec(),
        })
        .unwrap();
        assert_eq!(json, r#"{"data":"-__-"}"#);
    }

    #[test]
    fn deserializes_the_standard_alphabet() {
        let holder: Holder = serde_json::from_str(r#"{"data":"+//+"}"#).unwrap();
        assert_eq!(holder.data, BYTES);
    }

    #[test]
    fn deserializes_the_url_safe_alphabet_without_padding() {
        let holder: Holder = serde_json::from_str(r#"{"data":"-__-"}"#).unwrap();
        assert_eq!(holder.data, BYTES);
    }

    #[test]
    fn rejects_text_that_is_not_base64() {
        assert!(serde_json::from_str::<Holder>(r#"{"data":"not base64!"}"#).is_err());
    }
}
