//! Pure encode/decode between [`RegistryValue`] and raw `REG_*` byte buffers
//! as stored by `RegSetValueExW` / returned by `RegQueryValueExW`.
//!
//! Strings are UTF-16LE. `REG_SZ`/`REG_EXPAND_SZ` are NUL-terminated;
//! `REG_MULTI_SZ` is a list of NUL-terminated strings closed by an extra NUL.

use shared_types::{AppResult, RegistryValue};

use crate::errors::internal;

pub const REG_NONE: u32 = 0;
pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;
pub const REG_QWORD: u32 = 11;

/// Encodes a value into `(REG_* type, bytes)` exactly as it must be stored.
///
/// Fails when a string contains an interior NUL (it would be silently
/// truncated by the registry) or when a multi-string list ends with an empty
/// entry (indistinguishable from the list terminator).
pub fn encode(value: &RegistryValue) -> AppResult<(u32, Vec<u8>)> {
    match value {
        RegistryValue::Dword(v) => Ok((REG_DWORD, v.to_le_bytes().to_vec())),
        RegistryValue::Qword(v) => Ok((REG_QWORD, v.to_le_bytes().to_vec())),
        RegistryValue::String(s) => Ok((REG_SZ, utf16_bytes(s)?)),
        RegistryValue::ExpandString(s) => Ok((REG_EXPAND_SZ, utf16_bytes(s)?)),
        RegistryValue::Binary(b) => Ok((REG_BINARY, b.clone())),
        RegistryValue::MultiString(list) => Ok((REG_MULTI_SZ, encode_multi(list)?)),
        _ => Err(internal("unsupported registry value variant")),
    }
}

/// Decodes a raw registry value of type `reg_type`.
pub fn decode(reg_type: u32, data: &[u8]) -> AppResult<RegistryValue> {
    match reg_type {
        REG_DWORD => {
            let bytes: [u8; 4] = data
                .try_into()
                .map_err(|_| internal(format!("REG_DWORD with {} bytes", data.len())))?;
            Ok(RegistryValue::Dword(u32::from_le_bytes(bytes)))
        }
        REG_QWORD => {
            let bytes: [u8; 8] = data
                .try_into()
                .map_err(|_| internal(format!("REG_QWORD with {} bytes", data.len())))?;
            Ok(RegistryValue::Qword(u64::from_le_bytes(bytes)))
        }
        REG_SZ => Ok(RegistryValue::String(decode_sz(data)?)),
        REG_EXPAND_SZ => Ok(RegistryValue::ExpandString(decode_sz(data)?)),
        REG_MULTI_SZ => Ok(RegistryValue::MultiString(decode_multi(data)?)),
        REG_BINARY => Ok(RegistryValue::Binary(data.to_vec())),
        other => Err(internal(format!("unsupported registry value type {other}"))),
    }
}

/// UTF-16LE bytes of `s` followed by a terminating NUL.
fn utf16_bytes(s: &str) -> AppResult<Vec<u8>> {
    if s.contains('\0') {
        return Err(internal("registry string contains an interior NUL"));
    }
    let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    Ok(out)
}

fn encode_multi(list: &[String]) -> AppResult<Vec<u8>> {
    if list.last().is_some_and(String::is_empty) {
        return Err(internal("REG_MULTI_SZ must not end with an empty entry"));
    }
    let mut out = Vec::new();
    for item in list {
        out.extend(utf16_bytes(item)?);
    }
    // List terminator. An empty list becomes the canonical two-NUL form.
    out.extend_from_slice(&[0, 0]);
    if list.is_empty() {
        out.extend_from_slice(&[0, 0]);
    }
    Ok(out)
}

fn units(data: &[u8]) -> AppResult<Vec<u16>> {
    let (chunks, rest) = data.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(internal(format!(
            "UTF-16 registry data has odd length {}",
            data.len()
        )));
    }
    Ok(chunks.iter().map(|c| u16::from_le_bytes(*c)).collect())
}

fn from_units(units: &[u16]) -> AppResult<String> {
    String::from_utf16(units).map_err(|_| internal("registry string is not valid UTF-16"))
}

fn decode_sz(data: &[u8]) -> AppResult<String> {
    let all = units(data)?;
    // A missing terminator is tolerated; data after the first NUL is ignored.
    let end = all.iter().position(|u| *u == 0).unwrap_or(all.len());
    from_units(all.get(..end).unwrap_or(&[]))
}

