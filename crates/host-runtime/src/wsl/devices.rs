use crate::{Error, Result};
use ai_phone_api::wire::NativeWslDevicesReady;
use prost::Message;

pub(crate) fn decode_receipt(bytes: &[u8], bridge_id: &str) -> Result<NativeWslDevicesReady> {
    let value = NativeWslDevicesReady::decode(bytes)
        .map_err(|_| Error::Invalid("wsl_device_setup_receipt"))?;
    if value.version != 1
        || value.bridge_id != bridge_id
        || value.desktop_id.is_empty()
        || value.desktop_id.len() > 128
        || value.desktop_id.chars().any(char::is_whitespace)
    {
        return Err(Error::Invalid("wsl_device_setup_receipt"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_an_exact_pair_and_an_observed_desktop() {
        let id = "a".repeat(32);
        let value = NativeWslDevicesReady {
            version: 1,
            bridge_id: id.clone(),
            desktop_id: "fixture-desktop".into(),
        };
        let payload = value.encode_to_vec();
        assert_eq!(decode_receipt(&payload, &id).unwrap(), value);
        assert!(decode_receipt(&payload, &"b".repeat(32)).is_err());
        for invalid in [&[][..], &payload[..payload.len() - 1], b"ready"] {
            assert!(decode_receipt(invalid, &id).is_err());
        }
    }
}
