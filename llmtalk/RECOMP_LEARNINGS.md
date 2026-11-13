# What nxport can learn from totk-pc-recomp

Study of https://github.com/valentderah/totk-pc-recomp (v0.4.0, 1006 commits, 2026-10-23 to 11-10) for the user,
2026-11-12, by a win-worker subagent reading its source, docs and commit history (nothing built or run). It is a
static recompiler of TotK to C for x86-64 Windows and Linux with a Vulkan backend and Ryujinx's shader translator. It
supports TotK 1.1.0, 1.2.1 and 1.4.3 (1.4.3 is its main target), so **none of its addresses fit our 1.0.0 / 1.1.2
profiles directly**. Most of its TotK work comes from MacNativeGames (github.com/ly05010419/MacNativeGames, MIT), which
has 1.0.0 sites (`games/totk/runtime/game_version.h`), a 1.1.2 block in its history until 2026-11-02, and
`scripts/map_version_sites.py` to find a site in another version from the instructions around it.

**Licences.** The project is MIT, and so are its upstreams (LAS Recomp, MacNativeGames, nxnative, d2r-mobile,
SavageWindRecomp, Ryujinx shader translator 1.3.2, RenoDX approach, VMA, ImGui); deko3d layout zlib, FSR 1 MIT,
libvpx and zstd BSD-3. **Do not reuse** the ultrawide HUD pane list and distances (`tools/ultrawide_layout.py` and its
data): from the community TOTK-Ultrawide mod, GPL. DLSS is NVIDIA's licence. `docs/maxwell.md` cites yuzu/suyu (GPL)
for behaviour only. Ideas are fine either way.

Status column: what nxport has, from a quick check of our code (verify before relying on it).

## 1. Performance

| # | What they do | Where | nxport |
|---|---|---|---|
| P1 | Counter reads converted with a precomputed integer + 64-bit fraction instead of `(u128)t*num/den` (a `__udivti3` call: a tenth of guest thread time in a fight) | `platform.h` PlatScale; removed `docs/research/windows-host.md` (fb51973) | Same u128 pattern twice per lifted counter read: `clock::ticks_to_ns`, `guest/regs.rs lifted_ticks`. Exact fix; measure the share first. |
| P2 | Proven spin waits (~860 in TotK: loops only another thread can end) get a `pause` on the back-edge | `tools/spin_waits.py` (from nxnative, MIT), `runtime/kernel.c rt_spin_gate` | Lifter emits nothing for `yield` / `wfe` (`lifter/src/insn.rs` ~359). Their 50 us give-away is a timing heuristic: skip it. |
| P3 | Frame-critical threads on P-cores (SetThreadSelectedCpuSets), from the CPU's reported efficiency classes | commits 3a26046, 0d9525b | Apple only (QoS). Windows lacks it. |
| P4 | Native libzstd for the game's ZSTD_decompressMultiFrame / _usingDDict (lifted decoder ~3x slower), fallback to the game's | `runtime/ov_zstd.c` (1.4.3 sites) | Lacking on Windows (lifted). Not needed natively. |
| P5 | Movies decoded on a host worker per decoder, handed over at presentation time; libvpx threads=4 + row MT | `runtime/ov_movie.c` | Our decode was under the global lock on the game's thread: moved out of the lock (b1ea336); a worker per decoder would go further. Their 1.5-period drop is tuned: no. |
| P6 | A draw with a pipeline still compiling is skipped only if its pass's targets were also rendered last frame, else it waits (one-shot draws never lost) | 173462f, a4b46fa, a248cc9 | `vulkan/async_pipe.rs` waits a time budget, then skips: theirs is game state, fits our no-heuristics rule. |
| P7 | VK_EXT_graphics_pipeline_library: stages as libraries, fast-link new states, optimised pipeline in the background; keys seen saved and built at boot | | Not used: the main Windows lever against shader stutter. |
| P8 | Shared shader list (29,587 variants for 1.4.3), prepared at install; cached files validated on load (a driver crash left zero-filled files, dbe8daf) | `tools/pack_shader_cache.py` | We have a shared pipeline list; validate-on-load worth checking. |
| P9 | Re-record only changed dynamic states (752e15b); rebind only changed vertex streams a draw reads (7c5f88d); descriptor update templates and set reuse; recorder queue fields on separate cache lines (b578d26) | | Unchecked. |
| P10 | Windows host: no file I/O on game/render threads (small opens 11-40 ms); save Commit on its own thread (93-135 ms); vkGetPipelineCacheData off the pipeline lock (~60 ms per autosave); VMA blocks allocated ahead on a background thread (8-19 ms each) | windows-host.md, `vk_headroom.c` | Unchecked: worth a look at our save and pipeline-cache threads and allocations. |
| P11 | MEM_WRITE_WATCH + GetWriteWatch for CPU-written pages (no faults); idle pool regions read-only | | We use page protection (`vulkan/guard.rs`): write watch could lower texture-cache cost. |
| P12 | PGO, 6-12% a frame; retrain rules in `docs/pgo.md` (inline helper header changes invalidate a profile silently) | | We have PGO (`build/src/lift.rs`); read their rules. |

## 2. TotK patches and game fixes (their sites are 1.2.1 / 1.4.3 main offsets: locate in 1.0.0 and 1.1.2)

