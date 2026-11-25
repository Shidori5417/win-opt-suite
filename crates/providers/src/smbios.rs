//! Pure, platform-independent SMBIOS `RSMB` firmware table parser.
//!
//! Used by `WindowsProbeSource::chassis_types` to extract System Enclosure
//! (SMBIOS Type 3) chassis codes from `GetSystemFirmwareTable('RSMB', 0, ...)`
//! without requiring Windows to unit-test the binary framing logic.

/// Minimum formatted length of any SMBIOS structure header (Type, Length, Handle).
const MIN_STRUCT_LEN: usize = 4;
/// Offset of `RawSMBIOSData.SMBIOSTableData` after the 8-byte Windows header.
const RAW_HEADER_LEN: usize = 8;
/// SMBIOS Type 3: System Enclosure or Chassis.
const TYPE_SYSTEM_ENCLOSURE: u8 = 3;
/// SMBIOS Type 127: End-of-Table marker.
const TYPE_END_OF_TABLE: u8 = 127;
/// Byte offset of `Type` within a Type 3 structure (bits 6..=0 hold the code;
/// bit 7 indicates whether a chassis lock is present).
const CHASSIS_TYPE_OFFSET: usize = 5;

/// Extracts SMBIOS Type 3 chassis type codes from a raw `RSMB` buffer returned
/// by `GetSystemFirmwareTable('RSMB', 0, ...)`.
///
/// The buffer starts with the 8-byte `RawSMBIOSData` header (`u8` method,
/// major, minor, DMI revision, followed by little-endian `u32` `Length`) and
/// the SMBIOS structure stream immediately after. Malformed or truncated
/// structures stop parsing cleanly and return any codes already collected.
pub fn parse_smbios_chassis(raw: &[u8]) -> Vec<u16> {
    let Some(len_slice) = raw.get(4..RAW_HEADER_LEN) else {
        return Vec::new();
    };
    let Ok(len_bytes) = <[u8; 4]>::try_from(len_slice) else {
        return Vec::new();
    };
    let declared_len = u32::from_le_bytes(len_bytes) as usize;
    let Some(rest) = raw.get(RAW_HEADER_LEN..) else {
        return Vec::new();
    };
    let table_len = declared_len.min(rest.len());
    let Some(table) = rest.get(..table_len) else {
        return Vec::new();
    };

    let mut codes = Vec::new();
    let mut pos = 0usize;
    while let (Some(&struct_type), Some(&formatted_len)) = (
        table.get(pos),
        pos.checked_add(1).and_then(|idx| table.get(idx)),
    ) {
        let fmt_len = usize::from(formatted_len);
        if fmt_len < MIN_STRUCT_LEN {
            break;
        }
        let Some(formatted_end) = pos.checked_add(fmt_len) else {
            break;
        };
        if formatted_end > table.len() {
            break;
        }

        if struct_type == TYPE_SYSTEM_ENCLOSURE
            && fmt_len > CHASSIS_TYPE_OFFSET
            && let Some(&raw_chassis) = pos
                .checked_add(CHASSIS_TYPE_OFFSET)
                .and_then(|idx| table.get(idx))
        {
            let code = u16::from(raw_chassis & 0x7F);
            if code != 0 {
                codes.push(code);
            }
        }

        if struct_type == TYPE_END_OF_TABLE {
            break;
        }

        let Some(tail) = table.get(formatted_end..) else {
            break;
        };
        let Some(rel_end) = tail.windows(2).position(|pair| pair == [0, 0]) else {
            break;
        };
        let Some(next_pos) = formatted_end
            .checked_add(rel_end)
            .and_then(|end| end.checked_add(2))
        else {
            break;
        };
        pos = next_pos;
    }

    codes
}

