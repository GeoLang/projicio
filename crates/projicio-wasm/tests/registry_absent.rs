// The wasm build turns projicio-core's default features off, which leaves the
// embedded EPSG registry out. This checks the shipped artifact rather than the
// feature flags: whatever the flags say, the half megabyte of EPSG metadata
// must not be in the file browsers download.
//
// CI pairs this with a `cargo tree` check that the wasm target does not even
// resolve projicio-epsg-format, because the linker drops the blob while
// nothing in the bindings reads it.
//
// The test needs a built wasm file, so it runs only when PROJICIO_WASM_ARTIFACT
// names one by absolute path:
//   cargo build -p projicio-wasm --target wasm32-unknown-unknown --release
//   PROJICIO_WASM_ARTIFACT=$PWD/target/wasm32-unknown-unknown/release/projicio_wasm.wasm \
//     cargo test -p projicio-wasm --test registry_absent

use projicio_epsg_format::MAGIC;

const ARTIFACT_ENV: &str = "PROJICIO_WASM_ARTIFACT";
const WASM_PREAMBLE: &[u8] = b"\0asm";

#[test]
fn wasm_artifact_carries_no_epsg_registry() {
    let Ok(path) = std::env::var(ARTIFACT_ENV) else {
        eprintln!("skipped: set {ARTIFACT_ENV} to a built .wasm file");
        return;
    };

    let artifact =
        std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));

    assert!(
        artifact.starts_with(WASM_PREAMBLE),
        "{path} is not a wasm module"
    );
    assert!(
        !artifact
            .windows(MAGIC.len())
            .any(|window| window == MAGIC.as_slice()),
        "{path} contains the EPSG registry magic, so the blob was linked in"
    );
}
