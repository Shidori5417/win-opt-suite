//! GUID parsing/formatting (`8-4-4-4-12`, lowercase, no braces).

use shared_types::AppResult;
use shared_types::action::is_valid_guid;

use crate::errors::internal;

/// Field layout identical to the Win32 `GUID` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidParts {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// Parses `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. Surrounding braces are
/// tolerated; any other deviation is an error.
pub fn parse_guid(value: &str) -> AppResult<GuidParts> {
    let bare = value.trim().trim_start_matches('{').trim_end_matches('}');
    if !is_valid_guid(bare) {
        return Err(internal(format!("invalid GUID `{value}`")));
    }
    let hex: String = bare.chars().filter(|c| *c != '-').collect();
    let n =
        u128::from_str_radix(&hex, 16).map_err(|_| internal(format!("invalid GUID `{value}`")))?;
    let bytes = n.to_be_bytes();
    let [a, b, c, d, e, f, g, h, i, j, k, l, m, n2, o, p] = bytes;
    Ok(GuidParts {
        data1: u32::from_be_bytes([a, b, c, d]),
        data2: u16::from_be_bytes([e, f]),
        data3: u16::from_be_bytes([g, h]),
        data4: [i, j, k, l, m, n2, o, p],
    })
}

/// Canonical lowercase form without braces.
pub fn format_guid(g: &GuidParts) -> String {
    let d = g.data4;
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

    #[test]
    fn parse_and_format_roundtrip() {
        let g = parse_guid(HIGH_PERF).unwrap();
        assert_eq!(g.data1, 0x8c5e7fda);
        assert_eq!(g.data2, 0xe8bf);
        assert_eq!(g.data3, 0x4a96);
        assert_eq!(g.data4, [0x9a, 0x85, 0xa6, 0xe2, 0x3a, 0x8c, 0x63, 0x5c]);
        assert_eq!(format_guid(&g), HIGH_PERF);
    }

    #[test]
    fn uppercase_and_braces_are_normalised() {
        let g = parse_guid("{8C5E7FDA-E8BF-4A96-9A85-A6E23A8C635C}").unwrap();
        assert_eq!(format_guid(&g), HIGH_PERF);
    }

    #[test]
    fn invalid_guids_are_rejected() {
        for bad in [
            "",
            "abc",
            "8c5e7fda-e8bf-4a96-9a85",
            "zzzzzzzz-e8bf-4a96-9a85-a6e23a8c635c",
        ] {
            assert!(parse_guid(bad).is_err(), "{bad}");
        }
    }
}
