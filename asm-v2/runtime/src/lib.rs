//! Runtime macros for `anchor-asm-v2`. Zero dependencies, `#![no_std]` safe.

#![no_std]

/// Emit `global_asm!` linking the combined assembly from
/// `anchor_asm_v2::build()`.
///
/// Call at crate root scope. Requires `#![feature(asm_experimental_arch)]`.
///
/// ```toml
/// # Cargo.toml
/// [dependencies]
/// anchor-asm-v2-runtime = { path = "..." }
///
/// [build-dependencies]
/// anchor-asm-v2 = { path = "..." }
/// ```
///
/// ```rust,ignore
/// // build.rs
/// fn main() { anchor_asm_v2::build("src/asm"); }
///
/// // lib.rs
/// #![no_std]
/// #![feature(asm_experimental_arch)]
/// anchor_asm_v2_runtime::include_asm!();
/// ```
#[macro_export]
macro_rules! include_asm {
    () => {
        include!(concat!(env!("OUT_DIR"), "/combined.rs"));
    };
}
