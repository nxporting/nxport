# In progress and claims

Shared notes between agents working on nxport. Claim an item before you start it (put your name / session on it),
and update this file when you finish or drop it.

## Claimed: S41L0R Windows Machine


## Open (unclaimed)

- **Dynamic FPS done properly, physics included** (later): the user will provide a newer TotK so we can study how
  UltraCam handles frame-rate-independent time and physics. UltraCam itself is not a goal once dynamic FPS is right.
  For now, delta time only makes game time follow real time (no physics verification).
- **TotK at high fps, still wrong** (reported by the user): physics on a platform moving down a rail. Not looked at
  yet; needs gameplay to reproduce. (Ladders: fixed as of 2026-11-02, user-checked; the Ultrahand camera too.)
- **TotK shrine walkway: dark wedge beside the walkway** (S41L0R Windows Machine, open): the ground material
  (s_40b797f18632f7bc / s_6facd1ab0508d563) blends into what is under it at its screen position (normals, albedo:
  feedback copies) by a weight from the linear depth of pass "s_0b38d2c1131e4c9b" (R32, sampled as tcb_10). Before
  the render-target feedback fix the blend read stale copies (black outline around the walkway's base); with it a
  wedge of the base goes dark. Where it goes dark nothing is under the ground yet, so the blend weight there (the R32
  value for empty pixels) is the next suspect; the pass dump clamps R32 to 8 bits, so read it raw.
  Repro: scripted run on the shrine-entrance save, `2400-2650:37,2680-2760:40` after the A presses, capture at 2900.
- **TotK Ultrahand / ability wheel background is black** (S41L0R Windows Machine looked, not fixed): reproduced with an
  input script (hold L from frame 3300). The frame after the wheel opens draws no world: it clears the scene target,
  draws only the UI, then (conditional render) copies and blurs that target into the 768x432 / 960x540 background
  textures, so they hold black. The frame before samples the 768x432 texture uninitialized (garbage overlay). Cause
  not found: why the game copies after the world stopped being drawn (ordering, aliasing or predicate timing).
- `--log-drawables` (TotK) crashes about 2 s after its first report (frame 600): SIGBUS with PC = x8 = a guest stack
  address on ModuleSystemWorker2, i.e. the drawable probe's wrapper (it calls the original through blr x8) ran with x8
  clobbered. Seen 2026-10-28 on main (3 of 3 runs on the Hateno save).
- `retro-work` has commits not on main.
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

- **TotK fixes, 2026-11-02** (S41L0R Windows Machine, `f2063da`..`75b0aa1` and after): black fringe on the shrine
  floor's rim (draws sampling their own targets now see the draws before them; F9 also dumps every pass); stutter
  with VSync (steps rounded to whole refreshes, remainder carried); printf HLE ("has no adapter" gone); the camera-rate
  getter scaled to the frame's step (Ultrahand camera; 1.1.2 main+0x1d3675c, 1.0.0 main+0x1cd3678, user to verify);
  listed point pipelines built as points. 1.1.2's frame-rate hooks are the frame_time.rs sites (the old
  windows-support-2 `dynamic_fps` time-struct rewrite is gone; the setting now switches real-time steps).

- **Vulkan recording, fourth round: work moved to vk-record** (S41L0R Windows Machine, `361a5ad`, `e3b25c9`): uniform
  copies are made on vk-record (inline updates and reports log what they overwrite so late copies hold what their draws
  saw; the replay differs without it), and descriptor sets are hashed, found, allocated and written there. sky2203
  replay: whole frame 13.4 -> 11.0 ms, recording 10.2 -> 8.7 ms. **In the game nothing changed**: Great Sky Island
  headless, --no-vsync, matrix.ps1 input script: 51.3 -> 51.2 fps (record 17.4 -> 16.7 ms), and --gpu=null gives only
  57.1 fps. The backend costs ~2 ms of the 19.5 ms frame there; the Presentation thread idles ~20% in
  ReceiveLightMessageQueue and guest-main waits ~38% in WaitLightEvent. More fps needs the game's CPU side (threads
  waiting on each other's jobs; see the CPU item under Open). In-game recording also has costs the replay lacks:
  texture refresh checks (get_desc ~10% of the Presentation thread), scan_counters ~8%, images created every frame
  (~5%: Textures::create, Dev::alloc).

- **Vulkan recording, third round** (S41L0R Windows Machine, `6a4a870`, `d3ba8b7`): uniform bindings resolved once
  per contents, driver constant buffer rebuilt only when it changes, draw states found by content across submits,
  unchanged vertex / index buffers and dynamic state not bound again. sky2203 replay, idle machine: recording 13.1 ->
  10.1 ms, whole frame 16.7 -> 13.6 ms; sky2203 and title5950 match. Not yet measured in the game (the cross-submit
  draw-state map is at its best in a replay of one frame). What is left is spread thin (~1.4 us a draw, 7,300 draws,
  no hotspot; array slots instead of the (stage, index) maps measured no faster). The "mpmc recv" samples on the
  Presentation thread are wait_ended (identical-code folding), ~5%: vk-record ending command buffers.
  Next: per-draw preparation over several threads.

- **Vulkan recording, second round** (S41L0R Windows Machine): driver calls run on their own thread (vk-record), and
  chunks are queued without waiting for it. The bound pipeline and stages are reused until pipeline state really
  changes. Small vertex ranges are hashed before the submit on helper threads, not draw by draw. Texture staging
  buffers are pooled. On discrete GPUs, write-backs read host-cached memory: shadow slots had been read from the BAR
  window, 173 KB in 12 ms a frame on the watcher. Idle machine, sky2203 replay: whole frame 35.3 -> 16.7 ms, recording
  18.3 -> 13.0 ms, GPU 21.6 -> 9.4 ms. Most of the "pass 21 stall" was the GPU waiting for chunks. Every pass matches;
  title5950 matches too. Game, Great Sky Island headless with --no-vsync, same input script: 36 fps / record 26 ms
  -> 46-48 fps / record 20.4 ms. 1000 s run without hang or device loss.
  Next: the Presentation thread is still saturated. stage_writes is about 40% of it. 100+ fps needs the per-draw
  preparation split over threads.

- **Sky Island hang fixed** (S41L0R Windows Machine, `bdd0211`): the watcher's write-back spun forever on a page a
  reprotect race left read-only. **UltraCam runs on TotK 1.1.2** (`b440ac1`: plugin-mod trampolines are lifted); its
  frame-rate design (target FPS + own limiter, smoothed delta time, menus at 60) is in local docs/ultracam-totk-112.md.

- **No sound with VSync off fixed** (S41L0R Windows Machine, `cd68d55`): above ~200 fps (the audio renderer's frame
  rate) TotK starts no sounds at all; VSync off is now paced to at most 144 fps (sound checked through gameplay).

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

- **TotK 1.0.0 game time follows real time** (S41L0R Windows Machine): the time manager's step (main+0x7a83e4) and
  the controllers' hold-timer step come from real elapsed time (1/240 .. 2/30 s), in every fps mode. Lifted title menu
  at 30 / 60 / ~140 fps shows the same shots at the same wall times. Lifted builds need a re-lift to pick it up.
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
