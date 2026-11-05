# In progress and claims

Shared notes between agents working on nxport. Claim an item before you start it (put your name / session on it),
and update this file when you finish or drop it.

## Claimed: S41L0R Windows Machine


## Open (unclaimed)

- **TotK device lost entering the Depths: FIXED, 2026-11-03** (RTX 4080). Read-only pages under a live host-memory import
  lose the device, even with nothing using the import: a fresh 64 MB import with some pages then made read-only was lost
  one frame later, the same import left alone was fine. Write tracking kept TotK's 252 MB pool's textures read-only, so
  its first import (bindings of guessed size, 1 MiB, entering the Depths) killed the GPU; hence no fault report and
  checkpoints stopping in varying places. Now no page under a live import is protected (dirty_track::imported, until its
  GPU memory is freed): dirty_track, the write guard and the report guard all skip them, and textures there are checked
  by hash. 3 of 3 runs to frame 4850, record ~8.6 ms, GPU ~12.5 ms. Not known: why the driver does this (no public source
  found, inferred: it locks imported pages for writing when making them resident), and why one 17 MB pool was protected
  twice without a loss. Lesson: heavy logging makes crashes disappear, it does not make them: a death is evidence even in
  a slowed build, a survival there is not. Repro: a save at Lookout Landing, walk from frame 1800; about 75 s.
- **Texture formats 58 and 59 added** (vulkan/format.rs): stored as A1R5G5B5 with red and blue swapped in views. Not the
  cause of the device loss. Colours not checked by eye yet.
- **TotK hang as gameplay starts: CAUSE FOUND, FIXED in the lifter 2026-11-04.** A thread dump caught BaseProcExecutor0 not waiting, in f_7000fbc368 at a `jmp` to itself, with the others waiting for it.
  Guest spinlock (obj+0x1120): `ldr w8, [x20]; cmp w8, #1; b.eq` lifted to a plain load in a loop with nothing else in it, which the compiler made `read once; if (held) for (;;);`: a thread finding the
  lock held spun forever. Loads in loops are now volatile (module.rs reload_in_loops, cpu.h LDV*): the jump in that function is gone, and no remaining jump to itself sits on a plain load (203 -> 171, mostly bare
  `b .` traps and code split across functions, not each checked by hand). No cost uncapped (141.0 vs 140.6 fps). Needs the guest lifted and compiled again. NOT shown by counting: hang rate is too low (old guest 0 of 40, new 0 of 40
  alternating, though the old caught at 1 in 16 once); the evidence is the dump and the disassembly. Why it looked like other things: STLR (042e045, correct by the rules, not this) and the import rule
  changed timing. A per-operation lock in the event code hid it entirely (0 of 78). Tools: an in-process thread walker (cdb and lldb do not run on this machine, dbgeng.dll ordinal 1002, python3.dll).
  Audit of the 171 jumps to themselves left in the new guest: 69 branches lead to 62 of them, 51 of those with a memory operand nearby, so some do test guest memory once and then spin. That is not this bug: a volatile load cannot be removed, so such a loop has no load in it. The image has 133 literal `b .`; 107 of the 115 functions have one within 16 KB (proximity, not matched one by one), and the two short loops read among the other 8 only test registers, which would spin on a Switch too. The other 6 were not read through. Not covered: a loop split across lifted functions (tail calls, so real jumps, and it would need inlining to be hoisted).
