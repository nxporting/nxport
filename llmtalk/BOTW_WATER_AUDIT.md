# BotW water / floating / milky water / flicker: Savage Wind against nxport (audit, 2026-11-15)

Read-only audit. Savage Wind (SW) paths are under `nxport-build/ref/savagewind`, nxport paths under `crates/engine/src`.
Everything already settled in SAVAGEWIND_LEARNINGS.md or ruled out in the brief is left out.

## Ranked differences

| # | Difference | Symptoms | Backends | Plausibility |
|---|---|---|---|---|
| 1 | `--early-fence` (on by default) gives the game its frame fence before the GPU work and the linear-target write-backs are done. SW waits for real on first reads of a target and on 1x1 targets after a load | 1, 2, 3, maybe 4 | Vulkan (Metal's rt_guard covers it) | High |
| 2 | CPU writes to a linear target's memory made between the write-back and the next submit's `retire` go into the baseline (`resettle`): the image never gets them, and the next copy back overwrites them | 1, 3 | Vulkan | High (exact read of the code) |
| 3 | Frame-late write-back of non-linear targets into memory that may belong to other data by then (only the pool being alive is checked) | 1, 3, 4 | Vulkan; Metal only with rt_guard off | Medium |
| 4 | Linear write-backs deduplicated by storage address: two textures at one address in one submit write back the first one | 1 | Vulkan | Medium-low |
| 5 | Feedback copies are shared by "same program in a row" and only renewed by INVALIDATE_TEXTURE barriers. SW reads the layers below the one being written live, with no copy | 1 (Windows variant), 4 | Vulkan (copies) | Medium-low |
| 6 | The D16 twin of the R16 shadow copy may never get the R16 image's GPU contents (the depth upload route skips `flush_overlaps`) | 2, 4 (water / terrain shadows) | Vulkan | Medium, unverified |
| 7 | Draws into arrays skipped while their pipeline compiles. SW makes them wait (`draw_must_happen`); nxport waits only when the target was not drawn last frame | 1 | both, async only | Low (async is off by default and in the user's settings) |
| - | Shader translation (sass2spv is Ryujinx's translator, and Ryujinx swims), occlusion reports (report_guard is exact), ReadTexels (SW leaves it unimplemented, so BotW does not need it), libm (not HLE'd: nxport HLEs only mem*/str*) | - | - | Low: not differences |

The Mac report (1.5.0, "stands on the pond floor", with rt_guard on or off) points to a shared cause. Items 1-6 are
Vulkan-only, and they explain the Windows symptoms: water too high, milky water. No shared cause of the same strength was
found in the shared code (nvn/api.rs, sass2spv, horizon). The Mac case may be a separate bug: section 8.

## 1. Early fence: the game reads GPU results before they exist

nxport, `vulkan/mod.rs:332-351`, the `wait_fence` call on guest-main:
```
let prev = PREV.load(..); if prev < serial && !inflight::wait(prev, ..) { return false; }
PREV.fetch_max(serial, ..); if inflight::completed() >= serial { guard::release_all(); return true; }
guard::protect(serial); true
```
`settings/mod.rs:272` has `early_fence = true`, and the user's BotW settings do not change it. `vulkan/guard.rs:19-20`
says it outright: "Pages GPU work writes ... are never protected ... The game reads such results a frame late." The
linear targets are written by the watcher only once the submit completes (`inflight.rs:104-110`), and their pages are left
out of the guard on purpose (`exec.rs:1179`, `guard::note_write`). So on BotW's main thread, which waits on its frame
fence in nvnSyncWait, as SW notes at `exec.c:731`:
- The height and water tiles drawn this frame are read with what guest memory held before. For a tile the game has just
  made again at the same memory as terrain streams in (SW `exec.c:1538`), that is the previous tile's heights, from
  another place. Water from another place is "pulled up that high". A wrong ground height under objects pins them to walls
  or floats them (symptom 3).
- The 1x1 / 3x3 light and exposure targets arrive a frame late. SW #362 / #332 (`exec.c:1735-1742`): read late in their
  first reads after a load, the water stays milky white until the next load. They fixed it by waiting for the first 30
  reads after a loading screen. Symptom 2 matches exactly.
- CopyTextureToBuffer (`vulkan/copy.rs:149-162`) and storage-buffer results arrive late as well. SW makes
  CopyTextureToBuffer synchronous (`exec.c:9064-9078`: `renderer_flush(); ... vk_readback_sync(); memcpy`).

SW has the same overlap, with guards (`exec.c:1743-1764`, `808-850`): a target not read back within the last 16 frames
counts as "one-off", and a wait on its submission is real (only up to that submission). So are the first 30 reads of
targets of 16 texels or fewer after a load. Mali also gets query reports on time.

Test (cheap): BotW with `--early-fence=false` at the user's spot near water. If the floating and the milky water are
gone, the exact fix is to make guest-main's wait real when the submit has linear-target write-backs (or CopyTextureToBuffer
/ storage write-backs) whose memory the game reads. The rule can be exact: wait up to that submit only, as SW's
`renderer_wait_seq` does. It need not be SW's 16-frame / 30-read thresholds. Simplest exact version: hold the linear
targets' pages under the write guard until written back, as Metal's rt_guard does.

## 2. CPU writes to linear-target memory lost after a write-back (`resettle`)

SW `exec.c:1517-1557` (`linear_target_sync`): "the CPU fills BOTW's height tiles in memory it also has the GPU draw
into; copying the whole image back over them lost those writes". The guest bytes' hash is taken in `readbacks_apply`
right after its own memcpy (`exec.c:1683-1686`). Before each pass that draws into the target, a different hash means the
CPU wrote, and the image is reloaded from guest memory.

nxport: the write-back lands on the watcher thread (`inflight.rs:105-109`, `dirty_track::write_unseen`). The baseline is
taken again only when the vk thread retires the submit, at the start of the next submit (`exec.rs:820` `retire`, then
`exec.rs:1086-1089` `self.tex.resettle(d)`, then `texture.rs:1044-1058` `settle_one`, then `take_baseline`, which hashes
whatever memory holds now). A CPU write in between (a whole frame under the early fence, still a window without it) is
taken as the image's own contents ("The image's own contents: not a CPU write"). `refresh` (`texture.rs:433-500`) then
sees nothing changed. The image lacks the CPU's texels, the next partial draw into the tile is made over the stale image,
and `record_rt_reads` copies the whole level back over the CPU's data. CPU writes made after `settle` and before the
watcher's write is done are overwritten outright. SW keeps the same "pending" window but reads only after real waits.

Test: in `resettle`, compare guest memory with the bytes the watcher wrote (hash them in `finish` after `write_unseen`)
and log any mismatch, by address and size. Any hit on a 192x192 / 256x256 R16 tile confirms the bug. Fix: take the
baseline from the written-back bytes, in the watcher (SW's order), not from memory at retire time.

## 3. Frame-late write-back of non-linear targets

`exec.rs:1123` (`note_rendered`): non-linear targets of 64 KiB or less, or not drawn in the last 4 frames, are copied at
the next submit's start (`write_back_rts`, then `record_rt_reads` at `exec.rs:870`) and written by the watcher with
`write_unseen`. The only check is that the memory lies in a live pool (`exec.rs:1162-1165`). BotW frees and remakes
textures in pool memory as it streams. A frame later, that memory may hold CPU-written data: water / height source data,
vertices. It is then overwritten with stale texels, and since the write is "unseen", nothing that caches that memory
notices. Metal hit the same thing in TotK (`metal/mod.rs:7276-7283`, a list node overwritten with texels) and moved to
rt_guard. SW writes back only linear targets.

Test: `--no-rt-writeback` (`settings/mod.rs:326`; check that Vulkan honours it, since `grep no_rt_writeback vulkan/`
finds nothing). Or log write-backs whose texture object no longer has that storage address or format at write time.

## 4. Linear write-backs deduplicated by storage

`exec.rs:1119-1121`: `if !self.pending_rt_reads.iter().any(|w| w.storage == t.storage) { push }`. A second texture at the
same address in the same submit (another size or format, such as tiles made again during streaming) is dropped, and the
first one's image is written back with its final contents. SW copies after every pass that drew a linear target, in GPU
order (`exec.c:2028-2029`, `readback_schedule` from `vk_end_pass`), and before drawing it waits for a copy back from
another image at the same memory (`exec.c:1560-1571`, `2105-2109`). Test: log when a pending linear entry and a new one
share storage but differ in Desc.

## 5. Feedback copies for array layers

SW `exec.c:4255-4275`: the terrain fills layer n of its height and water arrays from layer n-1, sampling the array it
renders into. SW binds a live view of the layers below the target layer: no copy, and it always sees what earlier passes
wrote. nxport (`draw.rs:1376-1412`) copies every level and layer once per feedback epoch. The epoch moves only at a
barrier with INVALIDATE_TEXTURE (`exec.rs:1400-1408`) or when another program draws after a write (`draw.rs:400-411`). If
the game fills layer n, then n+1, with the same program and no INVALIDATE_TEXTURE barrier in between, layer n+1 samples
the copy made before layer n was drawn. Then: zero or stale heights, plus the copy's cost. The terrain's G-buffer OR pass
(~200 same-program draws reading their own pixel, `exec.c:4277-4290`) also loses earlier draws' bits, a lighting /
flicker candidate. Metal samples in place (`metal/translated.rs:3723-3760`), so it is not affected. Test: in a frame
journal near water, check whether successive passes into different layers of one array show the same "snapshot ...
epoch" line. Or make the epoch move whenever the pass's target layer changes, and compare.

## 6. Depth twin of the R16 shadow copy

`draw.rs:1219-1229` samples a colour texture under a shadow sampler through a D16 / D32F image over the same memory
(`layout::depth_twin`). That image is a separate Desc, filled only by `refresh`. For depth images `uploads` is false
(`texture.rs:472`), so `flush_overlaps` is skipped, but `upload` still runs (`texture.rs:497`) from guest memory. That
memory does not hold the R16 copy's GPU rendering, because the copy is GPU-only and drawn every frame. Unless something
outside `refresh` copies it (not found), water and terrain shadows use a stale twin. SW copies image to image after every
pass that draws the R16 copy (`exec.c:1806-1820`, `depth_twin_after_pass`). Test: dump the twin and the R16 image in one
frame (pass dump) and compare.

## 7. Draws skipped while pipelines compile

SW (docs/SAVAGE-WIND.md:1457-1464): BotW copies each loaded terrain tile into its layer of the height / normal / material
arrays once, so a skipped draw left wrong data there for good. Draws into array / 3D / cube targets always wait.
nxport (`draw.rs:664-668`, `exec.rs:1134-1139`) waits only if no target of the pass was drawn last frame, keyed by
`t.storage`. One layer of the array drawn last frame then lets this frame's draw into another layer be skipped. This
matters only with `async-pipelines` on: the default is off, and the user's BotW settings set nothing. Metal has the same
rule (`metal/translated.rs:5783`).

## 8. What is not a difference, and the Mac case

- Occlusion: nxport reads results exactly (`inflight.rs:84-95`, report_guard on by default). SW gets them on time only
  where it waits.
- ReportValue: SW leaves it unimplemented (`cmdbuf.c:721`). nxport writes it at record time.
- ReadTexels: SW logs "not implemented" (`objects.c:900`), so BotW's play does not depend on it.
- Map / Flush / Invalidate: no-ops in both, except SW's `nvnBufferFlushMappedRange`, which marks the range written
  (`objects.c:484`). nxport finds CPU writes by page tracking, which serves the same purpose.
- HLE: SW HLEs only longjmp and its own features. nxport HLEs mem* / str* / printf (`horizon/sdk/base/libc.rs`), and its
  libm is the game's own. Havok sees no HLE math.
- sass2spv: the array index is read as S32 and converted for float lookups (`codegen/instructions.rs:1036-1043`), as in
  Ryujinx. Ryujinx swims, so nothing points at translation.
- The Mac (1.5.0) shows the opposite water symptom (too low), with rt_guard exact. Items 1-6 do not reach it. To check
  with mac-worker: (a) whether a CPU write to a linear-target page after rt_guard's write-back gets into the MTLTexture
  before the next draw into it (the item 2 class: SW's `linear_target_sync`); (b) whether a texture made again at the same
  memory (new image) is loaded from guest memory before a partial draw (SW: "the copy back of a new, empty image put zeros
  and stale texels into the rest, and Link stood in the ground there"). "Stands on the pond floor" fits (b): water heights
  of 0.

## Order to test

1. `--early-fence=false` near water (Windows): floating and milky water. One run.
2. The `resettle` mismatch log (item 2). One build, one run.
3. Write-back off, or the stale write-back log (item 3). Check that the setting reaches Vulkan.
4. A frame journal near water for items 4-6.
