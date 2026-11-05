# In progress and claims

Shared notes between agents working on nxport. Claim an item before you start it (put your name / session on it),
and update this file when you finish or drop it.

## Claimed: S41L0R Windows Machine


## Open (unclaimed)

- **Push TotK until it is heavily GPU bound (user goal, 2026-11-05).** RTX 4080 machine, 16 threads. A live sample in the user's play (15 s, scene unknown, 144 limit, vsync off): 87-90 fps, GPU 37-40%, busiest thread 75% of a core, four more near 50%, 4.3 cores busy of 16. So not GPU bound; no thread pinned, which suggests threads waiting on each other (not measured). Test save: the user's own, read-only copy at nxport-build/save-100fps (slot 2, 2026-11-05 01:55), where they expect 100+ fps. First: --gpu=null at that spot for the CPU-side ceiling, then per-thread CPU and wait profiles, no guesses.
- **Perf overlay on Windows: done 2026-11-05.** The Mac's window, in host/perf_stats.rs, drawn by vulkan/overlay.rs: F10 or `perf-overlay = true` under [display]. Seen working in a capture of the window; F10 itself not pressed in a test. First reading at the user's save: 78 fps, GPU 6.7 ms of a 13 ms frame, 4275 draws, 214 passes, record 9.3 ms in the fps line.
- **CI: builder releases on GitHub (user request, 2026-11-05).** Written on local branch `ci-builder-releases`, never run, not pushed: awaits the user's review. It adds opus.dll to the Windows archive, built from Opus 1.6.1, because without it Opus streams decode to silence (horizon/audio/opus.rs).
- **Push TotK: compilers, 2026-11-05: no gain.** The user's save, frames 3500-5000, 4 pairs each. Round A: current build (zig 0.13, generic tuning) 11.18 ms (11.04-11.27), clang 23.1.4 -O2 11.13 (10.92-11.23). Round F: current 11.24 (11.13-11.37), zig tuned for this CPU 11.18 (11.12-11.27). Both inside the spread: ship neither. Found: zig's -O1 is clang -O2 and any -m flag makes it tune for generic x86-64, so the "-mcpu=native" comment in lift.rs is wrong. clang 23 can build the library (mingw headers from zig, zig's lld-link; System V and musttail fine) in 1155 s. Not checked for those builds: behaviour against the current one, because neither is shipping.
- **Test scripts on unlocked fps: frame numbers are not gameplay.** At 144 fps the title and menus pass frame 1800 in about 12 s, so `600-620:6..1500` and `--until 1800` never reached the game. Gameplay starts near frame 2900 with A every 300 frames. Every "uncapped" comparison before 2026-11-05 03:30 may be menus: 141 vs 142 fps for UltraCam, the 134 "machine" numbers, no-cost claims. Corrected on the user's save, frames 3500-5000, base2, same round: no mods 93.3 / 88.7 / 89.5 fps, UltraCam live with MaxFramerate 100000 84.3 / 89.4 / 86.1: a cost of about 4% is possible, runs overlap. Depths route to frame 6000 at about 80 fps, no fault, commit 11 GB. Also unexplained: these read record 6.7 ms and about 90 fps against 9 ms and 80 in the profiling runs on the older library an hour before.
- **Push TotK: profile-guided optimisation wins; link order and less spinning do not, 2026-11-05.** Save nxport-build/save-100fps frames 3500-5000 and the Depths route frames 3500-6000, headless, 4 interleaved sets, ms per frame against the same round's h/base2.
  - **PGO, clang 23 -O2, same semantic flags.** Spot profile: spot 11.02 against 11.42. Spot + Depths profile: spot 10.52 against 10.98 (95 against 91 fps), Depths 10.24 against 11.67 (97.6 against 85.7 fps). DLL 304 MB against 338; compile 5.5 min at -j 14 against 22. Instrumented code runs at about 70 fps; -fprofile-generate needs a small shim for the MSVC-built profile runtime and a watcher thread that writes the profile before the kill.
  - **On top.** ThinLTO: spot 10.45, Depths 10.12, needs LLVM 23's LLD; small extra gain. -O3: 10.65, 10.33: no gain over -O2.
  - **No gain.** Hot-first link order from 176,961 samples (6,179 functions, 31.6 MB): base 11.53, -ffunction-sections 11.63, ordered 11.75; a noisier round agrees. So instruction TLB pressure is not shown to matter. Less spinning, base2 library: control 11.47, LightEvent spin 10 us 11.56, none 11.70, 64 pauses per sleep poll 11.64. The pause samples: light_event_spin 62%, clock sleep_ns 34%.
  - **Accuracy.** 8 runs of the merged build, Depths included: no crash, stall or device loss. Opened paths differ from base2 by 33-147 against 49-152 base2 to base2. A frame 4000 screenshot shows the same scene by eye; pixel differences are inside base2's own run-to-run noise, so this check is weak (frame numbers are not game time). Other scenes (shrines, combat, other regions) not tested.
  - **Shipping.** zig 0.13 refuses the profile (unsupported format version), so the builder would have to download clang, plus one profile per game version and lifter output (90 MB, 15 MB gzipped). Changed functions lose their profile. Flags not changed.
  - **Test trap.** A cold DLL can leave the frame-numbered input script in the menus: one 144 fps run with 5 s stalls.
- **Push TotK: where the job threads' CPU goes, 2026-11-05.** Same save and frames, h/base2, main plus a temporary in-process RIP sampler (about 1 kHz on guest-main, ModuleSystemWorker1/2, BaseProcExecutor0-2; not committed). 4 runs, 176,049 samples; 70,346 (40%) were threads already blocked in NtWaitForAlertByThreadId / NtWaitForSingleObject and are dropped, leaving 105,703. Sampler cost, paired off/on: +0.03 to +0.66 ms, mean +0.38 ms (3.4%).
  - **Split of CPU samples.** Lifted code 76% (f_ 70.5%, plus unnamed pieces 6.7%, taken to be split parts of f_ functions, not checked one by one). Dispatcher 1.5% (rt_dispatch 0.4%; the unnamed function after it 1.1%, which walks the modules, taken to be the cache miss path). Engine 17.9%, of which `_mm_pause` 9.0% and atomic_load 1.1% (spinning, callers not resolved); the rest about 8% over HLE paths (host_call_hle 0.41, Hle::call 0.33, memmove 0.33, LockMutex 0.30, record_hle_call 0.23). ntdll 4.3% (heap, clock), other DLLs 0.1%. Lifted share per thread 73-85%.
  - **Flat.** 4,773 functions sampled; top 12 are 13%, top 60 about 25%. Per-function tuning has no target; only changes to all lifted code can move this.
  - **By instruction (the one before each sample, to allow for skid; top 150 functions, 36% of lifted samples; approximate).** Guest memory load 31.7%, store 7.1%; c-> load 16.8%, store 8.5%; ALU 15.1%; float/SIMD registers 9.5%; jumps 7.2%; host stack 3.2%; atomic/barrier 0.6%; indirect call/jump 0.1%; FP rounding-mode test 0.1%.
  - **Top functions read.** f_700078b20c (1.9%): plane test loop, vector registers moving through locals and c->v. f_7002348474 (0.5%): 82% on loads ending a pointer chase, guest data. f_7000a211f8 (1.45%), f_70014f5068 (0.6%): a guest load then a store of it to c->x, registers beyond the kept locals. f_7000d94750 (0.7%): 40% just after a c-> load following a stack load. f_7002341b64 (0.8%): a guest flag polled in a loop.
  - **Ranked (estimates; frame about 10.9 ms, job-thread work about 1:1 on it).** 1: fewer c-> loads and stores: 25% of lifted samples, about 21% of non-spin CPU; half of that is about 10%, at most about 1 ms, about 9 fps. A sample next to a forwarded store is not all cost, so 0.5 ms is as likely. Way: hot registers in host registers across tail calls. 2: order hot functions together at link time, from these samples (flat profile over 300 MB of code; iTLB and instruction cache not measured): 1-3% is a guess, cheap to test. 3: dispatch miss 1.1%: at most 0.1 ms. 4: engine HLE paths and heap, about 12%: at most about 0.3 ms if a quarter goes, not lifted code. **Not worth doing:** indirect dispatch, barriers, FP mode tests, spin (it runs while waiting), -O level (lift.rs: zig cc builds -O1 to -O3 as clang -O2). Guest memory (39%) is the game's own data; cache against TLB misses not separated, no counters here.
  - **Open:** the waits at lr 0x7f7744, 0x86fcc8, 0x8785b8. Those values appear in main_table.c but not in a `c->x[30]` search of the lifted C; not pursued. Callers of the spin.
  - **80 against 90 fps: not the library.** Same exe: h/base 91.7/91.0, h/base2 92.3/90.7. Not the exe either: the earlier build read 92.3 on h/base in its last run, with record and GPU down too. So the machine's state changed, cause unknown (inferred). The 76.8-82.8 fps below are from the slower state; the backend gap was not measured again in the fast one, except the 1.13 ms pair.

- **Push TotK until it is heavily GPU bound: CPU diagnosis at the user's spot, 2026-11-05.** Save nxport-build/save-100fps, headless, frames 3500-5000, limit 144, idle camera; base `2c8432c`. Timers were temporary (not committed), compared with an off build: base 77.8/80.3 fps, timers off 78.5/79.4, on 77.0/82.8, so no visible effect.
  - **Ceiling.** Vulkan 76.8-82.8 fps (about 12.4 ms), --gpu=null 87.5-93.9 (about 11.1 ms), so all of the backend is worth about 1.2-1.3 ms, 81 -> 90 fps. The last two runs read 92.3 and 103.1 (gap 1.13 ms) with every number faster, cause unknown; the machine drifts by 10%, so compare only within a round. **Null is not the same work**: over 600 frames it records 1.02M DrawElements against 0.89M, 0.70M instanced against 0.41M, 4.9M uniform binds against 2.9M (steady across frames). Inferred, not tested: occlusion reports. So 90 fps is a ceiling with a heavier game, and the true one may be higher. World looks the same: 70 of about 9,700 opened paths differ (map tiles, sound, save captions).
  - **Budget, Vulkan run at 12.07 ms a frame (CPU + timed waits; one run, CPU times are tick based).** guest-main 6.5 CPU, 4.6 LightEvent (3.76 at lr 0x7f7744, 0.85 at 0x8785b8), 0.45 Sleep (22 a frame). ModuleSystemWorker1 6.7 CPU, 5.3 LightEvent (mostly 0x86fcc8). ModuleSystemWorker2 5.9 CPU, 5.5 LightEvent. BaseProcExecutor0-2 3.5 CPU each, 7.6-7.9 MsgQueueRecv (5 a frame). Presentation Thread 9.2 CPU, VkSubmit 9.84, 2.21 idle in MsgQueueRecv, 0.64 wait_ended. vk-record 6.5 CPU, 5.5 idle. Nothing is pinned: the critical path crosses the game's job threads.
  - **Slack test: 2 ms spun once a frame on one thread or group, added ms against the same round's control, two rounds.** Vulkan: guest-main +2.39/+2.08, ModuleSystemWorkers +2.86/+2.45, BaseProcExecutors +1.90/+2.34, Presentation +0.68/+0.50, vk-record +0.68/+0.33. Null: guest-main +0.55/+0.25 (not explained), ModuleSystemWorkers +2.42/+1.59, BaseProcExecutors +2.28/+1.13; Presentation +6 ms +0.65/-0.65, +10 ms +0.94/-0.12. So **`record 9 ms` runs mostly in parallel with the critical path**: about 0.3 ms of frame per ms on Presentation.
  - **Not the problem:** LightEvent signal 0.01-0.03 ms a frame per thread, wake about 5 us (0.02-0.04 ms a frame per thread), spin 0.16-0.42, contended mutex at most 0.01. GPU wait 0.0. GPU 7.4-8.2 ms by timestamps against 38% in nvidia-smi: the timestamps may include idle gaps (a guess).
  - **Ranked.** 1: guest work on the job threads, 1:1 on the frame. 10% off them is about 1 ms, about 7 fps (estimate); GPU bound needs about 8 ms, about 3 ms more than the backend can give. Next: sampler on guest-main and ModuleSystemWorker1, a `--profile profiling` build, split guest / HLE / memory; identify the waits at 0x7f7744, 0x86fcc8 and 0x8785b8. 2: backend, at most 1.2-1.3 ms in total; 2 ms off record is at most about 0.6 ms, about 4 fps (assumes the slope holds downward). Proof: A/B over the same frames. 3: guest-main's Sleep, at most 0.45 ms; log requested times and callers first. 4: sync primitives, under 0.5 ms: leave them.
  - **Not established:** the split inside the job threads (sampler not run, so the guest / HLE / driver share is open; only Presentation is known, nearly all backend, as under null it did not reach the top 16 threads). The swapchain present's cost: games now run headless at the user's request; two windowed runs finished before that (83.3, 76.9 fps), inside the spread. What the backend's 1.2 ms is, since occupying Presentation under null explains at most about 1 ms: run a cache or memory counter comparison, or a null run with vk-record's load.

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
- **Mods on every platform (user request, 2026-11-04).** Next needs the user: an .ipa on a device, UltraCam played, another exlaunch mod. Not started for want of a test: lifted `ic ivau` (no mod known to need it), the macOS present layer (no Mac). Content mods: fixed and pushed (0010bbb), each layout run in TotK against the old build. Not done: cheats, yuzu's `romfs_ext`.
  **Packed forms done (2026-11-05, horizon/packed_mods.rs, fs.rs).** `romfs.bin` is read in place, its index in memory, so a phone keeps one copy. Within a mod its folder wins over its romfs.bin, as Atmosphere's builder (from memory of its source, not rechecked), and mods merge by the usual priority. Run in TotK (private Mods folder, one run): `!High/romfs` held RegionLangMask.txt, `Packed/romfs.bin` held that, a bgyml and ZsDic.pack.zs. The game got the first from the folder and the others from the image, a read of 851968 returned the file's 108344 bytes, and it reached the shrine. No-mods run also reached the shrine. `exefs.nsp` is unpacked to `<game data>/mods_unpacked/<hash>` (modules load from paths; a mod's folder may be read-only); `<name>.stub` and the nsp were unit-tested only, with synthetic files, no game run and no real mod. Rules, inferred from memory of Atmosphere's code redirection: loose file, then stub, then nsp. An exefs.nsp stands for the whole ExeFS, so the game's main, sdk and subsdkN go; missing main refuses to load, missing sdk prints the existing "SDK load failed" and will crash. Across mods: main and sdk go to the first mod that says anything; subsdkN of mods still all load (they add up), so a stub removes only the game's module. On Windows a main of another size still fails in lift_overlay ("the base library holds other code"), as a loose one does. Not handled: a movie inside a romfs.bin (the player opens host paths, so it plays the game's), IPS files inside an nsp (Atmosphere keeps them in exefs_patches). iOS: not run on a device; the macOS type-check passes.
  Note for tests: a program inside an app folder gets no_mods unless the app has `Mods`, so `--mods-dir` does nothing there; put the mod in the app's Mods folder.
  `main.npdm`: the modder's request means treating exefs as Atmosphere's overlay. Atmosphere overwrites the npdm's program ids with the running program's (stratosphere/loader/source/ldr_meta.cpp), so the id says nothing; permissions are not enforced here. Stack size and the rest are still not read from a mod's npdm (the engine's stacks are larger than the 1 MB exlaunch's template asks for).
  **exlaunch on x86: no runtime slowdown today, the cost is build time.** exlaunch writes code only through svcMapProcessMemory views and flushes them (util/sys/rw_pages.cpp), and hooks go in a JIT_CREATE area of its own module; the engine sees all of it without page faults (svc.rs Shadow, flush_code_views). The two-pass lift puts the patched code in guest.dll, so it runs at full speed, but it is two full lifts (16m52s measured for one, 10,200 functions in 41 s per file), redone for each change of mods, and writes after --aot-dump-frame are lost.
  **Overlay prototype works on UltraCam, TotK 1.1.2, 2026-11-04** (nxport-cli base-code, lift-overlay, compile-guest, then `--guest-overlay`; not in the builder yet). Lift 15 s, compile 27 s, run to the dump 14 s, lift 16 s, compile 22 s: about 95 s, against 2 x 16m52s. Overlay 7 MB: 28 functions of main's 519,499 (24 instructions differ), UltraCam's 1,366, plus the game's subsdk0 (22,596, why is it needed at all?). The gate: 587,265 base functions, all 16-byte aligned, none shared, smallest gap 16, so the 14-byte jmp always fits. Evidence hooks run: only the overlay with patches reads UltraCam's maxlastbreath.ini; counters in the generated C show 17 of the 28 functions hit, many per frame, one with x30 inside UltraCam (0x70051e4c6c), so game -> mod -> trampoline -> game works. Shrine, uncapped, frames 3500-5000, two runs each: no mods 140.4, 142.9 fps; UltraCam by overlay 142.4, 142.3. Ran to frame 2600 without a fault. NOT shown: that UltraCam's features work. Fov 110 against 50: the same picture; Width/Height 1280x720, DisableFog, TimeSpeed 5: the same picture and size; 142 fps with MaxFramerate 120. Could be the overlay, the engine's own frame_time hooks, or how these options act in that scene: needs a fresh two-pass build of the same source as the control (the 10-28 build is not comparable and did not reach its capture). Also: writes after the dump frame are lost (as two-pass); proposed next: builder steps, cache by hash, drop the game's subsdk0.
  **The warmup run is NOT acceptable (user, 2026-11-04): hooks may not have settled by the dump frame.** Removed from the builder. A mod can patch later (a save loaded, a menu, game state), or differently with another save, display or config, and those patches are lost. Proposed, awaiting the user's choice: translate the mod's code at build (no run needed), then handle each flush in the real session (flush_code_views sees every write): find the changed functions, run them at once, compile in the background, swap entries, cache by code hash so later launches load them. Running them at once needs either an interpreter of the lifted instructions (no pause, new work that must match the lifter) or a pause on the first hook while it compiles (simple, may stutter once). Open: swapping an entry another thread is in (the current design patches before guest code runs), mods' JIT areas, the compiler being present at play time. Measure how many patches arrive after frame 300 with UltraCam; that could be part of why some of its hooks never fire (not tested). iPhone: exlaunch and other plugin mods are not supported, for now (user, 2026-11-04). IPS, pchtxt and romfs mods remain.
  **Live patches WORK and replaced the warmup (2026-11-04, on main). UltraCam, TotK 1.1.2:** no warmup; the builder makes the base once, then an overlay of the mod's code and of what IPS-style mods change, 26 s from mods to app (base seeded from a library on disk; a true first build, builder defaults, empty data folder, UltraCam: 1478 s to an app of 598 MB that ran to frame 1800, 16 waits on its first launch, 0 on its second; 4371 of 4423 pipelines compiled; the other 52 (1.2%) would compile on first use if ever needed, cause not measured, per-reason counts in vulkan_precompile would tell). At run time 37 hooks land, including 5 HLE trampolines, one twice (lifted code now leaves those to the interpreter, and a hook's call of the original reaches run_hle) and the SDK's. **FOV 110 now visibly widens the view; with the warmup it changed nothing, so the warmup was dropping hooks** (most of the 37 were missing from its 28 changed functions; why is not established: page versions in cpu/aot.rs are the place to look, if it matters). Every patch came before the first frame report in those shrine runs. Compared: uncapped 142.3 and 140.6 fps with UltraCam, 142.4 and 140.8 without; commit +500 MB; 1800 to 8400 frames, Depths route, wheel, no fault or device loss, each one run. 148 functions compiled in the background, 16 waits for a compile, on a first launch. Base lift 85 s, compile 20 min at -j14.
  Interpreter design as it ended: calls nest and returns return, as in lifted code, always, or a return runs on into lifted code (found by running it). Compile: engine live_patch.rs, one DLL per function in <data>/games/<title>/live_patches, named by a hash of its code, seeds and the engine exe.
  Later launches: 16 waits on the first, 1 after, the function cache by code alone with its entry points accumulating (3 launches, UltraCam; 127 of 136 changed functions served at once by the third). 116 MB of cache for this mod.
  Counted with UltraCam live, uncapped, frame 3600: 20 of 37 hook handlers ran, some over 10,000 times. Not run in that shrine scene: the camera getter's, the map-related ones and others. The present hook (sdk+0x17f8b4) and 0x17f520 are NOT NVN driver functions, as an earlier note here said: type info N7android7SurfaceE precedes their table, so they are android::Surface's vtable, and slot order against Android's header makes them queueBuffer and setSwapInterval (the method names by position, no symbols).
  **Present layer (2026-11-04):** queueBuffer never ran here, so mods hooking it did nothing. Now, in an app with plugin mods, it is emptied (mov w0,#0; ret) before their init, found through the type info; a present calls it only if a hook changed it. UltraCam's handler runs: it measures frame time, calls the empty original, then its limiter. Counted, shrine, --fps=unlocked: MaxFramerate 59 / 61 / 90 / 100 / 120 give 59.0 / 61.0 / 89.9 / 100.0 / 120.0 fps; was 142 regardless. No mods: 144, layer off. Depths route at 120: no fault.
  MaxFramerate 60, 30 and 20 skip the limiter's sleep by design (read in its code for 60 and 20; 30 measured), leaving them to the interval it forces in its Surface::setSwapInterval hook. So nvnWindowSetPresentInterval now also calls that hook when hooked, reads back the field (found by decoding the SDK's own function, 0xf24 here), and the frame limit is held to 60 / forced. The game keeps seeing its own interval. Counted, unlocked, same scene: 60 / 30 / 20 give 59.9 / 30.0 / 20.0; 100 and 120 still hold; --fps=stock stays 30 (an explicit setting wins, not a bug as an earlier note said); no mods 144, no forced interval. Depths route at 60: no fault. Open: macOS has the same dead hooks; this layer is Windows only, in live_patch.rs.

  Open: (1) Done: lifted `ic ivau` reaches the host (rt_code_changed), which copies the one page it names. Overlay re-lifted against the old base, UltraCam: 88 patches seen at a flush, init returned 1 page before and 0 now, 5 HLE hooks, limiter holds 100, 34-38 s to frame 1800; comparing whole views at each cache line took 96 s, hence one page. Full base lift (86 s + 1313 s), runs with and without mods, Depths. No flush came after the first frame, so a mod patching late is still untested. (2) Done: entries unused for 30 days leave the cache at launch (a planted 40-day-old one went, in a run). (3) Fixed: a thread that called an HLE trampoline before a mod hooked it kept calling it directly. Counted with a hook on the present trampoline at frame 1000: 1400 of 1400 later presents through the hook, 0 of 1400 without the fix; a temporary test, not kept. Cost: none seen, MaxFramerate 200 from frame 2000 on, 133.9 and 134.9 fps with it, 133.7 and 134.2 without. The 134 itself was the machine, not the limiter: same frames (2000-3500), three rounds, no mods 132.7 / 143.4 / 143.0, hook with nothing to sleep (MaxFramerate 100000) 139.4 / 141.7 / 143.2, hook at 200 139.3 / 142.5 / 143.2. All against the engine's 144 cap, so a cost under it would not show. (4) UltraCam's other settings are not checked. (5) Answered: the 58 was the frame limit, 60 by default; the app holds 60. (6) Only one mod's code tried: TOTK Optimizer here is byte for byte the same subsdk3 as UltraCam, so it adds nothing, and no other exlaunch mod is on this PC; needs one from the user. (7) iPhone: no plugin mods, by decision: files.no-plugin-mods, on in iOS apps, leaves their code out with a warning and keeps the rest of the mod; an .ipa built with UltraCam in the folder, never run on a device.
  **Live handling chosen: an interpreter (user, 2026-11-04), branch `live-mods`, local.** Done: its core, crates/lifter/src/interp.rs: integer, loads and stores, branches; anything else (FP arithmetic, SIMD, exclusives, atomics, svc, system registers it lacks) stops at that instruction with nothing done, to wait for the compile. Checked against the lifter's own C on the game's instructions: 12,000 distinct words, 43,368 states, 0 differ; about 9% of distinct words are declined (whether a mod's trampolines hit those often is not measured). The check caught both deliberate bugs put in to test it, a lost carry (42 differences) and asr as lsr (1: thin coverage of that form), after its first version missed the lost carry: random states rarely make operands equal, so it now adds edge values, all registers equal, and values equal to the immediate. Scratch, not committed. Not done: everything that connects it. Plan: at each flush, find the changed base functions; give each a small x86 stub (System V: rdi = Cpu*) setting c->pc and calling a host entry that interprets, then tails into rt_dispatch, which the guest library must export (a base rebuild); write the jump over the entry so a running thread is safe (a 2-byte jump to itself first, then the rest, then the first bytes); code outside any table (mods' JIT areas) already reaches Handlers::unknown; Stop::Unsupported waits for that function's compile. Then compile in the background, swap in, cache by code hash, and remove the warmup. report_guard also needs to treat the interpreter as guest code.
  Connection (since run, see above): live_patch.rs behind `guest.live-patches` (on in an app with an overlay), called from flush_one with the words that really changed. Functions keep their tables; each changed function's versions (base and overlay pieces) get an x86 stub, written with the process's other threads suspended and none inside the 14 bytes. Needs a base library lifted with nx_rt_dispatcher (a full lift) and refuses an older one.
  UltraCam's present hook: f_70051e5680 installs it at startup, at sdk+0x17f8b4 (index = module count - 1; hook list shows 0x70069a78b4). Under the warmup it was among the lost hooks; it lands now. Whether the function runs on this engine is not checked.
  In the builder since 2026-11-04 (windows.rs): the game without mods is lifted once per engine, game version and settings (work/base, base.from; the C files then go), and what mods change goes in `overlay/` beside guest.dll. Tested with a private APPDATA and the base seeded from a library on disk, so the first build's full lift was not rerun through this path: UltraCam 58 s, a mod of files only 4 s (no overlay), no mods 4 s; each app run as built, on a private save, to frame 900 or 1800. lift_game still takes --patched-image for the CLI.
  UltraCam's handlers, counted in the generated C at the shrine, frame 2400: speed (subsdk3+0x1641c), interval (+0x15fe4, +0x162c4) run each frame; init and the GetProcAddress wrapper (+0x14c4c) once. Never: the present hook (+0x14e84) and limiter (+0x161dc), the camera getter's handler (+0x17d74), +0x16b24. So nothing limits frames to MaxFramerate. The wrapper only keeps one NVN pointer, so what installs the present hook is still unknown; other mods hooking NVN present may meet the same. As built, UltraCam's app runs at 30 fps where the same app without it runs at 60: its interval hook sits where frame_time's was (main+0x8f6670, main+0x7f7a28). Whether that is the overlay or the engine needs the two-pass control above.
  The design, as built: keep the unmodded guest.dll;
 at the end of a mods' init run, diff the code; lift only the mods' modules and the functions holding changed instructions into a small library (about 4 ms a function at that rate, so seconds); point the base functions at it by writing a jmp over their x86 entries, and update the function table rt_dispatch reads. One file sampled (main_021.c, zig cc flags as lift.rs): all 10,200 functions keep their labels, 11,067 direct calls and jumps for 10,895 in the C, so entries are not being inlined away. Check that across the whole library, then mark them noinline if needed. A mid-function hook replaces the whole function, so it needs no entry in the middle. A thread already inside the old function finishes in it: apply before the game runs. Cache by hash of the patched code, so only the first launch with a set of mods compiles. Later writes: recompile on flush and swap entries, needs measuring on a mod that does it. iOS: the mods' code stays baked into the signed image, so exefs changes need a new .ipa; romfs mods could come from Documents without one (not done, ios_env.rs only reads the bundle's).
- **Building .ipa files on Windows (user request, 2026-11-04), on main, nothing run on a device; the history below is in order, see UPDATE.** Works: the iOS engine compiles and links on Windows without Apple's SDK: ld64.lld (`-flat_namespace -undefined suppress`) against generated .tbd stubs that only name each library, C through clang `--target=arm64-apple-ios16.0` with zig's macOS headers (zig's own `-target aarch64-macos` objects are refused: platform macOS). Result: iOS platform, 21 dylibs, 252 imports, parsed by llvm-objdump; whether dyld accepts flat lookup on iOS is unknown. The flags are only in a scratch env so far, not in the builder. adhoc_sign.rs replaces codesign (5 unit tests, signs that engine, llvm-objdump reads it; not checked by Apple's tools or a device). Icons and the zip work without macOS calls.
  BLOCKER: the guest image. nxport::guest_image on Windows has no native rewrites (x18, tpidr, svc, exclusives: cfg(nx_native), module_loader.rs native_rewrites, cpu/trampoline.rs, about 50 cfg sites in the load path, install_native_hooks), so ipa::build refuses off a Mac rather than package code that would crash. Porting it means running those rewrites on x86 with the same trampoline order, TSD offsets and guest base as the iOS engine; a mismatch would only show on a device, so the image should carry a hash of those and the engine check it. Needs an iPhone or a Mac to test. Also not done: Metal shader cache (optional, the app translates as it loads), cli/builder gating.
  **Reduced 2026-11-04 (pushed):** shader modules live only while a pipeline is made, and graphics stages no longer make their own set layout. The list made 20,322 stages for 4,105 distinct translations, so most of that was duplicates (listed.rs stages() still builds a Stage per entry, now without driver objects). Frame 2900, two runs each: shrine commit 8166 -> 7359 MB, working set 4295 -> 3552; Depths working set 6131 -> 5312, commit 11.1 -> 9.9 and 10.6 GB (noisy). List pipelines 4.0 -> 4.5 s on workers. Cold cache, wheel, Depths route fine, no device loss. Correction: the 8.1 vs 9.9 GB gap in the note above compared the shrine save with the Depths save; the Depths takes about 3 GB more in the same build, so most of it is scene. Next candidates: heap committed on demand (up to 1.5 GB of commit, not RAM), the remaining list pipelines (about 850 MB, by subtraction), per-heap numbers from VK_EXT_memory_budget (VRAM not measured yet).
  VK_EXT_memory_budget, after that change, one run each, frame 2700, RTX 4080: shrine VRAM heap 1802 MB and system heap 545 MB (commit 7385); Depths 3913 and 875 MB (commit 10505). So VRAM is not the limit on this card, and vram + system heap (2347 MB) is close to the 2432 MB of write-combined 32 MB blocks: the blocks look like Vulkan's own allocations also counted in the process's commit (by size only, not shown).
  UPDATE 2026-11-04, merged to main: `nxport-cli ipa --dump DIR --out DIR` builds an .ipa on Windows in 14 s, 30 MB, game not bundled, no shader cache, no mods. The guest image no longer blocks: the rewrites run on x86 when an image for iOS is made. Scan of the image: main.text has no svc, mrs tpidr_el0, mrs tpidrro_el0, mrs ctr_el0 or plain ldxr. Same image byte for byte with and without linker folding, after trampolines became per named function (Hle::id, NVN per name): 1,042 bytes differed before that. NOT RUN ON A DEVICE. The engine compares its own patched code with the image at load and refuses a mismatch, in Documents/log.txt. The iOS link is in nxport-cli ios-engine and the builder. plugin mods are left out of iOS apps with a message (user's decision; see Mods on every platform). Merged 2026-11-04 after that full lift and runs passed (shrine, Depths, wheel).
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
