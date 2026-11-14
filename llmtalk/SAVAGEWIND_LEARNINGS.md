# What nxport can learn from Savage Wind Recomp (BotW)

Study of Savage Wind Recomp (github.com/Revivedx/SavageWindRecomp, 0.5.3-alpha.5, 2026-11-12; MIT) for the user,
2026-11-14, by a win-worker subagent. It reads the source cloned at `nxport-build/ref/savagewind` (shallow: no
history). Nothing was built or run. Savage Wind is a static recompiler of BotW (Switch) to C from LAS Recomp, with a
Vulkan backend (Metal for macOS in `renderer.m`) and its own Maxwell translator. "Code" below means read in the source;
"docs" means only the docs say it. Its addresses are absolute with main at 0x7000000000: main offset = address -
0x7000000000.

## 1. Versions

- **Supported: 1.9.0 only** (main build id prefix `cd57b23fa4bbad65`, SDK 21.4.1; `tools/game_sites.py VERSIONS`,
  `runtime/game_versions.h`). 1.8.2 ran until 0.5.1; setup now refuses it. It has never run 1.5.0.
- **How it finds addresses** (code, `tools/game_sites.py`): about 170 sites, each a short pattern of instruction
  words with the fields that move masked (branch offsets, ADRP pages, the low 12 bits of the ADD/LDR/STR after an
  ADRP). Kinds: `code`, `call` (BL target), `ref` (ADRP+ADD address), `got`, `imm` (a field offset a version moved:
  1.9.0's UI screens are 0x18 bytes bigger from about +0x290), `data`, `end`. A site may list several patterns.
  `KNOWN['1.9.0']` holds every address: there a pattern must find exactly that place (self-check), and falls back to
  it with a warning. Optional groups (`OPTIONAL_GROUPS`) drop a whole feature when one of its sites is missing.
  Every patch in `tools/patches.py` also asserts the exact instruction word it replaces.
- **Version-tolerant, not version-independent.** Going from 1.8.2 to 1.9.0, 29 of the 36 sites of the time matched
  as they were; 7 needed new common patterns (`game_sites.py common`). Struct offsets (VFR fields, ScreenMgr +0x28 /
  +0x30, active event +0x1d2b8, PlayerInfo +0x88 / +0xa4) and screen ids are checked by hand per version (docs,
  SAVAGE-WIND.md "Versiones del juego").
- **Their 1.9.0 table is a free cross-check for us.** For example (main offsets): frame wait +0xe9487c, VFR ratio
  load +0x151d494, GUI speed store +0x1280ee8, arrow gravity +0x5ee734, sound floor +0x1551004, solver steps
  +0x156f930, Havok world tick +0xf5e20, perspective matrix +0xeb4a50, projection dirty flag +0xeb4474, camera stick
  input +0xd57fe0, LookAtCamera update +0xeb2d5c, traverse scale +0x10a4674, Yaz0 stream +0x131d674, pause state
  +0x1e1611c, event manager +0x1e3a638, controller step +0x1dad0e0, PlayerInfo +0x1e13ed0.
- **What nxport needs for 1.9.0:** the full main build id from the user's own files (Savage Wind lists only 8
  bytes); an SDK 21.4.1 layer in `horizon/sdk` (our newest is v15_3; BotW 1.5.0 uses v4_4). Savage Wind avoided most
  of that cost: it recompiles the SDK and runs the game's own AudioRenderer, and HLEs only ~80 SDK functions by name
  (`HLE_FUNC` in `runtime/hle/*.c`); nxport replaces SDK functions by symbol, so a new SDK means checking thread
  object layout, NVN API version and changed signatures. Size: unknown until the first boot; plan for several
  build/run cycles.
- **1.5.0:** keep the profile booting as it is, but put every new fix on 1.9.0 first. 1.9.0 is what an updated
  game is, and Savage Wind's addresses and bug reports all refer to it. Porting each fix to 1.5.0 costs a site search
  per fix (SDK 4.4 era code: expect more pattern misses than 1.8.2 -> 1.9.0 had). Worth it only if users ask.