/// Returns `true` when firmware identity strings (SMBIOS system manufacturer /
/// product name) belong to a known hypervisor's virtual hardware.
pub fn is_virtual_hardware(manufacturer: &str, product: &str) -> bool {
    let m = manufacturer.to_ascii_lowercase();
    let p = product.to_ascii_lowercase();
    const MANUFACTURERS: &[&str] = &[
        "vmware",
        "innotek",
        "qemu",
        "xen",
        "parallels",
        "red hat",
        "bochs",
    ];
    const PRODUCTS: &[&str] = &[
        "virtualbox",
        "vmware",
        "kvm",
        "qemu",
        "hvm domu",
        "parallels",
        "bochs",
    ];
    MANUFACTURERS.iter().any(|v| m.contains(v))
        || PRODUCTS.iter().any(|v| p.contains(v))
        // Hyper-V guest: "Microsoft Corporation" + "Virtual Machine"
        || (m.contains("microsoft") && p == "virtual machine")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_rsmb(table: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 3, 6, 0];
        out.extend_from_slice(&(table.len() as u32).to_le_bytes());
        out.extend_from_slice(table);
        out
    }

    #[test]
    fn parses_type3_after_earlier_structures_and_strings() {
        let mut table = Vec::new();
        // Type 0 (BIOS Info), length 6, with two strings
        table.extend_from_slice(&[0, 6, 0x00, 0x00, 0x01, 0x02]);
        table.extend_from_slice(b"American Megatrends\0F12\0\0");
        // Type 1 (System Info), length 5, no strings (double NUL)
        table.extend_from_slice(&[1, 5, 0x01, 0x00, 0x00]);
        table.extend_from_slice(&[0, 0]);
        // Type 3 (Chassis), length 9, Notebook (10) with chassis-lock bit (0x80 | 10)
        table.extend_from_slice(&[3, 9, 0x03, 0x00, 0x01, 0x8A, 0x03, 0x03, 0x03]);
        table.extend_from_slice(b"LENOVO\0\0");
        // Type 127 (End-of-Table), length 4, double NUL
        table.extend_from_slice(&[127, 4, 0x7F, 0x00, 0, 0]);

        assert_eq!(parse_smbios_chassis(&wrap_rsmb(&table)), vec![10]);
    }

    #[test]
    fn parses_multiple_enclosures_and_stops_at_end_of_table() {
        let mut table = Vec::new();
        // Desktop (3)
        table.extend_from_slice(&[3, 6, 1, 0, 0, 3, 0, 0]);
        // Convertible (31)
        table.extend_from_slice(&[3, 6, 2, 0, 0, 31, 0, 0]);
        // End-of-table
        table.extend_from_slice(&[127, 4, 3, 0, 0, 0]);
        // Trailing Type 3 after End-of-Table must be ignored
        table.extend_from_slice(&[3, 6, 4, 0, 0, 9, 0, 0]);

        assert_eq!(parse_smbios_chassis(&wrap_rsmb(&table)), vec![3, 31]);
    }

    #[test]
    fn malformed_and_truncated_buffers_do_not_panic() {
        assert!(parse_smbios_chassis(&[]).is_empty());
        assert!(parse_smbios_chassis(&[0, 1, 2, 3, 4]).is_empty());
        // Declared length larger than buffer
        assert!(parse_smbios_chassis(&[0, 3, 0, 0, 100, 0, 0, 0]).is_empty());
        // Invalid formatted length (< 4) stops cleanly
        assert!(parse_smbios_chassis(&wrap_rsmb(&[3, 2, 0, 0, 0, 9, 0, 0])).is_empty());
        // Type 3 too short for offset 5 is skipped safely; later valid Type 3 still parses
        let mut table = Vec::new();
        table.extend_from_slice(&[3, 4, 1, 0, 0, 0]);
        table.extend_from_slice(&[3, 6, 2, 0, 0, 9, 0, 0]);
        assert_eq!(parse_smbios_chassis(&wrap_rsmb(&table)), vec![9]);
        // Missing double-NUL terminator
        assert_eq!(
            parse_smbios_chassis(&wrap_rsmb(&[3, 6, 1, 0, 0, 14, b'A'])),
            vec![14]
        );
    }

    #[test]
    fn virtual_hardware_detection() {
        assert!(is_virtual_hardware("VMware, Inc.", "VMware20,1"));
        assert!(is_virtual_hardware("innotek GmbH", "VirtualBox"));
        assert!(is_virtual_hardware(
            "QEMU",
            "Standard PC (Q35 + ICH9, 2009)"
        ));
        assert!(is_virtual_hardware(
            "Microsoft Corporation",
            "Virtual Machine"
        ));
        assert!(!is_virtual_hardware(
            "Microsoft Corporation",
            "Surface Laptop 7"
        ));
        assert!(!is_virtual_hardware("ASUS", "ROG STRIX B650E-F"));
        assert!(!is_virtual_hardware("", ""));
    }
}
