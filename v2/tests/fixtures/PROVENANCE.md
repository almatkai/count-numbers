# Pinned beta test fixtures

Tezbar commit `fd300e276356587726566026f072563eb9c34cb1`; copied byte-for-byte with `git show`.
Only the test Tauri AppHandle/Emitter are substituted; the real CoreContext and SDK are compiled.
The SDK module file is loaded with `#[path]` so its inner documentation remains intact.
Apache-2.0: see TEZBAR-LICENSE.txt.

- `beta_common.rs` ← `src-tauri/src/native_core/common.rs` SHA256 `280a0a2f0dd04a8570b320aafc0c3ece0f5de8ddb21c363645567d6a35d84da7`
- `beta_v2_sdk.rs` ← `src-tauri/src/native_core/extensions/v2_sdk.rs` SHA256 `4f987346f863db9a7bd6aa1d5a99e149925dc69a44d629727ccc80a567cd27c2`
- `extension-v2.schema.json` ← `schemas/extension-v2.schema.json` SHA256 `6d5afda1b652605982104bdf16d1547547b98dfd707a7b2969316406c49f32af`
- `TEZBAR-LICENSE.txt` ← `LICENSE` SHA256 `0ed6b1b2760294d4aaaa45be38e166fcc0168e1dd5190423c096fcbe1c94afbf`
