# Upstream drafts (not posted)

Drafts for reporting the Korean `keyboardType` problem upstream. Posting is the
maintainer's (user's) call. Two places: IronRDP (root cause) and Warpgate (impact, and
until an IronRDP release lands, Warpgate vendors `ironrdp-pdu` itself).

---

## 1. IronRDP — issue + PR

**Title:** `ClientCoreData::decode` rejects keyboardType 8 sent by Korean Windows clients

**Body:**

`ironrdp-pdu` (0.9.0, `src/gcc/core_data/client.rs`) decodes the Client Core Data
`keyboardType` field with `KeyboardType::from_u32(..).ok_or_else(invalid_field_err!(...))`.
`KeyboardType` only has the seven values listed in MS-RDPBCGR 2.2.1.3.2 (1..=7).

Windows clients with a Korean keyboard (the default on Korean Windows) send
`keyboardType = 8`. That is the value Win32 `GetKeyboardType(0)` returns for Korean
keyboards (documented in the `GetKeyboardType` reference), and mstsc copies it
into TS_UD_CS_CORE. As a result, every Korean Windows `mstsc` connection to an
IronRDP-based server fails during the GCC Conference Create Request with:

```
ClientCoreData decode error: invalid `keyboardType`: invalid keyboard type
```

Windows RDP servers, xrdp and Weston accept the value. For a server the field is
informational, so failing the whole connection over it seems too strict.

Reproduce without a Korean PC: FreeRDP 2 can send the value,
`xfreerdp /v:<ironrdp-server> /sec:tls /kbd:0x412 /kbd-type:8`.

**Proposed change** (what we ship in our fork, see the patch below):

1. add `KeyboardType::Korean = 8`;
2. decode unknown values leniently instead of failing. In our fork we map them to
   `IbmEnhanced` (`KeyboardType::from_u32_lenient`) to keep the public enum shape.
   Upstream may prefer an `Other(u32)` / raw-value variant so the value survives a
   round trip. Happy to change the PR to that.

The `Input` capability set already decodes this field as `Option<KeyboardType>` and
does not fail, so only `ClientCoreData` needs the change.

Patch (against ironrdp-pdu 0.9.0): `vendor/ironrdp-pdu/warpgate.patch` in
https://github.com/dskim1979/warpgate/tree/moavi/0.29.2-ko, including unit tests
(`decodes_korean_keyboard_type`, `unknown_keyboard_type_does_not_fail`).

---

## 2. Warpgate — issue

**Title:** Native RDP: Korean Windows mstsc cannot connect (`invalid keyboardType`)

**Body:**

On Warpgate 0.29.2, Korean Windows `mstsc` connections to the native RDP listener
fail immediately. Warpgate logs a decode error from IronRDP:
`ClientCoreData decode error: invalid keyboardType`. The same users connect fine
with FreeRDP from Linux, and Windows/xrdp targets accept the same mstsc client directly.

Cause: Korean Windows sends `keyboardType = 8` (Korean, per Win32 `GetKeyboardType`),
which `ironrdp-pdu` 0.9.0 rejects (it only accepts 1..=7). Reported to IronRDP:
<link to the IronRDP issue/PR>.

Warpgate already vendors several IronRDP crates under `vendor/`. A minimal fix until
an IronRDP release accepts the value is to vendor `ironrdp-pdu` the same way, with a
`PATCHES.md` and `warpgate.patch`: https://github.com/dskim1979/warpgate/commit/<sha>
(adds `KeyboardType::Korean`, lenient decoding, unit tests in the vendored crate and
in `warpgate-protocol-rdp`). Glad to open a PR if you want it.

Reproduce: `xfreerdp /v:<warpgate>:3389 /sec:tls /kbd:0x412 /kbd-type:8`. On
0.29.2 the Warpgate log shows the `keyboardType` decode error. With the patch the
connection proceeds to authentication.