fn decode_multi(data: &[u8]) -> AppResult<Vec<String>> {
    let mut all = units(data)?;
    while all.last() == Some(&0) {
        all.pop();
    }
    if all.is_empty() {
        return Ok(Vec::new());
    }
    all.split(|u| *u == 0).map(from_units).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: RegistryValue) {
        let (t, bytes) = encode(&v).unwrap();
        assert_eq!(decode(t, &bytes).unwrap(), v);
    }

    #[test]
    fn dword_and_qword() {
        assert_eq!(
            encode(&RegistryValue::Dword(0x0102_0304)).unwrap(),
            (REG_DWORD, vec![4, 3, 2, 1])
        );
        roundtrip(RegistryValue::Dword(u32::MAX));
        roundtrip(RegistryValue::Qword(u64::MAX - 5));
        assert!(decode(REG_DWORD, &[1, 2, 3]).is_err());
        assert!(decode(REG_QWORD, &[0; 4]).is_err());
    }

    #[test]
    fn strings_are_utf16_nul_terminated() {
        let (t, b) = encode(&RegistryValue::String("aé".into())).unwrap();
        assert_eq!(t, REG_SZ);
        assert_eq!(b, vec![b'a', 0, 0xE9, 0, 0, 0]);
        roundtrip(RegistryValue::String(String::new()));
        roundtrip(RegistryValue::String("日本語 😀".into()));
        roundtrip(RegistryValue::ExpandString("%SystemRoot%\\x".into()));
    }

    #[test]
    fn sz_decode_is_lenient_about_terminator() {
        assert_eq!(
            decode(REG_SZ, &[b'h', 0, b'i', 0]).unwrap(),
            RegistryValue::String("hi".into())
        );
        assert_eq!(
            decode(REG_SZ, &[b'h', 0, 0, 0, b'x', 0]).unwrap(),
            RegistryValue::String("h".into())
        );
        assert_eq!(
            decode(REG_SZ, &[]).unwrap(),
            RegistryValue::String(String::new())
        );
        assert!(decode(REG_SZ, &[1, 0, 2]).is_err());
        // lone surrogate
        assert!(decode(REG_SZ, &[0x00, 0xD8]).is_err());
    }

    #[test]
    fn interior_nul_is_rejected() {
        assert!(encode(&RegistryValue::String("a\0b".into())).is_err());
        assert!(encode(&RegistryValue::MultiString(vec!["a\0".into()])).is_err());
    }

    #[test]
    fn multi_sz_layout_and_roundtrip() {
        let v = RegistryValue::MultiString(vec!["ab".into(), "c".into()]);
        let (t, b) = encode(&v).unwrap();
        assert_eq!(t, REG_MULTI_SZ);
        assert_eq!(b, vec![b'a', 0, b'b', 0, 0, 0, b'c', 0, 0, 0, 0, 0]);
        roundtrip(v);
        roundtrip(RegistryValue::MultiString(vec![]));
        roundtrip(RegistryValue::MultiString(vec![
            "a".into(),
            String::new(),
            "b".into(),
        ]));
        assert!(encode(&RegistryValue::MultiString(vec!["a".into(), String::new()])).is_err());
    }

    #[test]
    fn multi_sz_empty_forms_decode_to_empty_list() {
        for raw in [&[][..], &[0, 0][..], &[0, 0, 0, 0][..]] {
            assert_eq!(
                decode(REG_MULTI_SZ, raw).unwrap(),
                RegistryValue::MultiString(vec![])
            );
        }
        // Missing final terminator is tolerated.
        assert_eq!(
            decode(REG_MULTI_SZ, &[b'a', 0, 0, 0]).unwrap(),
            RegistryValue::MultiString(vec!["a".into()])
        );
    }

    #[test]
    fn binary_is_exact() {
        roundtrip(RegistryValue::Binary(vec![]));
        roundtrip(RegistryValue::Binary(vec![0, 1, 2, 255, 0]));
    }

    #[test]
    fn unsupported_types_error() {
        assert!(decode(REG_NONE, &[]).is_err());
        assert!(decode(5, &[0, 0, 0, 1]).is_err()); // REG_DWORD_BIG_ENDIAN
    }
}
