# Vendored SDL3 (Windows x86_64, dynamic)

Pre-built binaries from the official SDL3 3.4.18 VC release:
https://github.com/libsdl-org/SDL/releases/tag/release-3.4.18

## Contents

```
vendor/SDL3/
├─ lib/
│   ├─ SDL3.dll   ← copied next to nxport.exe at build time
│   ├─ SDL3.lib   ← import library, linked at compile time
│   └─ SDL3.pdb   ← symbols (optional, for debugging)
└─ include/SDL3/  ← headers (not used by the Rust build directly)
```

## How it works

`crates/engine/build.rs` emits `cargo:rustc-link-search=native=vendor/SDL3/lib`
so the linker picks up `SDL3.lib`. It also copies `SDL3.dll` into the
`target/<profile>/` directory alongside `nxport.exe` at build time.

No CMake, no vcpkg, no external installs required.