| Fix | Their site | nxport | Exact? |
|---|---|---|---|
| **Equipment-switch cooldown stuck** (weapons / shield / bow locked until reload): countdown at player +0x109c steps only while > FLT_EPSILON, but the gate blocks while > 0; a real-time step leaves a remainder in (0, eps] | 1.2.1 `852d28` (`fmov s1,w8` -> `fmov s1,wzr`); 1.4.3 `2b036c` | Missing; likely affected | Yes |
| **Gear / water-wheel rotation check** (shrine doors at Riogok, Turakamik, the Wind Temple never open above 30 fps): threshold times the AI context's frame delta, as 1.4.3 does | 1.2.1 `21350e0`, `21352d0` | Done (frame_time.rs rotate_check; 1.0.0 main+0x20c31b8, 1.1.2 main+0x2134244) | Yes (Nintendo's 1.4.3 rule) |
| **Map cursor, vertical Ultrahand aim, vertical gyro aim** 2x fast at 60, 4x at 120: each step times the 30 Hz ratio | 1.2.1 `ec3d9c`, `1d48d64`, `1d484e4` | Done (map_pan, gyro_pitch). The Ultrahand stick aim was already scaled by camera_rate (the `1d48d64` path calls it), so only its gyro path is scaled, at the join where both gyro sites read s9 | Yes |
| **Climb speed** (30% at 240 fps on 1.2.1) | 1.2.1 `19182cc`; 1.4.3 `1f497f4` | Skipped: 1.0.0 main+0x18d5b94 and 1.1.2 main+0x191b084 have 1.2.1's rule (1.272 - 0.362(r-1) in [0.5, 1.5], r = ratio times time scale). 1.4.3's factor would change 30 fps play too (slow motion, and frames longer than 1/30 s) and still loses speed above ~77 fps | Partly: 1.4.3's own factor (1.272 + 1.189(1-r), capped 2) is exact but runs out above ~77 fps; their curve below r = 0.5 is fitted to replays: don't copy |
| **Fades stop above ~63.5 fps** (`trunc(2.1*ratio)` = 0): increment floored at 1 | 1.4.3 `0a7ac4`, `95dd24` | Skipped: flooring at 1 is not exact (above ~63.5 fps a fade then runs faster than at 30 fps: 2x at 120); an exact fix needs the fraction kept per counter. Their sites not located in 1.0.0 / 1.1.2 | No |
| Texture pool offsets past 2 GiB widened unsigned (HD texture packs) | 1.2.1 `ad43f0`, `ad440c` | Unknown for 1.0.0 | Yes |
| Fixed sun shadow-map size (foliage shadows jump near the ground) | 1.2.1 `cec22c` | Missing | Optional quality setting (changes output) |
| Ground carry, hair/cloth clock, 60 fps controller, real-time step, 1/240 s shortest step | | We have all | |
| Camera types for direct mouse (cross-check list): Aiming, Telescope, Horse, Purah Pad, ClimbObj, EventTalk, Wakeboard, ShieldSurfing | 1.2.1 | We have exact mouse aim | |

No fix for the 1.0.0 map stamp flicker (they support 1.1.0 and later).

## 3. Other niceties

- Bug-hunting references: `docs/agents/symptoms.md` (symptom -> cause -> first check); `docs/maxwell.md` (Maxwell/NVN vs Vulkan and known TotK bugs: gather rounding on AMD, D16 clear / EQUAL squares, 0.5 border colour for grass, BFE past bit 31, SHF by 0); translator patches `tools/xlate/patches/0012, 0013, 0016` (undefined SPIR-V, Depths terrain): check sass2spv against them.
- Frame critical-path recorder (`TOTK_CRIT`, `tools/crit_path.py`): walks back from each present through the wake chain, charges each interval to a thread and state; fits our CPU-bound hand-off problem.
- Sched trace (sleep asked vs taken), flight recorder, crash reports naming driver frames and C++ exceptions, replay state self-checks.
- Mods: Atmosphère cheats (`runtime/cheats.c`); HD texture packs grow the texture heap and pool from the pack's HeapDef; the mods tab says why a mod is left out. We have no cheats; unknown whether we grow heaps for packs. We are ahead on UltraCam / subsdk plugins.
- UX: mouse in the game's menus and map, wheel zooms the map (`menu_cursor.c`, eui BoxCursorMgr); Xbox prompts with the game's fonts (`tools/button_icons.py`, MIT); right-stick speed scaling the game's turn rates; 0.1% lows; play time, Discord; HDR10, DLSS/DLAA, FSR 1, anisotropic filtering, contact shadows (we lack HDR, DLSS, Discord, play time).
- Process: a performance issue template asking for specific logs; compare medians of three runs, never one against one (one recording varied 31.5-50 fps 1% lows on near-identical builds).

## 4. Do not copy

Three virtual cores with asynchronous preemption (250 us) and spin hand-off after 50 us; real-time step from a 5-sample
moving average; the climb curve below r = 0.5; not-ready occlusion results counted as visible, and sync_ahead (both
change game behaviour); HDR tonemap recognised by constants and names; tuned spin times (kspin_us, rec_spin_us); the
movie 1.5-period drop as written; the recorder thread on AMD (device lost); anything from the GPL ultrawide data.

## First, by value for effort

1. Equipment-cooldown FLT_EPSILON fix for 1.0.0 and 1.1.2 (one instruction a version, exact, a reported bug class).
2. Gear-rotation and cursor / aim fixes (exact; the gear bug blocks shrine progress at high fps). Locate with
   MacNativeGames' 1.0.0 table and the 1.1.2 block in its history.
3. u128 divisions out of `lifted_ticks` / `ticks_to_ns` (fixed-point scale; measure the share in a fight first).
4. Movie decode on a worker per decoder with presentation-time handover (b1ea336 already took decoding out of the lock).
5. Pause on `yield` / `wfe` and proven spin waits in the lifter, then P-core CPU sets for frame-critical threads on
   Windows. Bigger: graphics pipeline libraries plus the state-based skip-or-wait rule against Windows shader stutter.