- **TotK claims 7.5 GB private at the title and 11 GB in play (measured 2026-11-04, RTX 4080 machine, working set 4-6 GB)**: against a commit limit of 33.9 GB with 15.3 GB
  already committed by the rest of the system, one copy leaves under 8 GB. A test copy died with `memory allocation of 1179648 bytes failed` while the user played
  another game, whose freeze at about that time may be this. Do not run tests while the user is playing. From the code and log, not measured per allocation: the guest's heap 2.9 GB, committed whole at SetMemoryHeapSize, and
  address regions 1.4 GB (3,419 pools) make about 4.3 GB; the other ~3 GB is not accounted for, driver memory likely (9,422 pipelines made at startup). Private 7.5 GB
  against a 4 GB working set: about 3.5 GB committed and untouched, the heap most likely. The tracking arrays and the 16 GB arena (reserved only) are small. Committing the heap
  on demand from the fault handler would be the saving; not tried, not obviously worth its risk. A larger page file avoids it.
  Measured 2026-11-04 (Depths save, frame 2900, 9.9 GB private; address-space walk from outside, plus temporary counters on vkAllocateMemory and os::alloc/commit):
  guest heap 2932 MB (only 1437 in the working set); address regions 1409; a 1022 MB and a 727 MB block, both almost all resident, requested through neither of those counters (Rust heap or driver, unknown; the pipeline cache file is only 73 MB);
  write-combined 3.2 GB, 2432 of it in 76 blocks of exactly 32 MB with 390 resident. Live textures total about 2250 MB in about 3500 images, close to those blocks, so they look like the driver's heap for small images; that link is by size only, not shown.
  Those two blocks do not come from Rust either (a temporary counter on the global allocator saw nothing over 111 MB, and that 111 MB is an owned pool in the 0-resident block), nor from startup pipelines: with pipelines.txt moved aside, same frame, 1021 and 705 MB and total commit unchanged (10.1 vs 10.0 GB). That control was flawed, see below: it moved one of three pipeline lists.
  **Those two blocks are NVIDIA driver heap, filled by the pipeline list** (measured 2026-11-04, same save and route, frame ~2720, one run each): they hold pointers into nvoglv64.dll, SPIR-V instruction words and option text. Private committed bytes: normal 8125 MB (20,562 shader modules, 472 MB of SPIR-V, 10,015 list pipelines, blocks 1022 and 682); stages only, no list pipelines 7273 MB; no list at all 6483 MB (518 modules, 10 MB of SPIR-V, about 555 pipelines, blocks gone but a 260 MB one). So the list costs 1640 MB: about 790 for modules and 850 for pipelines, by subtraction. The route used about 3% of the modules, though the list exists to stop hitches elsewhere, so this is not all waste. vulkan/shader.rs never destroys modules. Destroying all 20,562 at frame 2900 did not lower commit (the driver keeps its heap), so a saving would have to come from not holding them all at once, for example destroying a module once its list pipelines are made and creating it again from the stage cache when a later pipeline needs it. Not tried. Address regions here: 695 MB committed, 476 MB live, not the 1409 above (that number is unexplained, possibly a cumulative counter; not checked). Private committed is 8.1 GB against the 9.9 GB commit measured before; the difference is not accounted for. Hooking NtAllocateVirtualMemory inline crashed at startup, so ownership comes from block contents and these controls, not allocation stacks.
  Largest textures: ASTC decoded to RGBA8, ASTC_8X8_SRGB 14 images = 671 MB, ASTC_8X8_UNORM 2 = 161, ASTC_8X6_UNORM 8 = 63 (one run; list cut at 12 rows, so more may be below). 8x8 is 2 bits per pixel on the Switch and 32 here, 16 times. Tried for ASTC_8X8_SRGB on the local branch astc-bc3 (not pushed): encoded to BC3 on upload, commit 10.2 -> 9.4 GB, two runs each, so texture memory does drive this commit. Frame rate unchanged, Depths route and wheel right. Quality compared at 1080p (2x crops of terrain and rock, one Depths scene, one run each): no visible change. Static shrine, 480x270 frames: old vs BC3 differs less than old vs old (mean 0.5 and 0.64 against 0.74, nothing far off), how many of these textures are on screen there is not known. The user should see it in play, on the surface too, before it goes to main. Also spare staging at its 256 MB cap. Hair and cloth on 1.1.2 still needs a look by eye at 60+ fps.
