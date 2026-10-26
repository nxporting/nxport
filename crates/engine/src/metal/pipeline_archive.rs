// Metal binary archives of the pipeline list's pipelines (<shader cache>/pipelines_*.metalar): compiled GPU code, so a
// listed pipeline costs no compile even the first time the game runs. --build-pipeline-archive writes them from the
// shader cache's pipelines.txt, without running the game: for this Mac's GPU directly, or (--pipeline-archive-script)
// as archives whose functions keep their names plus a pipeline script (pipelines.mtlp-json) that metal-tt compiles for
// another GPU (an iPhone's). A pipeline the archives lack (another GPU or OS compiler) is compiled as before.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::Message;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSArray, NSString, NSURL};
use objc2_metal::{
    MTLBinaryArchive, MTLBinaryArchiveDescriptor, MTLDevice, MTLFunction, MTLLibrary, MTLPipelineOption, MTLPixelFormat, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLVertexFormat,
};

use crate::gpu::pipeline_list::Entry;
use super::shader_cache::{by_entry, CachedStage};
use super::translated::{derive_layout, pf_kind, pipeline_descriptor, pipeline_key, ShaderLibs};
use super::Device;

type Pipeline = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;

struct Archives(Retained<NSArray<ProtocolObject<dyn MTLBinaryArchive>>>);
// SAFETY: binary archives are thread-safe Metal objects; the array is never mutated after loading.
unsafe impl Send for Archives {}
unsafe impl Sync for Archives {}

static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);

/// The archive at `path`, or a new empty one.
fn open(device: &Device, path: Option<&Path>) -> Result<Retained<ProtocolObject<dyn MTLBinaryArchive>>, String> {
    let desc = MTLBinaryArchiveDescriptor::new();
    if let Some(path) = path {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        desc.setUrl(Some(&NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))));
    }
    device.newBinaryArchiveWithDescriptor_error(&desc).map_err(|e| e.localizedDescription().to_string())
}

static ARCHIVES: OnceLock<Option<Archives>> = OnceLock::new();

/// The shader cache's archives, once loaded (draws never wait for them: until then pipelines compile as before).
fn archives(_device: &Device) -> Option<&'static Archives> {
    ARCHIVES.get().and_then(|a| a.as_ref())
}

/// Loads the shader cache's archives, waiting if another thread is loading them.
pub(super) fn load(device: &Device) -> bool {
    ARCHIVES
        .get_or_init(|| {
            if !crate::settings::get().graphics.pipeline_archive {
                return None;
            }
            let t0 = std::time::Instant::now();
            let dir = super::shader_cache::cache_dir();
            let mut paths: Vec<_> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("pipelines") && n.ends_with(".metalar"))
                })
                .collect();
            paths.sort();
            // Opening an archive parses every entry: one thread per file.
            let opened: Vec<Option<SendArchive>> = std::thread::scope(|s| {
                let workers: Vec<_> = paths
                    .iter()
                    .map(|p| {
                        let d = SendDevice(device.clone());
                        s.spawn(move || {
                            let d = d;
                            match open(&d.0, Some(p)) {
                                Ok(a) => Some(SendArchive(a)),
                                Err(e) => {
                                    println!("[!] Pipelines: archive {}: {e}", p.display());
                                    None
                                }
                            }
                        })
                    })
                    .collect();
                workers.into_iter().map(|w| w.join().ok().flatten()).collect()
            });
            let bytes: u64 = paths.iter().map(|p| std::fs::metadata(p).map_or(0, |m| m.len())).sum();
            let list: Vec<_> = opened.into_iter().flatten().map(|a| a.0).collect();
            if list.is_empty() {
                return None;
            }
            println!(
                "[+] Pipelines: {} binary archives ({} MB) loaded in {:.1} s",
                list.len(),
                bytes >> 20,
                t0.elapsed().as_secs_f64()
            );
            Some(Archives(NSArray::from_retained_slice(&list)))
        })
        .is_some()
}

