# In progress and claims

Shared notes between agents working on nxport. Claim an item before you start it (put your name / session on it),
and update this file when you finish or drop it.

## Claimed: S41L0R Windows Machine

- **Unlocked frame rate (TotK).** Bringing windows-support-2's three commits to main (geometry shaders, TotK 1.1.2
  dynamic FPS hooks, parity ports), then the hooks for the Windows test dump's version and a re-lifted guest. In progress.
- **Vulkan device loss on Great Sky Island.** Reproduces headless: Continue on a save there loses the device at
  about frame 2400 to 2650 (last checkpoint: end of a render pass); not with --gpu-checkpoints-each (timing). The
  scene also runs at 20 fps (record and GPU about 33 ms a frame). In progress.
- **Plugin mods on lifted hosts (UltraCam / exlaunch subsdk modules), and an unlocked frame rate on Windows.**
  Plan: two passes. Lift the game with the mod's modules, run it headless with the mod until its initialisers and
  early hooks have patched the code (the iOS dump run's machinery: cpu/aot.rs Mode::Dump, horizon::svc code writes),
  then lift that patched image. Test mod: "TOTK Optimizer" (UltraCam 2.7.1, exefs/subsdk3), which supports TotK
  1.1.0 - 1.2.1 only; the Windows test dump may be 1.0.0 (main build id 082CE09B06E33A123CB1E2770F5F9147), so a
  supported version may be needed. Not started beyond reading the code.
- **SDL controllers per player.** main's 8-player input (host/input.rs PLAYER_PADS) is wired for GameController.framework;
  on Windows every SDL controller still feeds player 0 (input::set_pad). Give each SDL gamepad its own player.

## Open (unclaimed)

- `windows-support-2` holds three commits not on main: 9b16122 geometry shaders (Vulkan, Metal, sass2spv),
  504f09b TotK 1.1.2 dynamic FPS / delta-time hooks, a582bb2 parity ports (AddressRegionManager, KTGL 1D wrap, Metal
  UBO invalidation). Bring them to main before deleting that branch. `retro-work` also has commits not on main.
- With `--no-vsync`, TotK starts no sounds (title runs ~600 fps; its idle voices are never routed to splitters).
- Rare Vulkan device loss at startup (about 1 in 10, empty fault report); lost-device reports and checkpoints are on.
- Shared pipeline list is per title, not per version: TotK 1.0.0 matches 9 of 1,825 stages of the 1.1.2 list.
- VP8 movies have no decoder (crates/media); everything else the old ffmpeg shim covered is built in.
- CPU side on lifted hosts: null-GPU gameplay ~69 fps; threads wait on each other's jobs (see the CPU notes in
  docs/windows-port.md, kept local). Tried without effect: timer resolution, thread priorities, restrict + noescape,
  spinning before parking.

## Done recently (S41L0R Windows Machine)

- Movies decoded in the program (crates/media: WebM/MP4, VP9, H.264, AAC, Vorbis, Opus); the ffmpeg shim, its build
  scripts and packaging are gone. Fixes the TotK crash when its first movie opened with no tracks.
- Windows short sleeps end on time (clock::sleep_ns yields for the last 0.7 ms).
- docs/ is local only (ignored).