- **Mods on every platform (user request, 2026-11-04).** Content mods: fixed and pushed (0010bbb), each layout run in TotK against the old build. Not done: `romfs.bin` and `exefs.nsp` (packed; a message says so), `.stub` files, cheats, yuzu's `romfs_ext`. Note for tests: a program inside an app folder gets no_mods unless the app has `Mods`, so `--mods-dir` does nothing there; put the mod in the app's Mods folder.
  `main.npdm`: the modder's request means treating exefs as Atmosphere's overlay. Atmosphere overwrites the npdm's program ids with the running program's (stratosphere/loader/source/ldr_meta.cpp), so the id says nothing; permissions are not enforced here. Stack size and the rest are still not read from a mod's npdm (the engine's stacks are larger than the 1 MB exlaunch's template asks for).
  **exlaunch on x86: no runtime slowdown today, the cost is build time.** exlaunch writes code only through svcMapProcessMemory views and flushes them (util/sys/rw_pages.cpp), and hooks go in a JIT_CREATE area of its own module; the engine sees all of it without page faults (svc.rs Shadow, flush_code_views). The two-pass lift puts the patched code in guest.dll, so it runs at full speed, but it is two full lifts (16m52s measured for one, 10,200 functions in 41 s per file), redone for each change of mods, and writes after --aot-dump-frame are lost.
  Proposed, not built: an overlay. Keep the unmodded guest.dll; at the end of a mods' init run, diff the code; lift only the mods' modules and the functions holding changed instructions into a small library (about 4 ms a function at that rate, so seconds); point the base functions at it by writing a jmp over their x86 entries, and update the function table rt_dispatch reads. One file sampled (main_021.c, zig cc flags as lift.rs): all 10,200 functions keep their labels, 11,067 direct calls and jumps for 10,895 in the C, so entries are not being inlined away. Check that across the whole library, then mark them noinline if needed. A mid-function hook replaces the whole function, so it needs no entry in the middle. A thread already inside the old function finishes in it: apply before the game runs. Cache by hash of the patched code, so only the first launch with a set of mods compiles. Later writes: recompile on flush and swap entries, needs measuring on a mod that does it. iOS: the mods' code stays baked into the signed image, so exefs changes need a new .ipa; romfs mods could come from Documents without one (not done, ios_env.rs only reads the bundle's).
- **Building .ipa files on Windows (user request, 2026-11-04), local branch `ipa-windows`, not pushed, nothing run on a device.** Works: the iOS engine compiles and links on Windows without Apple's SDK: ld64.lld (`-flat_namespace -undefined suppress`) against generated .tbd stubs that only name each library, C through clang `--target=arm64-apple-ios16.0` with zig's macOS headers (zig's own `-target aarch64-macos` objects are refused: platform macOS). Result: iOS platform, 21 dylibs, 252 imports, parsed by llvm-objdump; whether dyld accepts flat lookup on iOS is unknown. The flags are only in a scratch env so far, not in the builder. adhoc_sign.rs replaces codesign (5 unit tests, signs that engine, llvm-objdump reads it; not checked by Apple's tools or a device). Icons and the zip work without macOS calls.
  BLOCKER: the guest image. nxport::guest_image on Windows has no native rewrites (x18, tpidr, svc, exclusives: cfg(nx_native), module_loader.rs native_rewrites, cpu/trampoline.rs, about 50 cfg sites in the load path, install_native_hooks), so ipa::build refuses off a Mac rather than package code that would crash. Porting it means running those rewrites on x86 with the same trampoline order, TSD offsets and guest base as the iOS engine; a mismatch would only show on a device, so the image should carry a hash of those and the engine check it. Needs an iPhone or a Mac to test. Also not done: Metal shader cache (optional, the app translates as it loads), cli/builder gating.
  **Reduced 2026-11-04 (pushed):** shader modules live only while a pipeline is made, and graphics stages no longer make their own set layout. The list made 20,322 stages for 4,105 distinct translations, so most of that was duplicates (listed.rs stages() still builds a Stage per entry, now without driver objects). Frame 2900, two runs each: shrine commit 8166 -> 7359 MB, working set 4295 -> 3552; Depths working set 6131 -> 5312, commit 11.1 -> 9.9 and 10.6 GB (noisy). List pipelines 4.0 -> 4.5 s on workers. Cold cache, wheel, Depths route fine, no device loss. Correction: the 8.1 vs 9.9 GB gap in the note above compared the shrine save with the Depths save; the Depths takes about 3 GB more in the same build, so most of it is scene. Next candidates: heap committed on demand (up to 1.5 GB of commit, not RAM), the remaining list pipelines (about 850 MB, by subtraction), per-heap numbers from VK_EXT_memory_budget (VRAM not measured yet).
  VK_EXT_memory_budget, after that change, one run each, frame 2700, RTX 4080: shrine VRAM heap 1802 MB and system heap 545 MB (commit 7385); Depths 3913 and 875 MB (commit 10505). So VRAM is not the limit on this card, and vram + system heap (2347 MB) is close to the 2432 MB of write-combined 32 MB blocks: the blocks look like Vulkan's own allocations also counted in the process's commit (by size only, not shown).