/// Starts loading the archives in the background.
pub(super) fn preload(device: &Device) {
    let device = device.clone();
    let d = SendDevice(device);
    std::thread::Builder::new()
        .name("pipe-archive".into())
        .spawn(move || {
            let d = d;
            load(&d.0);
        })
        .expect("spawn pipeline archive thread");
}

struct SendArchive(Retained<ProtocolObject<dyn MTLBinaryArchive>>);
// SAFETY: binary archives are thread-safe Metal objects.
unsafe impl Send for SendArchive {}

struct SendDevice(Device);
// SAFETY: MTLDevice is thread-safe.
unsafe impl Send for SendDevice {}

/// A library function specialised with `constants`, from the archives when they have it.
pub(super) fn function(
    lib: &ProtocolObject<dyn objc2_metal::MTLLibrary>,
    name: &str,
    constants: &objc2_metal::MTLFunctionConstantValues,
) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, Retained<objc2_foundation::NSError>> {
    let d = objc2_metal::MTLFunctionDescriptor::new();
    d.setName(Some(&NSString::from_str(name)));
    d.setConstantValues(Some(constants));
    BUILDING.with(|b| {
        if let Some(list) = b.borrow_mut().as_mut() {
            list.push((d.clone(), lib.retain()));
        }
    });
    if let Some(a) = archives(&lib.device()) {
        d.setBinaryArchives(Some(&a.0));
    }
    lib.newFunctionWithDescriptor_error(&d)
}

type Specialised = (Retained<objc2_metal::MTLFunctionDescriptor>, Retained<ProtocolObject<dyn MTLLibrary>>);

thread_local! {
    /// --build-pipeline-archive: the specialised functions a pipeline descriptor was made of, added to the archive
    /// with it (an archive serves a specialised function only when it holds it).
    static BUILDING: std::cell::RefCell<Option<Vec<Specialised>>> = const { std::cell::RefCell::new(None) };
}

/// Creates a pipeline, from the archives when they have it.
pub(super) fn create(device: &Device, desc: &MTLRenderPipelineDescriptor, vs: &str, fs: &str) -> Option<Pipeline> {
    if let Some(a) = archives(device) {
        desc.setBinaryArchives(Some(&a.0));
        if let Ok(p) = device.newRenderPipelineStateWithDescriptor_options_reflection_error(
            desc,
            MTLPipelineOption::FailOnBinaryArchiveMiss,
            None,
        ) {
            HITS.fetch_add(1, Ordering::Relaxed);
            return Some(p);
        }
        MISSES.fetch_add(1, Ordering::Relaxed);
    }
    match device.newRenderPipelineStateWithDescriptor_error(desc) {
        Ok(p) => Some(p),
        Err(e) => {
            println!("[!] Gfx: translated pipeline {vs} / {fs}: {}", e.localizedDescription());
            None
        }
    }
}

/// Pipelines found in the archives and not, when there are archives.
pub(super) fn stats() -> Option<(u64, u64)> {
    let (h, m) = (HITS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed));
    (h + m > 0).then_some((h, m))
}

/// A list entry's stages and descriptor inputs.
struct Planned {
    entry: Entry,
    vs: &'static CachedStage,
    fs: Option<&'static CachedStage>,
    /// Function constant values of the two specialised functions (for the script: one specialisation per name and
    /// archive keeps the archive's function names).
    vs_spec: Vec<u64>,
    fs_spec: Vec<u64>,
}

