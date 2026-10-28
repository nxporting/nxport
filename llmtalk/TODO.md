# In progress and claims

Shared notes between agents working on nxport. Claim an item before you start it (put your name / session on it),
and update this file when you finish or drop it.

## Claimed: S41L0R Windows Machine

- **TotK 1.0.0 game time follows real time.** Uncapped, game speed scales with the frame rate (time step = present
  interval / 2 per frame, main+0x8f68b8). Replacing it with a step from real elapsed time, clamped. In progress.

## Open (unclaimed)

- **Dynamic FPS done properly, physics included** (later): the user will provide a newer TotK so we can study how
  UltraCam handles frame-rate-independent time and physics. UltraCam itself is not a goal once dynamic FPS is right.
  For now, delta time only makes game time follow real time (no physics verification).
- TotK 1.1.2 `dynamic_fps` (on by default, from windows-support-2) rewrites a time struct from real elapsed time at
  main+0x7f7a58 and forces the pacer; nobody has checked it against a 1.1.2 binary. In 1.0.0 the game steps its time by
  present interval / 2 itself (main+0x8f68b8), so the 1.1.2 counterpart of the 1.0.0 unlock (lowest interval 1 + no
  veto) may be all 1.1.2 needs: verify on a Mac before relying on dynamic_fps.
- Lifted TotK with `--no-vsync` runs faster than real time above 60 fps (the time step stays half a 30 fps frame).
- `retro-work` has commits not on main.
- With `--no-vsync`, TotK starts no sounds (title runs ~600 fps; its idle voices are never routed to splitters).
- Shared pipeline list is per title, not per version: TotK 1.0.0 matches 9 of 1,825 stages of the 1.1.2 list.
- VP8 movies have no decoder (crates/media); everything else the old ffmpeg shim covered is built in.
- CPU side on lifted hosts: null-GPU gameplay ~69 fps; threads wait on each other's jobs (see the CPU notes in
  docs/windows-port.md, kept local). Tried without effect: timer resolution, thread priorities, restrict + noescape,
  spinning before parking.
- **TotK freefall flickers**:
  - Terrain-detail squares under Link while skydiving: fixed by guarding storage-buffer results until their GPU work
    completed (uncommitted in ../nxport-totk, `metal/fence_guard.rs`); measure its cost on a quiet machine, then commit.
    Captures / save: `work/captures/totk_20261025_freefall*`, `work/saves/totk-freefall` (slot 01: press A, hold right stick down).
  - Second flicker while falling: a sky island vanishes for one frame while a white tile appears on the ground below.
    Captured (`work/captures/totk_20261025_freefall_fix`, frames before 2153 / 4233) but not located in the frames yet.
- **Breath of the Wild**:
  - Water: Link stands on the pond floor instead of swimming (`work/saves/botw-pool`).
  - One-frame black foliage (~130 per 600 frames at `work/saves/botw-sunk`); blue-grey haze over the image.
- **Uncommitted work in ../nxport-nxnative (reviewed 2026-10-25)**:
  - Guest image cache (`lib.rs`, `aot.rs`): returns cached image even when settings differ (mods, x18 mode) and ignores
    failed settings install. Key cache on settings and keep error.
  - Shared-font fallback (`nn.rs`, `ios_env.rs`, `ipa.rs`, cli `--shared-fonts`): iOS font paths and fonts folder in
    the app are right; drop cwd/Documents searches and embedded dummy_font.ttf; provide console shared font from
    system data like Mii data.
  - Pipeline-archive function dedupe (`metal/pipeline_archive.rs`): keep, after iOS build test.
- **Ultrawide support for more games and versions**:
  - Other TotK versions: find aspect and HUD calculate sites (layouts may match, offsets move).
  - Other games (Mario Kart 8 Deluxe first): aspect sites and HUD tables.

## Done recently

- **SDL controllers per player** (S41L0R Windows Machine): on Windows each connected gamepad is its own player, in
  connection order (the first with the keyboard as player 1); players without one show as disconnected. Keyboard input
  checked headless; not yet tried with several real controllers.

- **TotK title hitches fixed** (S41L0R Windows Machine, `47bcfe5`, `96c89ce`): render targets the CPU reads were
  copied into write-combined memory and retiled from it (90-600 ms on the watcher, which the frame wait waited for:
  ~128 ms at every background cut); they now go through a host-cached arena. The counter-scan cache freed ~32k
  recorded lists at once when it overflowed (~80 ms every few seconds); it keeps recent ones and frees the rest on
  another thread. Title uncapped: ~148 fps, no periodic hitches.

