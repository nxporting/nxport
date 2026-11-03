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
  Likely cause (2026-11-03, from SkyboundTOTK's notes, MIT, their fix taken from MacNativeGames): a slow-motion check
  (1.1.2 main+0xbbb4c0, 1.0.0 main+0xce3fec) keeps 0.1 of the ground body's velocity when the time object's +0x28,
  30 / (step * time scale), is over 100, which happens at over 100 fps at normal speed. Fix merged 2026-11-03
  (1bf994a): compares +0x28 * +0x38 (= 1 / (+0x20 * +0x44), checked in 1.1.2: f_70008f77b8 sets them) instead. Lifted
  and run headless (hook installs, scripted run normal); the user should still try a moving platform at 144 fps.
- **TotK hair and cloth too fast above 30 fps** (from SkyboundTOTK's notes): the actor animation update builds a clock
  {seconds = factor / 30, ratio = factor} each rendered frame, ignoring the real-time step. 1.0.0 sites: inline at
  main+0x7b5988/0x7b598c and main+0x7b6b28/0x7b6b2c (`fmul s0, s8, s0; stp s0, s8, [x29, #-0x40]`), main+0x7f5ec4
  (`fmul s1, s0, s1`) / main+0x7f5ed0 (`str s0, [sp, #0x114]`), and the call at main+0x7b6358 (s2, s3). Each should
  scale by the step (seconds = step seconds * factor, ratio = step * factor). 1.1.2 differs (2026-11-03, by
  disassembly): the same request struct ({1.0}, 0x100, then the clock pair, then -1) is built at main+0x69a198,
  0x9d188c and 0x9f3c50 (the last is the 1.0.0 callee's counterpart), but the clock pair is set to {-1.0, -1.0}
  (unset) and no 1/30 global is loaded; the callee no longer takes s2/s3. So 1.1.2 likely takes the clock from the time
  manager (which follows our step) and may not have the bug: check hair/cloth speed at 60+ fps on 1.1.2 before porting
  anything. 1.0.0 still needs the four patches.
- **TotK ambient occlusion / green glow flickers while the camera moves** (S41L0R Windows Machine, open): the 320x180
  pass s_6189816858e66cfd / s_fa6a730d8cf66342 (early in the frame) reprojects the previous frame's low-res lit scene
  with a grid of per-cell depths; still camera: right; moving: streaks, the reprojection is off by far more than a
  frame of motion (green blobs jump around in the final image). Its inputs: storage s0 (view data per slot, GPU-written
  by s_9723f00f727627e0 at frame start; per-cell depths by s_4af9460eb2550614 at the end of the previous frame), s1
  (matrix by s_b8c6271a88128a58 from s0 and cbuf c3) and cbuf c3. Ruled out: GPU latency (synchronous submits),
  storage read in place (copies), our frame-time hooks (30 fps, dynamic FPS off), the tile reduction's barrier (fixed in
  dc36147 anyway). A probe of guest memory at the draw (synchronous submits) found the slot's 3x4 matrix sane but
  the frustum corners it reads (slot + 288..384) and the per-cell depths (+832) look like garbage (2e-38 .. 8e8,
  -1.7e38); the probe's cbuf/storage slot mapping was partly wrong (c3 read zeros), so confirm with a better probe
  before trusting it, then look at s_9723f00f727627e0 (24 KB compute). Repro: headless scripted run on the
  shrine-entrance save, `2400-2650:37,2680-2760:40,2800-3000:37` after the A presses, capture at 3000.
  Second symptom, still camera (2026-11-02, user capture + headless): the AO along slab edges / wall bases switches
  between two states, a sharp one-signed step of ~3.6 levels over ~7,000 pixels at half res, every few frames
  (`2400-2650:37,2680-2760:40`, capture at 3300 with --frame-history=300: ~140 switches). Cause: the real-time step's
  fraction of a 30 fps frame at timer+8 (same as the ability wheel). Turning dynamic FPS off at frame 2900: 0 switches;
  only timer+8 forced to 1.0: 0; only timer+0xc forced: still flickers; synchronous submits, steady timestamps, render
  resolution: not it. A constant 0.2 step flickers far less (11) than a varying one (94). Dark and light frames issue
  the same commands, textures and screen-pass constants. The timer is the object at *(main+0x462ab60) (1.1.2); ~150
  functions read +8 directly. Moving the fraction into the time scale (+0x108) with +8 = 1.0 also stopped it, but the
  camera and UI read +8 too (ran fast). Next: find which of those readers drives the AO.
  2026-11-03: that experiment rules out readers that use +8 * +0x88 * +0x108 (the product did not change), e.g. the
  round-to-frames helpers f_7001a6ad90 / ae18 / aef4: the AO's reader takes +8 without the scale. Averaging the step
  over 5 frames (SkyboundTOTK) only trims it: 101 -> 80 switches. The old repro view drifted: on today's build use
  `-turn ",2400-2720:37,2740-2820:40"` (archway view, ~100 switches in 300 frames at 3300).
  2026-11-03 night (headless, archway view `-turn ",2400-2800:37,2820-2900:40"`, capture at 4200):
  - A hardware watchpoint on timer+8 (DR0 in every thread, VEH, guest.pdb via llvm-symbolizer) lists 27 code
    sites per frame. Most are the time manager recomputing channels (timer+0x10+0x28k = +8 * slot * +0x108, slots of
    {value, target} at +0x88 + 8i); a static scan misses them (cached timer pointer).
  - The physics world's step: the time manager (f_70008f6c8c, main+0x7f6c8c) passes +8 to f_70008f77b8, which sets
    the object [[main+0x462ac90]+0xc8] (the one ground carry reads): +0x38 = step, +0x24 = dt = min(+0x20 * +0x44 *
    step, cap), +0x28 = 1/dt.
  - Redirecting all 181 direct `ldr` reads of +8 to a constant whole frame: still flickers.
  - Making only those two physics loads (main+0x7f6da8, 0x7f72cc) read +0xc, with time_step writing the step in
    frames into +0xc: 0 / 0 / 2 switches over three runs, against 101 / 29 / 72 / 52 / 94 without (same view). +0xc as
    frames without the physics change: still flickers. A log shows physics receiving the same values either way.
  - CORRECTION, same night: identical runs vary from 0 to 101 switches per 300 frames (the plain build with the same
    settings gave 72, 0, 25), so the physics result above is not established, and the earlier single-run conclusions
    (+8 = 1.0 -> 0 switches, etc.) need repeating. time_step is called once per frame on one thread (no race).
    Measure over longer windows / several runs before concluding anything.
  - Long windows (900 frames, 3300..4200, two runs each, toggles from frame 2900 so the path is the same):
    baseline 322 / 67 switches; +8 = 1.0 and +0xc = 1/30: 0 / 0; capped to 30 fps from 2900 (real step ~0.99..1.006,
    game at real speed, the window covers 30 s of game time): 0 / 0; +8 = 1.0 with the fraction in +0x108 (derived
    channels unchanged): 10 / 199; only +0xc = 1/30: 28 / 62; only the physics dt whole (preset its time object so
    the update leaves it): 176 / 39. So: it needs steps under a frame, and the consumer reads a derived channel
    (+0 = +8 * slot0 * +0x108, or channels at +0x10 / +0x38 / +0x60), not raw +8, +0xc or the physics step. The
    switching usually starts late in the window.
  - Watchpoints on +0, +0x10, +0x38, +0x60: 83 code sites. Besides the time manager: a getter-like f_70014df480
    (30 reads of +0 a frame) and many small per-object accumulators (`obj.t += +0 * rate`, e.g. f_7000b089d0,
    f_70007b9bc0, f_70008b064c, f_700286dac0): animation / timer components. Theory: something visual picks its state
    per whole game frame of accumulated time (a pattern index or nearest-frame animation), so at ~140 fps it holds
    for ~5 frames then jumps; at 30 fps it changes every frame and the temporal filter averages it. Next: find which
    accumulator feeds a render pass (log the accumulators' owners, or compare the AO pass inputs on switch frames).
  - GPU side, next: `--history-targets` is Metal-only; on Vulkan a scratch patch (passk.patch) makes the pass dump
    cover K frames (every other frame), and dumpvar2.ps1 aligns passes by size / format / attachment. One try (10
    dumps after 4100) caught a quiet stretch: depth drifts smoothly, nothing switches. Needs a burst inside the dump
    window (bursts come late in the 3300..4200 range); ~840 MB per dumped frame.
  - Tools (scratch, not committed): watch_win.rs (DR0..DR3 watchpoints + VEH, symbolize with llvm-symbolizer
    against guest.pdb at ImageBase 0x10000000 + RVA), a private build root at C:\Users\Greenlord\nxport-build\r8
    (short path: the scratch path is too long for the compiler), ao2.ps1 (900-frame capture, count, delete).
- **TotK Ultrahand / ability wheel background is black** (fixed 2026-11-03 by the fallback below, bbebcba: headless hold-L run black on main, right on the fix; weapon menu (d-pad right) likewise): reproduced with an
  input script (hold L from frame 3300). The frame after the wheel opens draws no world: it clears the scene target,
  draws only the UI, then (conditional render) copies and blurs that target into the 768x432 / 960x540 background
  textures, so they hold black. The frame before samples the 768x432 texture uninitialized (garbage overlay).
  Cause narrowed (2026-11-02): the real-time game step (frame_time.rs time_step). With dynamic FPS off, or with the
  stock step even at 144 fps, the capture runs in the last frame that draws the world and the background is right.
  With steps under one game frame (0.5 at 60 fps, or 0.993 from jitter at a 30 fps cap) the capture comes a rendered
  frame later, after the world stopped. The game keeps interval 2 (30 fps) and calls time_step once a frame, so the
  wheel's capture seems to wait for a whole game frame of time while the world stops the next rendered frame. Next:
  find the pause/capture code that reads the time manager (timer+8 / +0xc) and give that transition a whole frame.
  Repro: headless, `3300-3700:14` (hold L) after the A presses, capture at 3600.
  SkyboundTOTK's fallback (nn_vi.c hos_frame_step_buttons, MIT): while L or d-pad left/up/right is held (at most 3 s)
  the step is 1/30 s and frames are held to 30 fps, and a new press reaches the game two frames late; the d-pad quick
  menus (weapons, shields, throw / fuse arrows) go black the same way, so check those too. Prefer finding the capture
  code; this is the fallback. Fallback on main (bbebcba): 30 fps and whole steps while a menu button is held; the
  real fix would drop that. Static lead for it: the sequence node class named
  `SeqOneShotDRequestCaptureBeforeShortCutBG` (name getter main+0x2814bd8, vtable main+0x439c320..0x439c430). They also average the real-time step over 5 frames, which may calm the AO flicker
  above (a steady step flickered 11 times against 94).
- `--log-drawables` (TotK) crashes about 2 s after its first report (frame 600): SIGBUS with PC = x8 = a guest stack
  address on ModuleSystemWorker2, i.e. the drawable probe's wrapper (it calls the original through blr x8) ran with x8
  clobbered. Seen 2026-10-28 on main (3 of 3 runs on the Hateno save). Cause found 2026-11-03 (by reading the code):
  HLE trampoline entries are 8 words, but the probe wrote its 15-word wrapper into one entry, overwriting the
  post-call stub in the next; after the draw call the game branched into that overwritten stub, which reloaded x8/x30
  from popped stack slots and ran `blr x8` again. The wrapper now takes two entries; the occlusion-read journal hook
  (words 10..14, also past its entry) now uses words 3..7. Untested on macOS: check that it reports past frame 600.
- `retro-work` has commits not on main: checked 2026-11-03, all are on main (same subjects; `git cherry` shows the
  lifter, guest::call and WIP commits as applied) except README 5af9a32's credit line "BelmanteGu - Referenced
  SuperMarioWonderRecomp as well." Up to the user whether to add it; then the branch can go.
- Shared pipeline list is per title, not per version: TotK 1.0.0 matches 9 of 1,825 stages of the 1.1.2 list.
  Players' lists carry the build id in their header and submissions send it, but the published list
  (`<title>.txt`, header `build -`) is merged across builds, so the client cannot filter it. Fix on the publishing
  side (nxporting/pipelines merge + submit worker, outside this repo): one list per build, fetched as
  `<title>_<build>.txt` (pipeline_list.rs FETCH_URL), with the title-only file as the fallback. Needs the user (repo
  and worker are theirs).
- VP8 movies have no decoder (crates/media); everything else the old ffmpeg shim covered is built in. Candidate crate
  (2026-11-03): `zvidlib-vp8` 0.5 (pure Rust, MIT, decoder + encoder, but new: Oct 2026, ~800 downloads); default
  feature `hardware` should be off. Needs the user's OK for the dependency and a game whose movies are VP8 to test.
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

- **Lifted code's CPU target measured, left native** (S41L0R Windows Machine): `zig cc -target x86_64-windows-gnu`
  (the x86-64-v3 flags, generic tuning) ran TotK 11% slower than the default `-mcpu=native` (shrine, headless,
  uncapped: 147 -> 131 fps steady), so apps stay built for the building machine. Objects now recompile when the
  compiler or its flags change (obj/flags.txt).

- **TotK shrine walkway wedge fixed** (S41L0R Windows Machine): the ground material samples three of its own render
  targets; the first one's feedback copy ended the pass, and the later two were then not seen as attachments and were
  read live while being rendered to (stale linear depth: the ground blended into black beside the walkway). Repro:
  headless scripted run on the shrine-entrance save, `2400-2650:37,2680-2760:40` after the A presses, capture at 2900.

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