fn plan(entries: Vec<Entry>) -> (Vec<Planned>, usize) {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut skipped = 0;
    for e in entries {
        let (Some(vs), fs) = (by_entry(&e.vs), e.fs.as_deref().map(by_entry)) else {
            skipped += 1;
            continue;
        };
        let fs = match fs {
            Some(None) => {
                skipped += 1;
                continue;
            }
            Some(Some(f)) => Some(f),
            None => None,
        };
        let color_pfs = e.color_pfs.map(|f| MTLPixelFormat(f as _));
        let depth_pf = MTLPixelFormat(e.depth_pf as _);
        let l = derive_layout(vs, &e.attr_bits, &e.streams);
        let key = pipeline_key(
            f32::from_bits(e.point_size),
            vs,
            fs,
            e.depth_gl,
            e.color_state,
            e.channel_mask,
            depth_pf,
            &color_pfs,
            &e.blends,
            &l.attr_descs,
            &l.streams_used,
            &l.streams_eff,
        );
        if !seen.insert((key.to_vec(), e.y_negate)) {
            continue;
        }
        let mut vs_spec: Vec<u64> = l.attr_descs.iter().flat_map(|a| [a.0 as u64, a.4 as u64]).collect();
        vs_spec.extend([e.depth_gl as u64, e.y_negate as u64, e.point_size as u64]);
        let fs_spec = fs.map_or(Vec::new(), |f| {
            let mut s: Vec<u64> = f
                .colors
                .iter()
                .map(|&c| pf_kind(color_pfs.get(c as usize).copied().unwrap_or(MTLPixelFormat::Invalid)) as u64)
                .collect();
            s.push((depth_pf != MTLPixelFormat::Invalid) as u64);
            s.extend(f.varyings.iter().map(|v| vs.varyings.contains(v) as u64));
            s
        });
        out.push(Planned { entry: e, vs, fs, vs_spec, fs_spec });
    }
    (out, skipped)
}

fn descriptor(device: &Device, lib: &ShaderLibs, p: &Planned) -> Option<Retained<MTLRenderPipelineDescriptor>> {
    let e = &p.entry;
    let l = derive_layout(p.vs, &e.attr_bits, &e.streams);
    pipeline_descriptor(
        device,
        lib,
        p.vs,
        p.fs,
        &l.attr_descs,
        &l.streams_used,
        &l.streams_eff,
        &e.color_pfs.map(|f| MTLPixelFormat(f as _)),
        MTLPixelFormat(e.depth_pf as _),
        e.depth_gl,
        e.y_negate,
        f32::from_bits(e.point_size),
        &e.blends,
        e.color_state,
        e.channel_mask,
        false,
    )
}