- **The report guard costs about 4% uncapped** (shrine view, 135 fps off, 130 on; measured 2026-11-03): the game now waits
  for reports it used to read too early, as on the Switch. Its chunk split at passes holding reports moves the reported GPU
  time (5.2 to 6.0 ms, the timestamps span more) but not the frame rate (135 with, 133 without). Not worth chasing unless
  the guard's waits (0.2-0.7 ms a frame, up to ~10 ms at worst) show up as hitches in play.
- **Metal report guard: same retry bug as the Windows one had** (gpu/report_guard.rs `fault`, changed to `repeated` as on Windows, type-checked, NOT run: needs a Mac): LAST_RETRY is not cleared when a retry works, and games touch the same report slots every frame, so a retry from
  frames ago makes a write on a just-opened page look like another module's fault: a crash. On Windows this crashed 2 of
  3 long runs once reports were written early; Metal opens pages less often, so rarer. Fix as in report_guard_lifted.rs
  `repeated`: a repeat only within 1 ms of the retry it follows.
- **TotK map: a texture flashes when zooming the map in and out very fast** (user report, 2026-11-03, Windows). Repro 2026-11-04, stock fps, a save outside a shrine: `-`
  at frame 2100 opens the map, right stick up and down alternating every 6 frames from 2300; at a zoom reversal (2313, 2325, 12 apart) the terrain goes blurry and pale for a frame, 13,000 px
  (half-size frame). Not known whether the Switch shows this too (a low-detail layer while zooming) or it is ours: needs a reference (a Switch video, or the Metal build) before more work. Passes barely change on those frames and nothing is uploaded around them (no late upload). The detector also fires on most zoom steps, so it may be catching the zoom itself; only the one blurry frame was checked by eye.

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
  anything. 1.0.0: fixed 2026-11-03 (3e01322): all four sites read {step seconds, step frames} words time_step keeps
  after main's .bss (no call); lifted and run headless into the opening, not yet compared by eye at 60+ fps.
