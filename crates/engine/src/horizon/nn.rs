// HLE for nn:: SDK entry points that would otherwise run sdk module code needing the kernel
// (svc) or a sysmodule session (IPC): nn::os Event / MultiWait / MessageQueue / reader-writer
// locks, nn::time, nn::oe, nn::pl, nn::account, nn::err, nn::diag, nn::swkbd, nn::nfp, nn::hid
// (Npad/six-axis reads) and nn::prepo.
//
// Functions are bound
// by mangled name from the loader (see `lookup`), so they override the SDK symbol table.
//
// Synchronisation objects live in side tables keyed by the guest object's address, protected by
// one global mutex + condvar. That keeps them independent of the SDK's internal layouts (we never
// need to match sizeof/offsets) and makes cross-object waits (MultiWait) trivial.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};


pub(crate) const RESULT_SUCCESS: u32 = 0;

// ── global sync state ────────────────────────────────────────────────────────────────────────
#[derive(Default)]
pub(crate) struct EventSt {
    pub(crate) signaled: bool,
    pub(crate) auto_clear: bool,
    /// Signals so far: a manual-clear waiter returns once one arrived after it began, even if cleared again since.
    pub(crate) signals: u64,
}

#[derive(Default)]
pub(crate) struct MsgQ {
    pub(crate) cap: usize,
    pub(crate) q: VecDeque<u64>,
}

#[derive(Default)]
pub(crate) struct RwSt {
    readers: u32,
    writer: usize, // owning thread token, 0 = none
    write_depth: u32,
}

#[derive(Default)]
pub(crate) struct Sync {
    pub(crate) events: HashMap<usize, EventSt>,
    pub(crate) holders: HashMap<usize, usize>, // holder addr -> event addr
    pub(crate) multiwaits: HashMap<usize, Vec<usize>>, // multiwait addr -> ordered holder addrs
    pub(crate) msgqs: HashMap<usize, MsgQ>,
    pub(crate) rwlocks: HashMap<usize, RwSt>,
}

static SYNC: Mutex<Option<Sync>> = Mutex::new(None);
pub(crate) static SYNC_CV: Condvar = Condvar::new();

pub(crate) fn lock() -> MutexGuard<'static, Option<Sync>> {
    let mut g = SYNC.lock().unwrap();
    if g.is_none() {
        *g = Some(Sync::default());
    }
    g
}

fn me() -> usize {
    unsafe { crate::horizon::os::get_current_thread() as usize }
}

/// Signals an nn::os::EventType the game initialised through the HLE (for HLE'd libraries that report to the game).
pub(crate) fn signal_event(ev: usize) {
    unsafe { crate::horizon::os::signal_event(ev as *mut u8) }
}

/// Waits for the event; a signaled auto-clear event is consumed. `deadline` None = forever. Returns whether it was
/// signaled. An event nothing initialised (no side-table entry) never signals.
pub(crate) fn event_wait(ev: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    let signals0 = g.as_ref().unwrap().events.get(&ev).map(|e| e.signals);
    loop {
        if let Some(e) = g.as_mut().unwrap().events.get_mut(&ev) {
            if e.signaled {
                if e.auto_clear {
                    e.signaled = false;
                }
                return true;
            }
            if !e.auto_clear && signals0.is_some_and(|n| n != e.signals) {
                return true;
            }
        }
        match deadline {
            None => g = SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                g = SYNC_CV.wait_timeout(g, d - now).unwrap().0;
            }
        }
    }
}

/// MultiWaitHolder user data: an opaque value the caller attaches to a holder.
pub(crate) static HOLDER_USER_DATA: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);


/// Message queue operations with a deadline (None = block forever, Some(now) = try once). `front` jams the value at the
/// head instead of the tail. Returns false on timeout; an unknown queue fails at once.
pub(crate) fn mq_send(mq: usize, data: u64, front: bool, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) if q.q.len() < q.cap => {
                if front {
                    q.q.push_front(data)
                } else {
                    q.q.push_back(data)
                }
                drop(g);
                SYNC_CV.notify_all();
                return true;
            }
            Some(_) => {}
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}
/// `peek`: read the head without removing it.
pub(crate) fn mq_receive(out: *mut u64, mq: usize, deadline: Option<std::time::Instant>, peek: bool) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) => {
                let v = if peek { q.q.front().copied() } else { q.q.pop_front() };
                if let Some(v) = v {
                    if !out.is_null() {
                        unsafe { *out = v };
                    }
                    drop(g);
                    SYNC_CV.notify_all();
                    return true;
                }
            }
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}
pub(crate) fn deadline_ns(timeout_ns: i64) -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64))
}