/// The pipeline script entry of a descriptor (enum values as numbers), its functions taken from library `label`.
fn script_entry(desc: &MTLRenderPipelineDescriptor, label: &str) -> String {
    let name = |f: Option<Retained<ProtocolObject<dyn objc2_metal::MTLFunction>>>| {
        f.map(|f| format!("\"alias:{label}#{}\"", f.name()))
    };
    let mut fields = Vec::new();
    if let Some(v) = name(desc.vertexFunction()) {
        fields.push(format!("\"vertex_function\": {v}"));
    }
    if let Some(f) = name(desc.fragmentFunction()) {
        fields.push(format!("\"fragment_function\": {f}"));
    }
    fields.push(format!("\"rasterization_enabled\": {}", desc.isRasterizationEnabled()));
    unsafe {
        let vd = desc.vertexDescriptor();
        let attrs: Vec<_> =
            vd.iter().flat_map(|vd| (0..31usize).map(|i| vd.attributes().objectAtIndexedSubscript(i))).collect();
        let last_attr = attrs.iter().rposition(|a| a.format() != MTLVertexFormat::Invalid);
        let layouts: Vec<_> =
            vd.iter().flat_map(|vd| (0..31usize).map(|i| vd.layouts().objectAtIndexedSubscript(i))).collect();
        let last_layout = layouts.iter().rposition(|l| l.stride() != 0 || l.stepRate() != 1 || l.stepFunction().0 != 1);
        if last_attr.is_some() || last_layout.is_some() {
            let a: Vec<String> = attrs[..last_attr.map_or(0, |i| i + 1)]
                .iter()
                .map(|a| {
                    format!(
                        "{{\"format\": {}, \"offset\": {}, \"buffer_index\": {}}}",
                        a.format().0,
                        a.offset(),
                        a.bufferIndex()
                    )
                })
                .collect();
            let l: Vec<String> = layouts[..last_layout.map_or(0, |i| i + 1)]
                .iter()
                .map(|l| {
                    format!(
                        "{{\"stride\": {}, \"step_function\": {}, \"step_rate\": {}}}",
                        l.stride(),
                        l.stepFunction().0,
                        l.stepRate()
                    )
                })
                .collect();
            fields.push(format!(
                "\"vertex_descriptor\": {{\"attributes\": [{}], \"layouts\": [{}]}}",
                a.join(", "),
                l.join(", ")
            ));
        }
        let cas: Vec<_> = (0..8usize).map(|i| desc.colorAttachments().objectAtIndexedSubscript(i)).collect();
        let last = cas.iter().rposition(|c| c.pixelFormat() != MTLPixelFormat::Invalid);
        let c: Vec<String> = cas[..last.map_or(0, |i| i + 1)]
            .iter()
            .map(|c| {
                format!(
                    "{{\"pixel_format\": {}, \"blending_enabled\": {}, \"write_mask\": {}, \"source_rgb_blend_factor\": {}, \
                     \"destination_rgb_blend_factor\": {}, \"rgb_blend_operation\": {}, \"source_alpha_blend_factor\": {}, \
                     \"destination_alpha_blend_factor\": {}, \"alpha_blend_operation\": {}}}",
                    c.pixelFormat().0,
                    c.isBlendingEnabled(),
                    c.writeMask().0,
                    c.sourceRGBBlendFactor().0,
                    c.destinationRGBBlendFactor().0,
                    c.rgbBlendOperation().0,
                    c.sourceAlphaBlendFactor().0,
                    c.destinationAlphaBlendFactor().0,
                    c.alphaBlendOperation().0
                )
            })
            .collect();
        fields.push(format!("\"color_attachments\": [{}]", c.join(", ")));
    }
    fields.push(format!("\"depth_attachment_pixel_format\": {}", desc.depthAttachmentPixelFormat().0));
    fields.push(format!("\"stencil_attachment_pixel_format\": {}", desc.stencilAttachmentPixelFormat().0));
    format!("{{{}}}", fields.join(", "))
}

