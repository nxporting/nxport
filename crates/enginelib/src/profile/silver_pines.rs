// Profile definition for Silver Pines (Unity 6000.3, IL2CPP).

use super::{GameKind, GameProfile, VersionProfile, SILVER_PINES_TITLE_ID};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering::Relaxed};

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "91046036f8850de3ec306056fcc6ba12",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::SilverPines,
    name: "Silver Pines",
    short_name: "silverpines",
    title_id: SILVER_PINES_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_0_0],
    shader_dirs: &[],
    options: None,
    platform_options: None,
    presets: &[],
    module_patches: &[],
    main_thread_tick: Some(texture_size),
};

/// IL2CPP's internal-call lookup in 1.0.0 (il2cpp::vm::InternalCalls::Resolve): a name ("Class::method(signature)")
/// to the engine function, or null. il2cpp_codegen_resolve_icall (main+0x27b9d0) calls it and throws when it fails.
const RESOLVE_ICALL: usize = 0x2d1a50;
const SET_MIPMAP_LIMIT: &[u8] = b"UnityEngine.QualitySettings::set_globalTextureMipmapLimit(System.Int32)\0";
/// IL2CPP's table of internal calls in 1.0.0 (the hash map RESOLVE_ICALL searches): its entry count at +0x10 is 0
/// until Unity registers them (10,577 then).
const ICALL_TABLE: usize = 0x4c29858;
/// The game's one quality level ("Switch", in Data/globalgamemanagers) drops each texture's two largest mip levels.
const GAME_MIPMAP_LIMIT: i32 = 2;

/// The limit last set (-1: none yet; the game's own is in effect) and Unity's setter (0: not resolved yet).
static APPLIED: AtomicI32 = AtomicI32::new(-1);
static SETTER: AtomicUsize = AtomicUsize::new(0);

/// Texture size (graphics.texture_size, the menu's Texture Size): Unity's global texture mipmap limit, set through its
/// own QualitySettings setter on the main thread, where a game's options menu would set it.
fn texture_size() {
    let size = crate::host::live::texture_size();
    let want = if size < 0 { GAME_MIPMAP_LIMIT } else { size };
    let applied = APPLIED.load(Relaxed);
    if applied == want || (applied == -1 && want == GAME_MIPMAP_LIMIT) {
        return;
    }
    if super::active_version().map(|v| v.version_string) != Some(V1_0_0.version_string) {
        return;
    }
    // The first main-thread waits come early in start-up. The lookup needs Unity's internal calls registered (it crashes
    // on the empty table before), and the setter its QualitySettings, which Unity loads from globalgamemanagers before
    // it starts the graphics: both are in place once a frame was presented.
    let registered = unsafe { *((crate::guest::loader::main_base() + ICALL_TABLE + 0x10) as *const u64) };
    if registered == 0 || crate::gpu::try_backend().map_or(0, |b| b.presented_frames()) == 0 {
        return;
    }
    let mut setter = SETTER.load(Relaxed);
    if setter == 0 {
        let resolve: unsafe extern "C" fn(*const u8) -> usize =
            unsafe { std::mem::transmute(crate::guest::loader::main_base() + RESOLVE_ICALL) };
        setter = unsafe { resolve(SET_MIPMAP_LIMIT.as_ptr()) };
        if setter == 0 {
            println!("[!] Silver Pines: Unity's texture mipmap limit setter not found");
            APPLIED.store(want, Relaxed);
            return;
        }
        SETTER.store(setter, Relaxed);
    }
    let set: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(setter) };
    unsafe { set(want) };
    APPLIED.store(want, Relaxed);
    println!("[+] Silver Pines: texture mipmap limit {want}");
}
