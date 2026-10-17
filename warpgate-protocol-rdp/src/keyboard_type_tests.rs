//! Regression tests for the vendored `ironrdp-pdu` keyboardType fix (MOAVI build).
//!
//! Korean Windows mstsc sends `keyboardType = 8` in its Client Core Data. Stock
//! IronRDP rejected it ("invalid `keyboardType`"), so no Korean client could connect
//! to the native RDP port. See `vendor/ironrdp-pdu/PATCHES.md`.

use ironrdp::core::{decode, encode_vec};
use ironrdp::pdu::gcc::{
    ClientCoreData, ClientCoreOptionalData, ColorDepth, KeyboardType, RdpVersion,
    SecureAccessSequence,
};

/// Byte offset of keyboardType in TS_UD_CS_CORE (MS-RDPBCGR 2.2.1.3.2).
const KEYBOARD_TYPE_OFFSET: usize = 4 + 2 + 2 + 2 + 2 + 4 + 4 + 32;

fn korean_mstsc_core_data() -> ClientCoreData {
    ClientCoreData {
        version: RdpVersion::V5_PLUS,
        desktop_width: 1920,
        desktop_height: 1080,
        color_depth: ColorDepth::Bpp8,
        sec_access_sequence: SecureAccessSequence::Del,
        keyboard_layout: 0x0412, // ko-KR
        client_build: 26100,
        client_name: "KR-PC".to_owned(),
        keyboard_type: KeyboardType::Korean,
        keyboard_subtype: 3,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        optional_data: ClientCoreOptionalData::default(),
    }
}

fn encode_with_raw_keyboard_type(raw: u32) -> anyhow::Result<Vec<u8>> {
    let mut buf = encode_vec(&korean_mstsc_core_data())?;
    let field = buf
        .get_mut(KEYBOARD_TYPE_OFFSET..KEYBOARD_TYPE_OFFSET + 4)
        .ok_or_else(|| anyhow::anyhow!("buffer too short"))?;
    field.copy_from_slice(&raw.to_le_bytes());
    Ok(buf)
}

#[test]
fn client_core_data_with_korean_keyboard_type_decodes() -> anyhow::Result<()> {
    let buf = encode_with_raw_keyboard_type(8)?;
    let data: ClientCoreData = decode(&buf)?;
    assert_eq!(data.keyboard_type, KeyboardType::Korean);
    assert_eq!(data.keyboard_type.as_u32(), 8);
    assert_eq!(data.keyboard_layout, 0x0412);
    assert_eq!(data, korean_mstsc_core_data());
    Ok(())
}

#[test]
fn client_core_data_with_unknown_keyboard_type_is_accepted() -> anyhow::Result<()> {
    for raw in [0u32, 9, 0x51, u32::MAX] {
        let buf = encode_with_raw_keyboard_type(raw)?;
        let data: ClientCoreData = decode(&buf)?;
        assert_eq!(data.keyboard_type, KeyboardType::IbmEnhanced);
    }
    Ok(())
}
