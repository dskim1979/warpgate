Fork of `ironrdp-pdu` 0.9.0 (MOAVI / MAKUS SYSTEMS build of Warpgate).

## Lenient `keyboardType` in Client Core Data

`ClientCoreData::decode` rejected any `keyboardType` outside 1..=7 with
`invalid keyboardType`, so every Korean Windows `mstsc` client failed the GCC
Conference Create Request: Korean Windows sends `keyboardType = 8`, the Korean
keyboard type documented by Win32 `GetKeyboardType`, although MS-RDPBCGR
2.2.1.3.2 lists only 1..=7. Other RDP servers (Windows, xrdp, Weston) accept it.

The fork:

- adds `KeyboardType::Korean = 8`;
- adds `KeyboardType::from_u32_lenient`, which maps any other unknown value to
  `IbmEnhanced` instead of failing;
- uses it in `ClientCoreData::decode` (the server only logs this field; Warpgate
  connects to targets with its own `IbmEnhanced` setting);
- adds unit tests (`gcc::core_data::client::tests`).

`warpgate.patch` reproduces the source change. `Cargo.toml` additionally sets
`[lints.rust] warnings = { level = "allow", priority = 1 }` like the other vendored
IronRDP crates; re-apply it by hand on re-vendor. Upstream text: `UPSTREAM.md`
at the repository root. Drop this fork once an IronRDP release accepts value 8.