## 2. Game fixes and patches

Kind: **site** = one instruction replaced by C at a pattern site (`tools/patches.py`), runtime code in
`runtime/hle/framepace.c` unless named. **Exact?** uses our rule: Yes (the game's own rule or exact state), Derived
(a rule worked out from the game's code, no tuned numbers, but not Nintendo's), No (tuned or guessed).
nxport today for BotW: nothing (`profile/botw.rs` has no hooks); the TotK column says what we did there.

### 2a. Frame rate and pacing (all site patches, code-verified)

| Fix | What it does | Where | Exact? | nxport |
|---|---|---|---|---|
| Frame loop vsync wait | Skipped; the present paces instead | `_frame_wait`, `rt_frame_wait` | Yes | TotK: pacer lifted (frame_time.rs) |
| Game clock | `ksys::VFR::updateInterval`: after the game sets its interval ratio, the runtime writes ratio = step x 30 (0.5 at 60 fps) and the interval 1 / 2 / 3 (the game's 60 fps branches above ~43 fps) | `_vfr_ratio`, `rt_vfr_update` | No as written: step smoothed (5% average, 3%/frame clamp, 2% debt payback), interval by hysteresis thresholds. The hook point is exact | TotK: our own real-time step |
| Menus' speed | GUI speed (UI object +0xb20) stored as interval x 0.5, floored at 0.5: menus ran fast above 60 fps; becomes the frame's length | `_gui_speed`, `rt_gui_speed` | Yes (Cemu FPS++'s idea) | - |
| Button hold counters | `sead::ControllerBase` steps hold counts by a float the game keeps at 1; set to the frame's length in console frames each present | `framepace_presented` writes `controller_step` | Yes | - |
| Arrows (#144) | Fall gravity added once per update with no time step: arrows fell 2-4x as fast. Scaled by the frame ratio | `_arrow_gravity`, `rt_arrow_gravity` | Yes to first order | - |
| Battle music (#145) | Sound frame = max(delta frame, 0.5): sound clock ran fast above 60 fps, segments overlapped. Floor becomes 0.5 x ratio | `_snd_frame_floor` | Yes | - |
| Havok solver microsteps (#161 cucco jump, #349 Magnesis doors) | Game asks round(df x 6). Runtime: 6 / sqrt(df) for df >= 0.4, else 1 | `_solver_steps`, `rt_solver_steps` | **No**: fitted to jump heights and door tries | - |
| Keyframed platforms (#209, Vah Medoh floor) | Turn angle 2 acos(\|w\|) rounds turns under ~1 mrad to 0 at high fps; uses 2 atan2(\|xyz\|, \|w\|) | `_kf_angle`, `rt_kf_angle` | Yes mathematically; also changes 30 fps results slightly | - |
| Bodies falling asleep (#398, Ree Dahee ball) | Havok deactivation counts steps, one per frame: islands slept 12x sooner at 360 fps. Only steps that complete a console frame count; solver damping m^r, sleep speed x r | `_phys_tick` (world, single and multi-threaded), `_phys_motion_tick` (two integrators), `rt_phys_tick` | Derived (counts exact; damping scaling a model) | - |
| Scripted walks stop (#364, leaving the Shrine of Resurrection) | Arrival test `distance < speed per console frame`; scaled by delta frame (the game's other end test already is) | `_demo_move_arrive` | Derived | - |
| Shield-surf jump (#305) | One console frame of grace for the takeoff boost below ratio 0.30 | `_surf_contact`, `_surf_boost_remove`, `_surf_jump_request` | **No**; their comment says unvalidated | - |
| Quick menu black flash (#173) | Alpha fix applied when frame > 0 instead of == 1 (fractional GUI frames skip 1) | `_quick_menu_capture_alpha` | Derived | - |
| Master Kohga's entrance freezes (#196) | Some events held at 30 fps: `Electric_Relic` within 150 m of (-3481, 1315), `Demo347_0`. Cause found: `Demo_WaitForASTriggerEvent` waits for an animation trigger that exists only in the update that crosses its frame | `events_at_30` | **No**: a name and place list | - |
| Menus, cutscenes at any rate; movies at their own speed | Default runs events at the chosen rate; `SAVAGEWIND_EVENT_FPS` / `MENU_FPS` hold them | `framepace.c`, `hle/movie.c` | - | TotK: yes |

Still broken there above 30 fps (docs, FAQ): #425 is their catch-all; Game Over text half revealed at 70-120 fps
(#482), cannot dismount the horse at 120 (#625), some cutscenes end early (frame-counted waits).

### 2b. Screen, camera, draw distance

| Fix | What it does | Where | Exact? | nxport |
|---|---|---|---|---|
| Ultrawide and taller screens | sead `PerspectiveProjection` matrix build: tan(fovy/2) and aspect widened to the window (Vert+ for taller); frustum loads; projection dirty flag forced; screen-to-camera; world camera written back so the shadow cascades fit the drawn view (#103); 16:9 render targets widened | `_persp_matrix`, `_frustum_load`, `_proj_update`, `_persp_unproject_*`, `_persp_matrix_offset`; `nvn/output_scale.c rt_persp`; `nvn/vulkan/textures.c` | Partly: views picked by value (aspect 16:9 +-0.01, world camera = far plane > 10001); menus, map, loading kept 16:9 by screen id (exact) | TotK aspect.rs hooks the same sead class (setFovy_): portable with BotW sites |
| HUD size, HUD at edges | Panes anchored by which third of the screen they sit in | `nvn/hud_scale.h`, `exec.c hud_patch` | **No** (position zones) | TotK hud.rs: per-layout tables (keep that way) |
| Field of view | World camera's tan scaled 70-150%; off while an event runs (event manager +0x1d2b8), eased 0.25 s; fovy, sin, cos written back (#162, #203) | `rt_persp`, `output_scale.c` | Event test exact; camera pick by far plane | TotK: none |
| Draw distance | Placed actors' traverse scale (the debug manager's +0x310, or 1); grass view range, fades, 512 blade slots and caps; distant trees' ranges | `_traverse_scale*`, `_grass_*`, `_tree_ranges`; `runtime/draw_distance.c` | Sites exact; factors tuned in play tests (1.6x the most that kept every actor; 2.5x+ crashed while loading) | - |
| Shadow resolution | No game patch: square depth targets >= 512 made at 2048-8192; the filter's texel size (1/896) rewritten in constant buffers found by value | `textures.c tex_scale_group`, `exec.c sdw_patch` | **No** (shape and value matching) | - |
| Haze | Climate scattering values (CalcMie, CalcRayleigh, CalcSfParamNear) scaled in the RomFS file as it is read | `services/fs.c haze_patch` | A data option | - |
| Effects off (bloom, depth of field, dynamic resolution) | By draw in exec.c | docs only | Unknown | - |

### 2c. Input

| Fix | Where | Exact? | nxport |
|---|---|---|---|
| Mouse camera: answers the one function every camera reads the right stick through (dead zone, v\|v\|, unit circle), linear and unclamped; skips the chase cameras' extra y\|y\|; aim cameras get the horizontal scale; game's inversion options followed | `_cam_stick`, `_cam_pitch`, `_cam_pitch_aim`, `_cam_reverse`; `hle/input.c` | Yes | BotW: stick emulation; TotK: exact |
| Controller camera speed past full tilt | `_cam_stick_end` | Yes | - |
| Gyro: six-axis HLE from SDL sensors (Mahony fusion, 200 Hz samples); without a gyro, T + mouse tilts; keys and drag in a shrine apparatus | `hle/motion.c`, `_gyro_scene_actor`, `pc_gyro_scene.c` | Host-side filtering; game side exact | **Missing**: `hle_nn_hid_GetSixAxisSensorStates` returns zero states, which BotW takes as "no data": tilt shrines and motion aim do not work |
| Mouse bow aiming: right button draws, left fires (ZR released 140 ms) | `pc_bow.h` | Input timing only | TotK: exact bow aim |
| Per-action bindings: the game's isHold / isTrig over a 41-action mask table | `_act_query`, `hle/bindings.c` | Yes | - |
| Mouse pointer in menus, map, shops, Divine Beast map | `pc_cursor*`, `pc_map_focus*`, `beast_*` sites, `pc_cursor.c` | Mostly exact, large | - |
| Radial stick dead zone: the game's menus mute a stick until it is within 1% of centre | `hle/input.c` | Yes (the console's input service has one) | Check ours |

### 2d. Crashes, completion, memory

| Fix | Where | Exact? | nxport |
|---|---|---|---|
| Stale actor in the target manager (#391, #393: Magnesis and camera targets): an actor outside the guest window or with a null +0x570 is treated as none | `_stale_actor_helper`, `_stale_actor_target` | **No**: a guard, root cause not found | Check whether it happens first |
| Gerudo desert model loader (#524): each model's RSTB entry +64 KB (the loader's temporary heap overflowed one load in five) | `services/fs.c rstb_model_grow` | **No**: margin tuned; why the console fits is unknown | Check first |
| DLC merged into the RomFS: RSTB entries grown to their Aoc/0010 entries (else a white screen after "Open your eyes") | `fs.c romfs_has_dlc_merged` | Yes | Check our DLC path |
| Voices in another language (#2, #169): voice bank RSTB grown, subtitle markers retimed | `fs.c voice_redirect` | Data feature | - |
| Unit heaps x4 (for mods such as bushsanity), failed allocations logged | `hle/heaps.c` | Not needed for the plain game | - |
| Loading screen watchdog dumps threads at 90 s (#438) | `framepace.c load_watch` | Diagnostics | - |

The game was finished on Savage Wind (all Divine Beasts, Ganon, both DLC) with these; the frame-rate items above are
what it took above 30 fps.

## 3. Engine-level things they needed for BotW

| Item | Where | nxport |
|---|---|---|
| **nvnTextureCompare compares state, not addresses.** Cuttable trees keep a texture pool slot when the rebuilt texture compares equal; by address each rebuild freed the slot and the trunks bound whatever took it (#16: trees "thrown across the sky") | `nvn/objects.c` | **Gap**: `hle_nvnTextureCompare` (nvn/api.rs) compares pointers |
| **Linear render targets go back to guest memory.** The sky is sampled into an 8x8 RGBA32F linear target and read by the CPU for **fog and ambient colours**; 1x1 / 3x3 for exposure. The first ~30 tiny reads after a loading screen are waited for, or the water stayed milky white (#362) | `nvn/vulkan/exec.c` readbacks | We write back targets <= 64 KB one frame late (vulkan/exec.rs write_back_rts); check the sky and exposure reads |
| Depth clamp for the sun cascades (without it the cliff over the Shrine of Resurrection cast no shadow; lit fog) | `CMD_SET_DEPTH_CLAMP` | Have (Vulkan, Metal) |
| IPA multiply for soft particles (w / clip.w: tiled "fog" in the shrine); noperspective HUD colours (black minimap) | `nvn/maxwell/translate.c` | sass2spv handles IPA MULTIPLY; check the w/clip.w case |
| IADD3.RS / .LS shift (#147, beach sprites) | `translate.c` | Have (Lrs) |
| R16 shadow copy sampled with depth comparison: a depth twin made where the format lacks it (water, terrain shadows; AMD #59) | `exec.c depth_twin_*` | Check |
| Occlusion results read on time with GPU overlap (Kakariko's morning fog went out) | `exec.c` | Check with our overlap |
| Sampler and texture descriptors: samplers copied by value (shared descriptions), textures resolved at record time (the exposure chain re-initialises one NVNtexture inside a command buffer), BindTexturesDeferred, cube maps as cube arrays | `objects.c`, `cmdbuf.c` | Likely have; not checked |
| Vertex data the CPU rewrites each frame copied before the GPU reads it (#620: rain and torch particles stretched) | `exec.c` | Check with our overlap |
| Kernel: timed condvar wait returns without the mutex; yield after unlock | docs | Likely have (TotK runs) |
| Native Yaz0 stream decoder, checked byte for byte against the game's code (`SAVAGEWIND_YAZ0=2`): Hyrule Castle frames were the main thread waiting for it | `hle/yaz0.c`, `_yaz0_stream` | None; measure first |
| Guest setjmp / longjmp (libjpeg errors in the autosave thumbnail) | `hle/setjmp.c` | Lifter has longjmp support; check |
| FPCR.FZ set by BotW's threads mapped to MXCSR DAZ/FTZ | `runtime/simd.h` | Lifter honours FZ |
| nnMedia movies on FFmpeg, opening skippable | `hle/movie.c` | Have movie.rs |

**Our open BotW bugs against this:**

- **Blue-grey haze.** Not named in their changelog. Their early notes (docs, SAVAGE-WIND.md 2026-10-16) saw "very
  strong blue fog" from half-resolution fog and volume passes composited over the HDR image, and do not say which
  fix ended it. The likeliest lead from their code: fog and ambient colours come from the CPU reading back the 8x8
  sky target, so a stale or zero read tints the air. Second: the indoor / outdoor palette eases over 60 s of real
  time after leaving a shrine (their notes; they did not find who fills it).
- **One-frame black foliage.** Not named either. Cheapest test: nvnTextureCompare by state (a freed pool slot rebound
  by trees). Then the per-frame vertex data copy and the readback timing under our frame overlap.
- **Link stands on the pond floor.** Nothing in their code or changelog explains it; their water fixes are all
  rendering. Unexplained here.

## 4. Player features

| Feature | What | Where | Generic or BotW | For nxport |
|---|---|---|---|---|
| Settings inside the game's Options | Own pages in the game's Options screen and quit dialog | `ui/game_options.mm`, 30+ `opt_*` / `title_*` / `dialog_*` sites | BotW-only, large | No: our host menus cover it |
| Graphics presets (Potato, Original, High, Savage) | Groups of the options above | `settings.c` | Generic idea | We have presets |
| Free camera and photo mode | sead::LookAtCamera::doUpdateMatrix's eye / at replaced; world clock nearly stopped; filters, hour of day, picture | `hle/freecam.c`, `hle/photo.c`, `_lookat_update` | sead games (TotK too) | Yes, one hook per game |
| Save import / export | Wii U / Cemu saves both ways; profiles; 5 backups | `services/save_convert.c`, `save_profiles.c`, `save_backup.c` | Import BotW-only; backups generic | Backups worth having (we lost a user save once) |
| Cheats | Items into the pouch (the game's item get, on its thread), infinite rupees (GDT flag found on the heap), weather (world manager fields) | `items.c` + `_item_tick`, `rupees.c`, `weather.c` | BotW-only | We have dmnt cheats; these optional |
| Accessibility | God mode, one-hit kill, damage scales, weapon wear, infinite arrows and stamina | `assists.c` + `_no_wear*`, `_link_life`, `_stamina`, `_remove_arrows` | BotW-only, exact sites | Optional, after fixes |
| amiibo | Virtual reader for the user's figure dumps with their keys | `amiibo.c`, `services/nfp.c` | Generic (nfp) | Worth it across games |
| Quick inventory sort and drop | In the quick selectors | `quick_inventory_sort.c`, `quick_drop*.c` | BotW-only | No |
| Button icons | Glyph sheet `External_00.bffnt` (recognised by size, format, hash) redrawn with the bound keys or pad | `nvn/glyphs.c` | BotW font; method generic | Later |
| Shrine sensor volume | Waveform scaled in M_UI.bars | `shrine_sensor.c` | BotW-only | No |
| Software keyboard, text and voice languages apart | swkbd overlay; voice folder redirect | `hle/swkbd.c`, `fs.c` | swkbd generic | Have swkbd |
| Colour grading, HDR10, FSR 1 / 3, CAS, SMAA, TAA | | `color_grade.c`, `nvn/vulkan/*` | Generic | Lower priority |
| Teleport console, scripted input, clock probe | `control*.c`, `SAVAGEWIND_CLOCK_PROBE` | Generic tooling | Clock probe idea: time of day, stamina, position per second at 30 / 60 / 120 fps is the test for any frame-rate fix |

## 5. Do not copy

- Tuned or guessed: `rt_solver_steps` (6 / sqrt, 0.4 border), the surf takeoff grace, `events_at_30` (event names
  and coordinates), the VFR smoothing and interval thresholds, draw distance factors and NEAR_MAX / FAR_MAX, the
  grass cone narrowing, shadow map and texel-size matching by shape and value, HUD zones, view classification by far
  plane, the stale-actor guard and the +64 KB model margin (both hide a cause), the 140 ms bow timing.
- Licences: the project is MIT and so is `third_party/` (FidelityFX SDK, ImGui, SMAA, VMA, stb, totk-pc-recomp);
  fonts are OFL, LUTs CC0. **DLSS** (`nvn/vulkan/dlss*.cpp`) needs NVIDIA's SDK under NVIDIA's licence. FFmpeg is LGPL
  (dynamic only). The generator bundles Unicorn (GPL 2.0): not code to take. `rupees.c` says its reader comes from
  "Slop of the Wild" (licence not stated). The GUI speed idea is from Cemu's FPS++: take the idea, not its data.

## First, by value for effort

1. **nvnTextureCompare by state** (exact, about 1 hour, then the foliage count at `work/saves/botw-sunk`). Helps every
   game that rebuilds textures in place.
2. **Haze check:** log the 8x8 sky target's and the 1x1 / 3x3 exposure targets' readbacks (when written back, values)
   against a capture; 2-3 hours.
3. **Gyro from SDL sensors** for six-axis (generic; BotW's tilt shrines need it, TotK's motion aim too): 4-6 hours.
4. **BotW 1.9.0 profile:** build id from the user's files, SDK 21.4.1 layer, boot to play. Several build/run cycles;
   size known after the first boot. Use their KNOWN table to check our own finders.
5. **Frame rate, exact parts** (on 1.9.0): frame wait, VFR hook with our own exact real-time step (as frame_time.rs),
   GUI speed, controller step, sound floor, arrow gravity. 6-10 hours with clock-probe runs at 30 / 60 / 120.
6. **Physics above 30 fps:** Havok deactivation pacing, keyframed angle, demo walk arrival. 4-6 hours. Solver
   microsteps need our own rule: research first (Havok's solver rule, not fitted counts).
7. **Ultrawide and field of view** through the sead projection hooks, as TotK's aspect.rs; FOV off during events
   (event manager state). 4-8 hours; HUD tables after.
8. **Exact mouse camera** through the camera stick function (squares, aim scales, inversion). 3-5 hours.
9. **Frame-counted cutscene waits:** make `Demo_WaitForASTriggerEvent`'s trigger test catch a crossing, instead of a
   list of events held at 30 fps. Research, 4-8 hours.
10. **Native Yaz0 decoder** for the lifted engine, after a profile shows its share; verify against the game's code as
    they do. 2-3 hours.
11. Later: draw distance and shadow resolution options (sites exact, levels are choices), save backups, free camera,
    amiibo.