- **Intermittent Vulkan device loss fixed** (S41L0R Windows Machine, `426ec48`): vertex buffers bound in place from
  imported guest memory lost the device on NVIDIA (title ~45 s in uncapped, every run; Great Sky Island ~1 in 2;
  also the "rare loss at startup"). Bisected on the title: only --no-host-import and not importing that one 20 MB pool
  avoided it; vertex data now always comes from its shadow, the vertex cache or a per-submit copy (no measurable cost).
  Lost-device reports now also mark dispatches, copies, clears and feedback snapshots; --log-hitch names a slow
  frame's work.

- **Vulkan recording speed, first round** (S41L0R Windows Machine, `cc29ace`): redundant pipeline and descriptor set
  binds left out, the driver constant buffer copied only for the extent read and once per contents, vertex/index cache
  lookups once per submit, no per-draw allocations for stage keys. Capture sky2203 replayed (interleaved with the old
  build): recording 26.1 -> 21.3 ms a frame, every pass matching. Game, Great Sky Island headless: record 41-46 -> 28
  ms, 19 -> 30 fps (another game run shared the machine). What is left is spread over the per-draw path (stage_writes
  ~40%, driver ~15%, arena copies ~8%); a bigger step needs recording passes on several threads.

- **Plugin mods on lifted hosts** (S41L0R Windows Machine, `e1cd615`): two passes. `nxport-cli lift-game --mods
  DIR` lifts the game with the mods' modules; a lifted run with `--aot-dump=FILE` writes the code as the mods
  patched it at `--aot-dump-frame`; `lift-game --mods DIR --patched-image FILE` lifts that (function starts seeded
  across modules: hooks, trampolines, ADR/ADRP targets, DT_INIT). The builder does it all when the app's mods include
  exefs/subsdkN modules (`nxport-builder DUMP --build --mods DIR`, checked end to end). Still open: UltraCam itself
  on Windows needs a supported TotK version (2.7.1: 1.1.0 - 1.2.1). On the 1.0.0 test dump it patches 113 bytes,
  the patched lift runs into its hooks, and they fault (hooks at 1.1.x addresses).

- **TotK 1.0.0 frame rate unlocked** (S41L0R Windows Machine): lowest present interval 1 and no veto on drops
  (profile/totk/v1_0_0.rs); applies natively and to lifted images (re-lift to pick it up). Title menu 60 fps with
  vsync, ~80 without. Great Sky Island is held at 20-30 fps by Vulkan recording (~29 ms a frame), not pacing.
- **`windows-support-2` brought to main** (`3f46f2b`, `cf17023`, `76430c3`):
  - **Geometry shaders for Vulkan, Metal, and sass2spv** (`76430c3`): Maxwell geometry shader stage (SPH/hint 4) with
    dedicated `GEOMETRY_BINDING_BASE = 128`, integrated into Vulkan graphics pipeline layout, creation, async compilation,
    and MSL codegen.
  - **TotK 1.1.2 Native Dynamic FPS Core Hooks** (`cf17023`): Dynamic delta-time scaling in
    `profile/totk/v1_1_2/dynamic_fps.rs` (`hle_nn_os_GetSystemTick`), frame pacer patch at `main+0x008f6670`, and interval
    patch at `main+0x021bd514` for variable 60+ FPS without game speedups.
  - **Parity Ports from `nxnative-parity`** (`3f46f2b`): AddressRegionManager constructor patch (1 GB -> 62 MB reclaiming
    ~960 MB address space without scanning .text), KTGL 1D texture wrap restricted to integer fetches (TLD/TLDS), and Metal
    UBO copy invalidation via `GUEST_WRITES` atomic epoch increments.
- Movies decoded in the program (crates/media: WebM/MP4, VP9, H.264, AAC, Vorbis, Opus); the ffmpeg shim, its build
  scripts and packaging are gone. Fixes the TotK crash when its first movie opened with no tracks. (S41L0R)
- Windows short sleeps end on time (clock::sleep_ns yields for the last 0.7 ms). (S41L0R)
- docs/ is local only (ignored). (S41L0R)
