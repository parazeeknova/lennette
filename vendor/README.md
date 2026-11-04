# Vendored patches

Only dependencies requiring local compatibility fixes belong here. Keep their
source, licenses, and test fixtures; omit registry cache markers and duplicate
unpatched manifests. Remove each patch when its upstream fix is adopted.

## `redis-protocol` 6.0.0

Fred 10.1 depends on this crate, which pins `cookie-factory` to `=0.3.2`.
The desktop's PipeWire dependency requires `^0.3.3`, preventing workspace
resolution. The only source change is allowing the compatible 0.3.3 patch
release in Cargo.toml. The crate also allows the new lifetime-syntax warning
emitted by the current compiler for its older public type aliases. Keep this
patch until upstream relaxes the pin.

## `xattr` 0.2.3

`xattr` 0.2.3 referenced `libc::ENOATTR`, which newer `libc` releases no longer
expose on Linux (`ENODATA` is the correct value there). The only change is that
constant's definition in `src/sys/mod.rs`.

## `proc-macro-error2` 2.0.1

`proc-macro-error2` 2.0.1 triggered a future incompatibility warning (`E0365: extern crate proc_macro is private and cannot be re-exported`) on newer Rust compilers because `extern crate proc_macro;` on line 277 was private while line 494 did `pub use proc_macro;`. Marked as `pub extern crate proc_macro;` in `src/lib.rs`.
