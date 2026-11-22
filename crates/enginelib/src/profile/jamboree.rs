// Profile definition for Super Mario Party Jamboree.

use super::{GameKind, GameProfile, JAMBOREE_TITLE_ID};

fn options() -> crate::settings::SettingsPatch {
    let mut s = crate::settings::SettingsPatch::default();
    // Its background task queue (main+0x5dcba0 in 2.3.0) is posted to by more than one thread without a lock: one thread
    // clears the pending list while another still uses it, a null write on 'Bezel:BackgroundTask'. Threads that share a
    // core cannot run it at once on the console.
    s.guest.guest_cores = Some(true);
    s
}

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::MarioPartyJamboree,
    name: "Super Mario Party Jamboree",
    short_name: "jamboree",
    title_id: JAMBOREE_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[],
    shader_dirs: &[],
    options: Some(options),
    platform_options: None,
    presets: &[],
    module_patches: &[],
    main_thread_tick: None,
    mods: &[],
};
