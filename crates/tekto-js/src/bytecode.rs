//! Versioned envelope for QuickJS bytecode.
//!
//! QuickJS bytecode is only valid for the engine that produced it. The
//! envelope carries a format number and an engine tag, and the loader refuses
//! anything else so an app can fall back to its source bundle instead.

use crate::JsError;

/// First eight bytes of a Tekto bytecode file.
pub const BYTECODE_MAGIC: &[u8; 8] = b"TEKTOQBC";
/// Envelope version. Bump when the layout of this header changes.
pub const BYTECODE_FORMAT: u32 = 1;
/// Embedded engine identity. Bump when rquickjs or QuickJS-ng changes.
pub const BYTECODE_ENGINE: &str = "rquickjs-0.14";

fn header_len(engine: &[u8]) -> usize {
    8 + 4 + 4 + engine.len()
}

/// Wrap raw QuickJS bytecode in the Tekto header.
pub fn encode_bytecode(payload: &[u8]) -> Vec<u8> {
    let engine = BYTECODE_ENGINE.as_bytes();
    let mut out = Vec::with_capacity(header_len(engine) + payload.len());
    out.extend_from_slice(BYTECODE_MAGIC);
    out.extend_from_slice(&BYTECODE_FORMAT.to_le_bytes());
    out.extend_from_slice(&(engine.len() as u32).to_le_bytes());
    out.extend_from_slice(engine);
    out.extend_from_slice(payload);
    out
}

/// Check the header and return the raw bytecode.
pub fn decode_bytecode(bytes: &[u8]) -> Result<&[u8], JsError> {
    if bytes.len() < 16 || &bytes[..8] != BYTECODE_MAGIC {
        return Err(JsError::Message(
            "not Tekto bytecode; rebuild the bundle from source".into(),
        ));
    }
    let format = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
    if format != BYTECODE_FORMAT {
        return Err(JsError::Message(format!(
            "bytecode format {format} is not {BYTECODE_FORMAT}; rebuild the bundle from source"
        )));
    }
    let engine_len = u32::from_le_bytes(bytes[12..16].try_into().expect("four bytes")) as usize;
    let start = 16;
    let end = start + engine_len;
    if bytes.len() < end {
        return Err(JsError::Message(
            "truncated bytecode header; rebuild the bundle from source".into(),
        ));
    }
    let engine = &bytes[start..end];
    if engine != BYTECODE_ENGINE.as_bytes() {
        return Err(JsError::Message(format!(
            "bytecode was built for `{}`, this engine is `{}`; rebuild the bundle from source",
            String::from_utf8_lossy(engine),
            BYTECODE_ENGINE
        )));
    }
    Ok(&bytes[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_stale_envelopes() {
        let payload = b"quickjs bytecode";
        let bytes = encode_bytecode(payload);
        assert_eq!(decode_bytecode(&bytes).unwrap(), payload);

        assert!(decode_bytecode(b"nope").is_err());
        let mut stale = bytes.clone();
        stale[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(decode_bytecode(&stale)
            .unwrap_err()
            .to_string()
            .contains("rebuild the bundle from source"));

        let mut wrong_engine = bytes.clone();
        wrong_engine[16] = b'X';
        let error = decode_bytecode(&wrong_engine).unwrap_err().to_string();
        assert!(error.contains("this engine is"), "{error}");

        let mut truncated = bytes.clone();
        truncated.truncate(14);
        assert!(decode_bytecode(&truncated).is_err());
    }
}