// ── nn::os::ReaderWriterLock (real exclusion) ────────────────────────────────────────────────
pub fn rw_init(addr: usize) {
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .insert(addr, RwSt::default());
}
pub fn rw_acquire_write(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == t {
            r.write_depth += 1;
            return;
        }
        if r.writer == 0 && r.readers == 0 {
            r.writer = t;
            r.write_depth = 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}
pub fn rw_try_acquire_write(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t {
        r.write_depth += 1;
        true
    } else if r.writer == 0 && r.readers == 0 {
        r.writer = t;
        r.write_depth = 1;
        true
    } else {
        false
    }
}
pub fn rw_release_write(addr: usize) {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t && r.write_depth > 0 {
        r.write_depth -= 1;
        if r.write_depth == 0 {
            r.writer = 0;
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}
pub fn rw_acquire_read(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == 0 || r.writer == t {
            r.readers += 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}
pub fn rw_try_acquire_read(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == 0 || r.writer == t {
        r.readers += 1;
        true
    } else {
        false
    }
}
pub fn rw_release_read(addr: usize) {
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.readers > 0 {
        r.readers -= 1;
        if r.readers == 0 {
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}
pub fn rw_write_held_by_me(addr: usize) -> bool {
    let t = me();
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .get(&addr)
        .map(|r| r.writer == t)
        .unwrap_or(false)
}


// ── nn::time ─────────────────────────────────────────────────────────────────────────────────
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
// CalendarTime { i16 year; i8 month, day, hour, minute, second }  (8 bytes)
// CalendarAdditionalInfo { i32 dayOfWeek; i32 yearDay; TimeZone { char name[8]; bool dst; i32 utcOffset } }
pub(crate) unsafe fn fill_calendar(
    tm: &libc::tm,
    cal: *mut u8,
    info: *mut u8,
    name: &[u8],
    dst: bool,
    off: i32,
) {
    if !cal.is_null() {
        *(cal as *mut i16) = (tm.tm_year + 1900) as i16;
        *cal.add(2) = (tm.tm_mon + 1) as u8;
        *cal.add(3) = tm.tm_mday as u8;
        *cal.add(4) = tm.tm_hour as u8;
        *cal.add(5) = tm.tm_min as u8;
        *cal.add(6) = tm.tm_sec as u8;
    }
    if !info.is_null() {
        *(info as *mut i32) = tm.tm_wday;
        *(info.add(4) as *mut i32) = tm.tm_yday;
        std::ptr::write_bytes(info.add(8), 0, 16);
        let n = name.len().min(7);
        std::ptr::copy_nonoverlapping(name.as_ptr(), info.add(8), n);
        *info.add(16) = dst as u8;
        *(info.add(20) as *mut i32) = off;
    }
}

/// Notification messages for the application. After launch the system reports the focus
/// state once (MessageFocusStateChanged = 15; GetCurrentFocusState then says InFocus), as Ryujinx's applet state
/// manager does. Kirby and the Forgotten Land (FocusHandlingMode_SuspendAndNotify) ignored all input without it.
pub(crate) static OE_MESSAGES: Mutex<Option<VecDeque<u32>>> = Mutex::new(None);
// ── nn::pl shared fonts ──────────────────────────────────────────────────────────────────────
// The system's shared fonts (Super Mario Odyssey renders its text with them; TotK ships its own). They are read from
// --shared-fonts (a .app's Resources/fonts), named by type: FontStandard.ttf, FontChineseSimplified.ttf,
// FontExtendedChineseSimplified.ttf, FontChineseTraditional.ttf, FontKorean.ttf, FontNintendoExtended.ttf. A missing
// font falls back to FontStandard.ttf, then to a host font with Latin glyphs.
const SHARED_FONT_FILES: [&str; 6] = [
    "FontStandard.ttf",
    "FontChineseSimplified.ttf",
    "FontExtendedChineseSimplified.ttf",
    "FontChineseTraditional.ttf",
    "FontKorean.ttf",
    "FontNintendoExtended.ttf",
];
const HOST_FONT_CANDIDATES: &[&str] = &[
    // macOS font paths
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Helvetica.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    // iOS font paths
    "/System/Library/Fonts/Core/Arial.ttf",
    "/System/Library/Fonts/CoreAddition/Arial.ttf",
    "/System/Library/Fonts/Core/Helvetica.ttc",
    "/System/Library/Fonts/CoreAddition/Helvetica.ttc",
    "/System/Library/Fonts/Core/HelveticaNeue.ttc",
    "/System/Library/Fonts/CoreAddition/HelveticaNeue.ttc",
    "/System/Library/Fonts/Core/TimesNewRoman.ttf",
    "/System/Library/Fonts/CoreAddition/TimesNewRoman.ttf",
    "/System/Library/Fonts/Core/CourierNew.ttf",
    "/System/Library/Fonts/CoreAddition/CourierNew.ttf",
    "/System/Library/Fonts/Core/PingFang.ttc",
    "/System/Library/Fonts/CoreAddition/PingFang.ttc",
    "/System/Library/Fonts/Core/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/CoreAddition/AppleSDGothicNeo.ttc",
];
pub(crate) static DUMMY_SHARED_FONT: &[u8] = include_bytes!("dummy_font.ttf");
static SHARED_FONTS: [std::sync::OnceLock<Vec<u8>>; 6] = [const { std::sync::OnceLock::new() }; 6];

fn load_shared_font(ty: usize) -> Vec<u8> {
    let dir = crate::settings::get().files.shared_fonts.clone();
    let mut font_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = &dir {
        font_dirs.push(d.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            font_dirs.push(parent.join("fonts"));
            font_dirs.push(parent.join("Resources/fonts"));
            if let Some(grandparent) = parent.parent() {
                font_dirs.push(grandparent.join("Resources/fonts"));
                font_dirs.push(grandparent.join("fonts"));
            }
        }
    }
    if let Some(game_dir) = &crate::settings::get().files.game_dir {
        font_dirs.push(game_dir.join("fonts"));
        font_dirs.push(game_dir.join("RomFS/fonts"));
    }
    if let Some(app_data) = crate::profile::data_dir() {
        if let Some(nxport_dir) = app_data.parent().and_then(|p| p.parent()) {
            font_dirs.push(nxport_dir.join("fonts"));
            font_dirs.push(nxport_dir.join("system_data/0100000000000811"));
        }
    }
    if let Some(sys_font_dir) = crate::horizon::fs::system_data_dir(0x0100000000000811) {
        font_dirs.push(sys_font_dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = std::path::PathBuf::from(home);
        font_dirs.push(p.join("Library/Application Support/nxport/fonts"));
        font_dirs.push(p.join("Documents/fonts"));
        font_dirs.push(p.join("Documents/Game/fonts"));
    }
    font_dirs.push(std::path::PathBuf::from("fonts"));

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for d in &font_dirs {
        paths.push(d.join(SHARED_FONT_FILES[ty]));
        paths.push(d.join(SHARED_FONT_FILES[0]));
    }
    for candidate in HOST_FONT_CANDIDATES {
        paths.push((*candidate).into());
    }
    #[cfg(windows)]
    {
        let windows = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
        for name in ["arial.ttf", "segoeui.ttf", "tahoma.ttf", "verdana.ttf"] {
            paths.push(windows.join("Fonts").join(name));
        }
    }

    for p in &paths {
        if let Ok(bytes) = std::fs::read(p) {
            println!("[+] nn::pl: shared font {} from {} ({} bytes)", ty, p.display(), bytes.len());
            return bytes;
        }
    }
    eprintln!("[!] nn::pl: no shared font {} (--shared-fonts), falling back to embedded dummy font", SHARED_FONT_FILES[ty]);
    DUMMY_SHARED_FONT.to_vec()
}

pub(crate) fn shared_font(ty: i32) -> &'static [u8] {
    let ty = if (0..6).contains(&ty) { ty as usize } else { 0 };
    SHARED_FONTS[ty].get_or_init(|| load_shared_font(ty))
}

/// VR mode as the game set it (nn::pl::BeginVrMode / EndVrMode): the system only records it here; the game draws its own
/// split image.
pub(crate) static VR_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) static FRIENDS_EMPTY: [u8; 0x100] = [0; 0x100];

// ── nn::ldn: local wireless / ad-hoc networking ──────────────────────────────
pub(crate) static LDN_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0); // 0 = None
pub(crate) static LDN_STATE_EVENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);






















// ── nn::account: one local user (the uid TryOpenPreselectedUser / GetUserId report, horizon/hle.rs), no network account ──────
pub(crate) const USER_UID: [u64; 2] = [0x0123456789ABCDEF, 0xFEDCBA9876543210];
pub(crate) unsafe extern "C" fn fs_is_save_data_existing(_uid: *const u8) -> bool {
    true // saves live in the save directory (horizon/fs.rs); EnsureSaveData / MountSaveData create it
}



// ── nn::ngc::ProfanityFilter: every word checks clean (ngc:u is not emulated; the SDK's Initialize opens it) ──────────
pub(crate) unsafe extern "C" fn ngc_ok() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn ngc_check_words(
    _this: *mut u8,
    results: *mut u32,
    _patterns: u32,
    _words: *const usize,
    count: usize,
) -> u32 {
    if !results.is_null() && count < 0x10000 {
        std::ptr::write_bytes(results, 0, count);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn err_show_error(result: u32) {
    eprintln!(
        "[!] nn::err::ShowError(result={:#x}: module {}, description {})",
        result,
        result & 0x1ff,
        result >> 9
    );
}

pub(crate) unsafe extern "C" fn detail_unexpected_default_impl() {
    eprintln!(
        "[!] CRITICAL ABORT: nn::detail::UnexpectedDefaultImpl() (a switch default was reached)"
    );
    std::process::abort();
}

// nfp: no NFC hardware. State 0 = None.
pub(crate) const NFP_RESULT_NOT_AVAILABLE: u32 = 0x1;
pub(crate) unsafe extern "C" fn aoc_list(_out_indices: *mut i32, _offset: i32, _count: i32) -> i32 {
    0
}
/// nn::os::SystemEvent holds its SystemEventType first; the list never changes, so it is never signaled.
pub(crate) unsafe extern "C" fn aoc_list_changed_event(event: *mut u8) {
    crate::horizon::hle::hle_system_event_register_addr(event as usize);
}

/// Which handheld Joy-Cons count as an active controller (both / either / none); the engine's controller is the
/// keyboard / gamepad, so the mode is only kept for the game to read back.
pub(crate) static HANDHELD_ACTIVATION_MODE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub(crate) unsafe extern "C" fn noop_void() {}
pub(crate) unsafe extern "C" fn noop_obj(_this: *mut u8) {}

/// nn::web::ShowOfflineHtmlPage(OfflineHtmlPageReturnValue*, const ShowOfflineHtmlPageArg&): the
/// page "closes at once" as if Back was pressed. OfflineHtmlPageReturnValue: u64 exit reason
/// (OfflineExitReason_BackButtonPressed = 1), char lastUrl[4096], u64 lastUrlSize.
pub(crate) unsafe extern "C" fn web_show_offline_html_page(out: *mut u8, _arg: *const u8) -> u32 {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        println!(
            "[+] HLE: nn::web::ShowOfflineHtmlPage: no web applet, reported as closed with Back"
        )
    });
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 8 + 4096 + 8);
        *(out as *mut u64) = 1;
    }
    RESULT_SUCCESS
}

/// Data-cache maintenance (FlushDataCache etc.): the sdk module's versions execute `dc`/`mrs ctr_el0`
/// sequences that Darwin traps (SIGILL). Apple Silicon memory is coherent for our purposes, so no-op.
pub unsafe extern "C" fn cache_noop(_addr: *const u8, _size: usize) {}


/// nn::settings::detail::MakeLanguageCode(LanguageCode*, Language): the SDK asks the settings service (IPC to "set")
/// for the IETF tag of a Language value (Japanese = 0 ..
/// TraditionalChinese = 16; later versions add BrazilianPortuguese = 17).
/// Interposed onto the SDK's own definition (Kirby and the Forgotten Land calls it through
/// nn::settings::operator==(LanguageCode, Language)).
pub unsafe extern "C" fn settings_make_language_code(out: *mut [u8; 8], language: i32) -> u32 {
    const CODES: [&str; 18] = [
        "ja", "en-US", "fr", "de", "it", "es", "zh-CN", "ko", "nl", "pt", "ru", "zh-TW", "en-GB",
        "fr-CA", "es-419", "zh-Hans", "zh-Hant", "pt-BR",
    ];
    // An invalid value violates the documented precondition: say so and return an empty code.
    let code = usize::try_from(language)
        .ok()
        .and_then(|i| CODES.get(i))
        .copied()
        .unwrap_or_else(|| {
            println!(
                "[!] nn::settings::MakeLanguageCode: invalid language {} (precondition violated)",
                language
            );
            ""
        });
    let mut v = [0u8; 8];
    v[..code.len()].copy_from_slice(code.as_bytes());
    if !out.is_null() {
        *out = v;
    }
    0
}

// nn::bcat (data distribution / delivery cache): a console that never went online. The storage mounts and
// is empty, sync requests fail with no Internet connection, files and directories are not found.
// Result codes use module 122 (result = module | description << 9).
pub(crate) const BCAT_NOT_FOUND: u32 = 122 | (2 << 9);
pub(crate) const BCAT_NOT_OPENED: u32 = 122 | (7 << 9);
pub(crate) const BCAT_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 122 | (8 << 9);
 // DeliveryCacheProgressStatus_Done


// nn::ec (shop access): no network. The library initializes; requests fail
// with ResultInternetRequestNotAccepted (module 164, description 56). Constructors / destructors run
// in the SDK.
pub(crate) const EC_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 164 | (56 << 9);

// nn::account network service account: the user has none linked / no network,
// ResultNetworkServiceAccountUnavailable (module 124, description 200). An AsyncContext the
// failed calls never started reports done with that result; its event is signaled.
pub(crate) const ACCOUNT_NSA_UNAVAILABLE: u32 = 124 | (200 << 9);

/// nn::pdm::QueryApplicationPlayStatistics(out, uid, appIds, count) -> number of entries written: no play records.
pub(crate) unsafe extern "C" fn pdm_query_application_play_statistics(
    _out: *mut u8,
    _uid: *const u8,
    _ids: *const u8,
    _count: i32,
) -> i32 {
    0
}

/// nn::err::ShowError(ErrorCode{category, number}): the error applet; logged.
pub(crate) unsafe extern "C" fn err_show_error_code(code: u64) {
    eprintln!(
        "[!] nn::err::ShowError(error code {:04}-{:04})",
        code as u32,
        (code >> 32) as u32
    );
}

// No network (consistent with nn::socket::Initialize failing and nifm reporting no connection): every socket operation
// fails with Errno::ENetDown (100); closing / finalizing succeed; name lookups fail with EAI_FAIL
// (4). Falling through to the sdk module here would not fail cleanly: its IPC requests are written to the
// thread's Horizon IPC buffer, which is Darwin's TLS on this host (the reply port in slot 2 gets overwritten).
const SOCKET_ENETDOWN: i32 = 100;
thread_local! {
    pub(crate) static SOCKET_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(SOCKET_ENETDOWN) };
}
/// The errno the calling thread's next nn::socket::GetLastErrno returns.
pub(crate) fn set_socket_errno(e: i32) {
    SOCKET_ERRNO.with(|c| c.set(e));
}
/// nn::ssl without a network still works locally on a console: the library initializes, contexts are created and
/// certificates imported; connections fail at the socket level (above). Interposed on the SDK too (nn::ssl::Initialize
/// is called from inside it).
pub unsafe extern "C" fn ssl_ok() -> u32 {
    0
}

