#![no_std]

/// The host invokes this export after validating the manifest and granting the
/// `gameRead` capability. It intentionally has no imports, so this sample
/// demonstrates the safe default: a WASM plugin cannot read files, access the
/// network, or start processes.
#[unsafe(no_mangle)]
pub extern "C" fn sabaki_activate() -> i32 {
    0
}