- **TotK ambient occlusion / green glow flickers while the camera moves** (FIXED 2026-11-03, ab21579: compute stages read the dispatch's thread group counts at 0x200 of the driver constant buffer, which we left 0, so the depth-cell passes wrote only row 0 and the reprojection failed whenever the camera moved; user-checked in game. Notes below kept for the record): the 320x180
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
  AO TEST (2026-11-03, checked by eye with the user): save-test, `--debug-input-script=600-620:6,900-920:6,1200-1220:6,
  1500-1520:6,1800-2100:13,3200-3290:13,3350-3370:40 --frame-history=800 --history-dump-at=4200` headless: enters the
  shrine, walks to the two stone heads, tilts down, still from 3400. The AO (bases of the heads, rail edges, cobbles)
  switches on for a frame and off again ~180 times in 800 frames; count frames where >3000 pixels darken together, and
  look at a few (red-/blue diff image) before trusting a result. The entrance glow is out of view.
  INVALID (user, 2026-11-03): everything from here to the end of this item measured the shrine entrance's animated green
  glow (intended), not the AO bug: the switch metric (>3000 pixels moving one way) catches that animation, which follows
  game time. Keep the tools, discard the conclusions. The real AO flicker still needs a repro and a metric checked by eye.
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
    window (bursts come late in the 3300..4200 range); ~840 MB per dumped frame. Three more tries (dumps after 3900, 4050,
    4150) were quiet too, and dumping stalls frames (large steps), which itself suppresses the flicker: catching it
    this way needs the dump to cost nothing to the step (e.g. keep time_step at the pre-dump frame time while dumping). Tried that
    (passk-replay.patch: replays the 128 steps before the dump): two more 20-frame dumps (after 3800, 4100) quiet
    too, and no switches in the 60 frames before either. Five quiet windows in a row: the bursts are rarer in
    short-history runs than in the 900-frame runs, or the extra capture work changes timing enough to avoid them. Not the ground-carry fix (merged
    meanwhile): 900-frame A/B, before 2 / 294, after 223 / 124. Per-run variance is that large; compare several
    long runs per setting.
  - Journal of a flickering 900-frame run: the frames follow a 9-frame round-robin of work (events per frame repeat:
    ~2860, 2970, 2420, 2420, 3110, 3480, 2800, 2900, 13540), but dark / light does not lock to it. A dark and a light
    frame at the same position in the cycle (3980 vs 3989) issue the same passes, draws, samples and barriers (only a
    200x112 pass s_5d49414c2a11700b drew 18 vs 19, and that count does not follow dark / light). So the switch is in
    buffer or texture contents, not in what is drawn: next, hash uniform / storage contents per pass (all passes, not
    only screen passes) on dark vs light frames. Done (cbh.patch, scratch: hash of every constant buffer of every
    draw): the best split was the 200x112 pass s_5d49414c2a11700b, vertex slot 12, but its changing words are a
    position drifting ~1e-4 a frame (Link or the camera), not two states; the dark / light labels from the switch
    signs are approximate. Constant buffers do not show the switch. Next: storage buffers and the GPU-written data
    (the 320x180 reprojection pass and its per-cell depths, see the first symptom above).
  - Not GPU timing: waiting for every submit from frame 2900 (step then ~0.4 frames) still flickers, 319 / 193 in
    900 frames, so storage shadows chosen by what is in flight are not it either.
  - Not a tick rhythm: a constant step of exactly 0.25 from 2900 still flickers (5 / 155), and the switches do not
    lock to frame mod 4 (all phases alike; most gaps are 1 frame: one-frame dark or light blips). So it is not
    "something updates every whole game frame"; the blips need steps under a frame, come in bursts, and land at
    random frames within a burst.
  - Confirmed from the other side: raw +8 and +0xc real, time scale +0x108 = 1 / step so every derived channel is a
    whole frame (NXPORT_EXP_INV): 0 / 0 in 900 frames. The consumer reads +0 or a channel (all use slot 0 while
    +0x10c is 0). Next: of the ~25 readers of +0 (watchpoint list), redirect halves to a field time_step controls
    (slot 15 value +0x100, if its target +0x104 stays NaN), switched at frame 2900 so the path is the same.
  - Tools (scratch, not committed): watch_win.rs (DR0..DR3 watchpoints + VEH, symbolize with llvm-symbolizer
    against guest.pdb at ImageBase 0x10000000 + RVA), a private build root at C:\Users\Greenlord\nxport-build\r8
    (short path: the scratch path is too long for the compiler), ao2.ps1 (900-frame capture, count, delete).
- **TotK Ultrahand / ability wheel background is black: FIXED on 1.1.2, 2026-11-04** (frame_time.rs open_short_cut_bg; 1.0.0 needs its addresses, Sites::short_cut_bg). Uncapped 8 of 8 and stock 4 of 4 right (left edge brightness 69-70, black 6; by eye like stock), a second opening right, 142.7 fps. The capture lands 3 rendered frames after the opening animation's frame reaches 1, the world stops 4 frames after it starts opening: the animation now starts at frame 1 of 6. Not by eye in play yet. D-pad menus: below.
  D-pad quick menus are fixed by the same change (2026-11-04, Depths save, by eye): the 11-03 build is black behind the weapon menu, the item menu and the wheel, uncapped; this build shows the world behind weapons, shields and items uncapped, and weapons and items at stock. D-pad down opens no menu. One run each.
  1.0.0, found but NOT enabled: Sites::short_cut_bg would be (0x45596a8, 0x196dee8, 0xc5a49707). Found from the screen hash's mov/movk and the bl and global load next to it; the same search returns 1.1.2's known values. +0x16e and +0x28 appear near the matching code, the layout's +0x60/+0x68 and the animation's +0x20 were not checked. Enabling it needs a 1.0.0 guest and a 1.0.0 save with the ability unlocked, which I do not have.
  Located 2026-11-03: ONE whole step (1/30 s, timer+8 = 1.0) on the one frame in which the game makes the transition fixes it, uncapped. Measured with
  the headless repro below, by the mean brightness of the screen's left edge at frame 3600 (black 6, right 69-70, stock timing 70, checked by eye):
  whole step on the frame just before the state byte below is first seen by time_step: right; one frame earlier or on the frame it is seen: black. So
  whole steps once the state has changed come too late, and the step has to be chosen before the transition is visible. Unknown yet: what in that frame
  reads the step (timer+8 / +0xc), and whether it needs 1.0 or some threshold (try 0.75). Next: find that reader, then give it what it wants there
  rather than change the step for the whole game (the frame's other users get a jump of ~26 ms at 144 fps). The manager: lookup f_7001aaf400(x0 =
  [[0x700472cb20]], x1 = 0xc5a49707). +0x16e state: 0, 1 opening (from ~14 frames after L at 144 fps), 2 open, 3 closing. +0x176 bit 0x20: capture
  requested (SeqOneShotRequestCaptureShortCutBG, f_7001abaaa8, also sets +0x175 bit 2), cleared by SeqOneShotDRequestCaptureBeforeShortCutBG
  (f_7001aaf468). SeqQueryGetShortCutBGRestFrame (f_7001ae119c): a total minus the float at [+0x28]->[+0x60 or +0x68]+0x20. main starts at guest
  CAUSE, measured 2026-11-04 (step held at S from frame 3200, journal, repeated runs): the world stops at state+4 rendered frames whatever the step, the capture lands at state +
  2 + ceil(1/S): S 2.0 and 1.0 +3 (right), 0.99 and 0.5 +4, 0.25 +6 (black). It works only if it comes before the world stops. So something accumulates the step and acts at >= 1, while
  the world stops by counting rendered frames. Timer channels carrying the step: +0, +8, +0x10, +0x38, +0x60, +0x154, seconds +4, +0xc, +0x14, +0x3c, +0x64, +0x150; swapping all of them
  for readers of the timer does not move the capture (hold timers do move), so it reads a copy (about 300 floats hold the step). It is the wheel's opening animation (frame 0 to 6 at
  the step, [[+0x28]+0x60]+0x20): at a held 0.25, setting that frame to 1 when the state shows moved the capture from +6 to +3, 69 (2 of 2). Writer f_700084e408 (frame += rate * step).
  Search for the reader (page-protect the timer, single-step each read, swap the value for chosen readers; patches kept out of the repo): NOTHING usable. With the
  real step the single-stepping slows the game to ~16 fps, so the step is over 1.0 for everyone and every subset passes. With the step held at 0.5: control
  black 3 of 3, all 35 readers given 1.0 black, but one earlier run on that build gave 69 for subsets of 17 and 8: unexplained. f_70028ff8c4 / f_70028ffc08 are
  wrap-around timers (6000, 600), not the cause (black 3 of 3). The one-frame results above came from single runs and may not repeat: repeat them first. A
  better next step: a journal of the capture frame in both cases, instead of swapping values.
  0x7000100000. Earlier notes:
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
  code; this is the fallback. Fallback tried (bbebcba, reverted 9b3e4f1: no workarounds): 30 fps and whole steps while a menu button is held; the
  real fix would drop that. Static lead for it: the sequence node class named
  `SeqOneShotDRequestCaptureBeforeShortCutBG` (name getter main+0x2814bd8, vtable main+0x439c320..0x439c430; main starts at guest 0x7000100000, so the getter is lifted f_7002914bd8). They also average the real-time step over 5 frames, which may calm the AO flicker
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
  - Other TotK versions: done for 1.0.0 and 1.1.2 (3e03b61); any others still need their sites.
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