/// --build-pipeline-archive: compiles the shader cache's pipeline list into binary archives in `out`.
pub fn build(out: &Path, script: bool) -> Result<(), String> {
    let device: Device = objc2_metal::MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
    let lib = super::translated::load_shader_lib(&device).ok_or("no shader libraries")?;
    let list = super::shader_cache::cache_dir().join("pipelines.txt");
    let text = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let (planned, skipped) = plan(text.lines().filter_map(Entry::parse).collect());
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // Groups: one archive each. For the script, a function name has one specialisation per archive (the archive then
    // keeps it under its name, which the script refers to); otherwise one archive per thread.
    // Archives load one per thread, as do builds: as many as the CPU has cores.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut groups: Vec<Vec<usize>> = Vec::new();
    if script {
        let mut used: Vec<HashMap<&str, &Vec<u64>>> = Vec::new();
        for (i, p) in planned.iter().enumerate() {
            let fits = |m: &HashMap<&str, &Vec<u64>>| {
                m.get(p.vs.entry.as_str()).is_none_or(|s| **s == p.vs_spec)
                    && p.fs.is_none_or(|f| m.get(f.entry.as_str()).is_none_or(|s| **s == p.fs_spec))
            };
            let g = match used.iter().position(|m| fits(m)) {
                Some(g) => g,
                None => {
                    used.push(HashMap::new());
                    groups.push(Vec::new());
                    used.len() - 1
                }
            };
            used[g].insert(&p.vs.entry, &p.vs_spec);
            if let Some(f) = p.fs {
                used[g].insert(&f.entry, &p.fs_spec);
            }
            groups[g].push(i);
        }
    } else {
        groups = (0..threads).map(|t| (t..planned.len()).step_by(threads).collect()).collect();
        groups.retain(|g| !g.is_empty());
    }
    println!(
        "[+] Pipelines: {} distinct pipelines ({} entries without their shaders) in {} archives",
        planned.len(),
        skipped,
        groups.len()
    );
    let t0 = std::time::Instant::now();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed = AtomicU64::new(0);
    let entries: Vec<Vec<String>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.min(groups.len()))
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let g = next.fetch_add(1, Ordering::Relaxed);
                        let Some(group) = groups.get(g) else { break };
                        let path = out.join(format!("pipelines_{g:04}.metalar"));
                        let archive = match open(&device, None) {
                            Ok(a) => a,
                            Err(e) => {
                                println!("[!] Pipelines: archive: {e}");
                                failed.fetch_add(group.len() as u64, Ordering::Relaxed);
                                continue;
                            }
                        };
                        let mut lines = Vec::new();
                        for &i in group {
                            let p = &planned[i];
                            BUILDING.with(|b| *b.borrow_mut() = Some(Vec::new()));
                            let desc = descriptor(&device, &lib, p);
                            let functions = BUILDING.with(|b| b.borrow_mut().take()).unwrap_or_default();
                            let Some(desc) = desc else {
                                failed.fetch_add(1, Ordering::Relaxed);
                                continue;
                            };
                            // (Not for the script: the function would join the archive under its name next to the
                            // pipeline's own specialisation, which then gets another name.)
                            for (fd, flib) in functions.iter().filter(|_| !script) {
                                if let Err(e) = archive.addFunctionWithDescriptor_library_error(fd, flib) {
                                    println!("[!] Pipelines: function: {}", e.localizedDescription());
                                }
                            }
                            if let Err(e) = archive.addRenderPipelineFunctionsWithDescriptor_error(&desc) {
                                println!("[!] Pipelines: {} / {}: {}", p.entry.vs, p.entry.fs.as_deref().unwrap_or("-"), e.localizedDescription());
                                failed.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            if script {
                                lines.push(script_entry(&desc, &format!("g{g}")));
                            }
                        }
                        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                        if let Err(e) = archive.serializeToURL_error(&url) {
                            println!("[!] Pipelines: {}: {}", path.display(), e.localizedDescription());
                            failed.fetch_add(group.len() as u64, Ordering::Relaxed);
                            continue;
                        }
                        done.push((g, lines));
                    }
                    done
                })
            })
            .collect();
        let mut all: Vec<(usize, Vec<String>)> = workers.into_iter().flat_map(|w| w.join().unwrap()).collect();
        all.sort_by_key(|(g, _)| *g);
        let mut by_group = vec![Vec::new(); groups.len()];
        for (g, l) in all {
            by_group[g] = l;
        }
        by_group
    });
    if script {
        let libs: Vec<String> = (0..groups.len())
            .filter(|&g| out.join(format!("pipelines_{g:04}.metalar")).exists())
            .map(|g| format!("{{\"label\": \"g{g}\", \"path\": \"pipelines_{g:04}.metalar\"}}"))
            .collect();
        let pipelines: Vec<&String> = entries.iter().flatten().collect();
        let text = format!(
            "{{\"version\": {{\"major\": 0, \"minor\": 3, \"sub_minor\": 0}},\n\"libraries\": {{\"paths\": [\n{}\n]}},\n\"pipelines\": {{\"render_pipelines\": [\n{}\n]}}}}\n",
            libs.join(",\n"),
            pipelines.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",\n")
        );
        std::fs::write(out.join("pipelines.mtlp-json"), text).map_err(|e| e.to_string())?;
    }
    println!(
        "[+] Pipelines: archives written to {} in {:.1} s ({} failed)",
        out.display(),
        t0.elapsed().as_secs_f64(),
        failed.load(Ordering::Relaxed)
    );
    Ok(())
}
