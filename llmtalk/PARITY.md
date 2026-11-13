# Parity across hosts, targets and backends

The user, 2026-11-12: parity "in every way between all platforms and host target pairs", and code quality and
organization. win-worker (the Windows session) and mac-worker (the Mac session) work this list together: claim an item
by putting your name on it, log what you find under it, and keep findings that are not about parity in TODO.md.

## The matrix

Builder host (rows) x app it makes (columns). Engine backends: Windows Vulkan; macOS Metal and Vulkan (MoltenVK);
iOS Metal.

|              | Windows app        | macOS app          | iOS .ipa                                       |
|--------------|--------------------|--------------------|------------------------------------------------|
| Windows host | yes (lifted, zig)  | **no**             | yes; shaders as Metal source (branch ios-shader-cache-windows) |
| Mac host     | **no**             | yes                | yes; shaders as compiled metallibs             |

## Rules (from bugs that broke parity)

- One code path per target, whatever the host; the host differences are small leaves (builder/src/ios.rs is the model:
  engine prebuilt or built from source, shaders compiled or packed as source, app in-process or in a worker).
- What shapes an AOT image follows `aot::native_rewrites()`, never `cfg(nx_native)`: an image made on Windows for iOS
  must hold exactly the patches the iOS engine makes at start (41d5940).
- Patched code holds no absolute address; per-run values go through aot's literal pool (41d5940, test e5e1e7d).
- Two checkouts never share a cargo target dir (stale build-script output; nx_native goes missing).

## Gaps

1. **Windows host cannot make the macOS app.** Pieces that exist: the source pack (the Mac engine reads it as iOS does:
   metal/shader_cache.rs source_set), adhoc_sign (Rust), the image crate for icons; missing: the macOS engine
   cross-built from Windows (as ios_engine.rs does for iOS, aarch64-apple-darwin), .icns written without iconutil
   (a simple container of PNGs), pack.rs off macOS. Unclaimed.
2. **Mac host cannot make the Windows app.** Missing: the lift on a Mac host (lift.rs needs the engine's nx_runtime
   loader there), zig cc targeting x86_64-windows from macOS, the Windows engine exe cross-built. Unclaimed.
3. **TotK exact mouse aim is Windows-only, but the Mac menu shows it.** camera_angles' install_decision and
   install_aim (camera-input test, bow aim) are compiled only into the lifted engine; native gets the yaw hook only, so
   the item does little on a Mac. They call into the engine from the middle of a guest function, which a lifted host
   call allows (guest registers kept); a native HLE trampoline saves only x29/x30, so natively they need a trampoline
   that keeps the caller-saved registers. Then they go into native images too (native_rewrites decides).
   Also mac-worker: 1.1.2 logs "the camera's angle code was not found" (the finder targets 1.0.0). Suggest mac-worker
   (needs a Mac run).
4. **The menu tree is declared twice** (host/display.rs for AppKit, host/menu_win.rs for Win32) though state and
   commands are shared (host/menu.rs). Already drifted: "Reset Controls to Defaults" vs "Reset to Defaults", Controls
   a window on Mac and a submenu on Windows. Fix: declare the tree once in menu.rs as data, both hosts walk it.
   win-worker (Win32 side runnable here, AppKit side type-checked; mac-worker runs it).
5. **The .ipa built on Windows has no compiled shaders** (Apple's Metal compiler is Mac-only): source pack plus the
   long-press precompile planned (TODO.md); Apple's Metal Developer Tools for Windows when installed. win-worker.
6. **The unified iOS step list has not run on a Mac** (1496107 on the branch): mac-worker asked to build an .ipa from it.

## Checks still to make (claim and fill in)

- Settings: every setting reachable from each host's UI (menus, Controls windows, iOS Settings.bundle).
- Input: controls window / bindings on each host; mouse look on Mac.
- Tools: frame history (F9), perf overlay (F10), capture flags, log locations.
- Mods: which kinds work per target (IPS/pchtxt, romfs, exlaunch plugins: not on iOS by decision).
