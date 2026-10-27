# Count Numbers — Native V2

Standalone Rust **library** for compilation into Tezbar. Command: `count-numbers`.
It preserves Count, Settings, Decrement Count, Reset Counter, Switch Counter,
Create Counter and Delete Counter using the beta Detail/Form renderer. The original
Raycast PNG is embedded unchanged; the count uses a self-contained SVG in markdown.
There is no executable, JavaScript runtime, worker or custom loader protocol.

## Build and test

Requires Rust 1.85 or newer. From this directory:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo +1.85.0 test --locked
cargo package --locked --allow-dirty
```

Tests compile the unmodified integration module against pinned **actual** beta
`common.rs` and `v2_sdk.rs`. Only Tauri AppHandle/Emitter are replaced, with a router
that rejects any OS request. SDK persistence uses separate temporary CorePaths for
each test, including fresh contexts, namespace isolation, corrupt files, rollback,
and concurrent atomic updates. They never use installed profiles or the clipboard.
The Cargo package contains all fixtures and tests; Node/Bun is not needed to test.
`tests/generate_v1_vectors.mjs` optionally regenerates checked-in oracle vectors
with Node from the surrounding original JSX updater bodies (no V1 edits or JSX
runtime). Source SHA256 is checked when that surrounding tree is available.

## Behavior

The namespace root contains a `data` model, initially:

```json
{"counters":[{"id":0,"name":"Counter","count":0,"increment":1,"modulo":1}],"currentCounter":0}
```

Counters persist across launches. Settings edits name, count, increment and modulo;
Create defaults to `Counter N`, `0`, `1`, `1`, and assigns max existing ID + 1.
Switch offers a dropdown of counters. Reset sets count to zero. Deleting the only
counter fails; otherwise selection becomes the item at min(deleted index, new
length − 1). Cancel and the form's Back action discard unsaved input. Enter submits
with beta's `actionId`/`formValues` payload and `kind: submit-form` action.

Settings/Create implement JavaScript `parseInt(text) || fallback`, including numeric
prefixes, signs, `0x` hexadecimal, decimal truncation, ECMAScript whitespace, invalid
text fallback, count fallback 0 and increment/modulo fallback 1. A zero step thus
becomes 1. Arithmetic uses binary64 addition and signed remainder only when modulo
is greater than 1; negative counts retain the JavaScript remainder sign. Numeric
display uses ECMAScript formatting. Finite fractional persisted values remain valid.
Nonfinite parsed values and arithmetic results explicitly fail instead of saving
JSON null. IDs must be unique nonnegative safe integers; exhausted IDs fail.

Each session captures its intended counter ID. Every mutation resolves that ID
inside an atomic update of the **latest** store. Another session changing the global
selection cannot redirect an increment, settings save, reset or delete. Switching,
creating and deleting update the global selection for future launches; an already
open session keeps its own selection until it switches or its counter disappears.
A missing intended counter produces a concrete error; Back or refreshing the Detail
view selects an existing counter. This deliberate concurrency improvement prevents
lost updates and wrong-counter mutations present in the original state snapshots.
Created counters also carry a persistent `nativeInstance` token, allocated by the
namespace root's `counterGeneration` sequence. A session captures that token so
deleting the largest ID and later reusing it via max + 1 cannot redirect stale
actions to the new counter. These reserved V2 metadata keys do not change the
original default model. Exhaustion or malformed instance metadata fails safely.
Actions carry a session revision and are checked against the current screen. Counter
IDs in action payloads are ignored; dropdown values must have been offered by that
session and must still exist at commit time.
Disposal retains only the revision, so reopening with reused state cannot revive
an action ID from the closed view.

Form heartbeats return `unchanged`, so typed drafts survive refresh and rejected
submits. A failed transaction does not navigate or report success. Detail heartbeats
read storage without writing. Missing `data` alone initializes the model inside a
transaction; null, partial, invalid or corrupt data fails and remains untouched.
Unknown namespace/model/counter keys survive edits. No automatic V1 migration is
claimed; V2 storage is isolated from legacy extension storage.

## Host integration (requires a host change and rebuild)

Reference API: Tezbar beta commit
`fd300e276356587726566026f072563eb9c34cb1`. See pinned fixture provenance and licenses
in `tests/fixtures`. Source V1 base is `653f0f226371efe49a621936985a0662efdac3ae`;
Raycast source/icon pin is `6734d85992290066fd31a7f5bee3cca3d3545a1a`.
Packaging follows the published Base64 V2 `f586814866962b21225cc6f870f585cc3787d224`
and Epoch to Timestamp V2 `fdc1bd9a599276d1535abd4da1f35d2daa049396` examples.

1. Vendor this directory in the host source and add a Cargo path dependency, e.g.
   `tezbar-count-numbers-v2 = { path = "vendor/count-numbers-v2" }` relative to the
   host Cargo.toml. The crate exports `commands()`, `ICON`, `MANIFEST` and async
   `handle(host, command, operation, payload, state) -> Result<Value, String>`.
2. Copy `tezbar_adapter.rs` into
   `src-tauri/src/native_core/extensions/v2_count_numbers.rs`; declare that module
   beside `v2_sdk`. It wraps CoreContext and calls the actual SDK
   `read_store(ctx, "count-numbers")` and `update_store(ctx, "count-numbers", change)`.
   The SDK's global STORE_LOCK serializes all contexts, reads the latest object,
   and writes only after the callback succeeds. The standalone crate never opens
   profile files or chooses filesystem paths. A custom Host must provide equivalent
   transaction/rollback guarantees and propagate errors, including `{ok:false}`
   responses with `message` or `error`.
3. Register the `count-numbers` slug, canonical ID
   `com.tezbar.native.count-numbers`, command list and handler dispatch in
   `v2_runtime.rs`; register this manifest in `v2_catalog.json` and any host compiled
   catalog validation. It replaces `raycast.count-numbers`. Do not mark compiled
   coverage as working before adding dispatch. The host manages session state and
   serializes each session's requests. Supported operations are
   `extension:run-command`, `extension:invoke-action`, `extension:refresh-session`
   and `extension:dispose-session`. Search is not used.
4. Copy the exact `assets/extension_icon.png` to
   **`src/renderer/public/extensions/icons/count-numbers.png`** (beta Vite's publicDir,
   not root `public`). Optionally copy it into the host's managed package assets as
   well. The public icon URL is `/extensions/icons/count-numbers.png`.
5. Rebuild the host, install/register its bundled Native V2 package, and perform
   real macOS and Windows UI QA. This repository is not directly Rust-installable
   from the current extension store; a manifest/marker cannot add executable code.

Both declared UI platforms remain **not-run** in the manifest until actual app QA.
Rust tests and cross-target checks are not renderer or desktop verification.
The beta schema does not allow a root `author`; credits are in the description.
V1 command/platform metadata and preferences are preserved; V1 files stay unchanged
apart from the root README's V2 link. The MIT port/icon notices are in LICENSE;
Apache-2.0 applies to the byte-for-byte Tezbar test fixtures, whose license is bundled.
