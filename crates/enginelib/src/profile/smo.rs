// Profile definition for Super Mario Odyssey.

use super::{GameKind, GameProfile, VersionProfile, SMO_TITLE_ID};

pub static V1_4_1: VersionProfile = VersionProfile {
    version_string: "1.4.1",
    build_id_prefix: "b424be150a8e7d78701cbe7a439d9ebf",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

fn options() -> crate::settings::SettingsPatch {
    let mut s = crate::settings::SettingsPatch::default();
    // On a first boot (no save game) its scene-init thread, InitSystem, WorldResourceLoader and the main thread, all on
    // the process default core, use sead heaps without locks: run at once they damage the heap lists (ExpHeap tryAlloc
    // halt, Heap::destruct_ crash), 4-5 of 12 fresh starts. The console never runs threads of one core at once.
    s.guest.guest_cores = Some(true);
    s
}

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::SuperMarioOdyssey,
    name: "Super Mario Odyssey",
    short_name: "smo",
    title_id: SMO_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_4_1],
    shader_dirs: &["ShaderData", "EffectData", "LayoutData", "SystemData", "ObjectData"],
    options: Some(options),
    platform_options: None,
    presets: &[],
    module_patches: &[],
    main_thread_tick: None,
    mods: &[],
};
