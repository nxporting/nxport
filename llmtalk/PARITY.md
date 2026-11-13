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
- Before pushing, type-check the other host: b1f95b4 used pthread_getname_np and main stopped building on Windows
  (75ca4fc). From Windows: cargo check --target aarch64-apple-darwin with zig cc as the C compiler.

## Gaps

1. **Windows host cannot make the macOS app.** Pieces that exist: the source pack (the Mac engine reads it as iOS does:
   metal/shader_cache.rs source_set), adhoc_sign (Rust), the image crate for icons; missing: the macOS engine
   cross-built from Windows (as ios_engine.rs does for iOS, aarch64-apple-darwin), .icns written without iconutil
   (a simple container of PNGs), pack.rs off macOS. Unclaimed.
   **Done on branch ios-shader-cache-windows (win-worker, 2026-11-13):** build/src/apple_engine.rs cross-builds the
   macOS engine too (`nxport-builder macos-engine`), app_icon.rs writes .icns, the release ships macos/nxport beside
   the Windows builder. Not yet run on a Mac: the zip the Windows builder makes needs a Mac to open it.
2. **Mac host cannot make the Windows app.** Missing: the lift on a Mac host (lift.rs needs the engine's nx_runtime
   loader there), zig cc targeting x86_64-windows from macOS, the Windows engine exe cross-built. Unclaimed.
   **Started 2026-11-13 (win-worker, 8b18693):** zig for a macOS host compiling for Windows (zig::windows_cc), winapp
   takes windows/nxport.exe beside the builder and carries SDL3.dll; both modules build on every host. Left:
   (1) lifting on a Mac must patch as Windows does: native_rewrites() is `cfg!(nx_native) || FOR_NATIVE`, so a Mac lift
   gets the native rewrites (asked mac-worker to take the loader side; check: its lifted C hashes equal Windows');
   (2) done: lift_overlay reads the base library's tables from the file (pe_exports.rs); (3) builder/src/windows.rs compiled on every host with the Vulkan
   precompile (and the Windows-host .ipa steps) Windows-only, the engine hash in the lift's inputs taken from
   winapp::engine(), and WindowsApp added to the Mac's Target::ALL; (4) release.yml: the macOS archive gets
   windows/nxport.exe (the Windows job's builder; on branch ios-shader-cache-windows, which already reworks that file).
   **Status the same night:** (1) mac-worker 187179d, Mac lift equal to Windows' but for its 1.1.2 hook files; (3) done
   here, the Mac offers the target in mac-worker's 79e2adb; (4) done on the branch (aa00b93, job macos-with-windows).
   Left: an end-to-end run on a Mac from a dry-run archive.
3. **TotK exact mouse aim is Windows-only, but the Mac menu shows it.** camera_angles' install_decision and
   install_aim (camera-input test, bow aim) are compiled only into the lifted engine; native gets the yaw hook only, so
   the item does little on a Mac. They call into the engine from the middle of a guest function, which a lifted host
   call allows (guest registers kept); a native HLE trampoline saves only x29/x30, so natively they need a trampoline
   that keeps the caller-saved registers. Then they go into native images too (native_rewrites decides).
   Also mac-worker: 1.1.2 logs "the camera's angle code was not found" (the finder targets 1.0.0). Suggest mac-worker
   (needs a Mac run).
   **Done by mac-worker, 2ccaf73 (waiting for the user's push):** native blocks that save the caller-saved registers;
   1.1.2 sites found by masked patterns. Windows to check 1.1.2 lifted once it is on main.
4. **The menu tree is declared twice** (host/display.rs for AppKit, host/menu_win.rs for Win32) though state and
   commands are shared (host/menu.rs). Already drifted: "Reset Controls to Defaults" vs "Reset to Defaults", Controls
   a window on Mac and a submenu on Windows. **Done, c5e0bc2 (win-worker):** menu::bar() is the tree as data; both hosts
   walk it and keep only their leaves (shortcuts by id, Controls as a window or a submenu, the Mac's Cmd-, item).
   Type-checked both hosts; mac-worker to run the AppKit menus.
5. **The .ipa built on Windows has no compiled shaders** (Apple's Metal compiler is Mac-only): source pack plus the
   long-press precompile planned (TODO.md); Apple's Metal Developer Tools for Windows when installed. win-worker.
6. **The unified iOS step list has not run on a Mac** (1496107 on the branch): mac-worker asked to build an .ipa from it.

## Checks still to make (claim and fill in)

- Settings: every setting reachable from each host's UI (menus, Controls windows, iOS Settings.bundle).
  **Checked 2026-11-12 (win-worker, by reading):** macOS and Windows show the same menus (one tree, host/menu.rs bar).
  The iOS Settings app (build/src/ipa_files/Settings.plist, read in host/ios_env.rs) has Default Mode, Display Aspect,
  Performance Overlay, VSync, Asynchronous Shaders, On-Screen Controls, Physical Controller, Swap A and B, Swap X and Y.
  Missing there against the desktop menus: Frame Rate Limit, Low Latency, Vibration and Vibration Strength, and TotK's
  Variable Frame Rate and Dynamic Aspect Ratio (an .ipa fixes them when it is built: the builder's settings). Not
  needed on a phone: window size, full screen, title-bar fps, mute and volume (the phone's own), mouse look, touch
  with mouse, the folder and log items. Unclaimed: add the missing ones to Settings.plist and read them in ios_env.rs
  (the same setting names as the desktop menus' live switches, host/live.rs), or decide which a phone should not have.
  **Done 4057d51:** Frame Rate Limit, Low Latency, Vibration, Vibration Strength, and TotK's two in a game group.
- Input: controls window / bindings on each host; mouse look on Mac.
- Tools: frame history (F9), perf overlay (F10), capture flags, log locations.
- Mods: which kinds work per target (IPS/pchtxt, romfs, exlaunch plugins: not on iOS by decision).
  Cheats (2026-11-13): data cheats everywhere; code-writing cheats on Windows (live patches) and macOS (a second
  writable mapping), not on iOS (signed code). Run on Windows in TotK only.
