// horizon::movie::MediaData.

use super::*;

// ── horizon::movie::MediaData ({ void* m_MediaDataImpl }) ───────────────────────────────────────────────────────
#[derive(Clone)]
pub(crate) enum Val {
    I32(i32),
    I64(i64),
    Str(CString),
    Rect([i32; 4]),
}
pub(crate) type Store = HashMap<String, Val>;

pub(crate) unsafe fn store<'a>(md: *mut usize) -> &'a mut Store {
    if *md == 0 {
        *md = Box::into_raw(Box::new(Store::new())) as usize;
    }
    &mut *(*md as *mut Store)
}
pub(crate) unsafe fn key(name: *const c_char) -> String {
    if name.is_null() {
        String::new()
    } else {
        CStr::from_ptr(name).to_string_lossy().into_owned()
    }
}
pub(crate) unsafe extern "C" fn md_ctor(md: *mut usize) {
    *md = 0
}
pub(crate) unsafe extern "C" fn md_dtor(md: *mut usize) {
    if *md != 0 {
        drop(Box::from_raw(*md as *mut Store));
        *md = 0;
    }
}
pub(crate) unsafe extern "C" fn md_copy_ctor(md: *mut usize, o: *const usize) {
    *md = 0;
    md_assign(md, o);
}
pub(crate) unsafe extern "C" fn md_assign(md: *mut usize, o: *const usize) -> *mut usize {
    if md as *const usize != o {
        let copy = if *o == 0 {
            Store::new()
        } else {
            (*(*o as *const Store)).clone()
        };
        *store(md) = copy;
    }
    md
}
pub(crate) unsafe extern "C" fn md_clear(md: *mut usize) {
    store(md).clear()
}
pub(crate) fn md_set(md: *mut usize, k: &str, v: Val) {
    unsafe { store(md).insert(k.to_string(), v) };
}
pub(crate) unsafe extern "C" fn md_set_i32(md: *mut usize, name: *const c_char, v: i32) {
    md_set(md, &key(name), Val::I32(v))
}
pub(crate) unsafe extern "C" fn md_set_i64(md: *mut usize, name: *const c_char, v: i64) {
    md_set(md, &key(name), Val::I64(v))
}
pub(crate) unsafe extern "C" fn md_set_string(md: *mut usize, name: *const c_char, v: *const c_char) {
    md_set(md, &key(name), Val::Str(CStr::from_ptr(v).to_owned()))
}
pub(crate) unsafe extern "C" fn md_set_rect(
    md: *mut usize,
    name: *const c_char,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) {
    md_set(md, &key(name), Val::Rect([l, t, r, b]))
}
pub(crate) unsafe extern "C" fn md_find_rect(
    md: *mut usize,
    name: *const c_char,
    l: *mut i32,
    t: *mut i32,
    r: *mut i32,
    b: *mut i32,
) -> bool {
    if *md == 0 {
        return false;
    }
    match store(md).get(&key(name)) {
        Some(Val::Rect(v)) => {
            for (p, x) in [l, t, r, b].into_iter().zip(v) {
                if !p.is_null() {
                    *p = *x;
                }
            }
            true
        }
        _ => false,
    }
}
pub(crate) unsafe extern "C" fn md_contains(md: *mut usize, name: *const c_char) -> bool {
    *md != 0 && store(md).contains_key(&key(name))
}
pub(crate) unsafe extern "C" fn md_find_i32(md: *mut usize, name: *const c_char, out: *mut i32) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_i32('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    match (*md != 0)
        .then(|| store(md).get(&key(name)).cloned())
        .flatten()
    {
        Some(Val::I32(v)) => {
            *out = v;
            true
        }
        Some(Val::I64(v)) => {
            *out = v as i32;
            true
        }
        _ => false,
    }
}
pub(crate) unsafe extern "C" fn md_find_i64(md: *mut usize, name: *const c_char, out: *mut i64) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_i64('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    match (*md != 0)
        .then(|| store(md).get(&key(name)).cloned())
        .flatten()
    {
        Some(Val::I64(v)) => {
            *out = v;
            true
        }
        Some(Val::I32(v)) => {
            *out = v as i64;
            true
        }
        _ => false,
    }
}
pub(crate) unsafe extern "C" fn md_find_string(
    md: *mut usize,
    name: *const c_char,
    out: *mut *const c_char,
) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_string('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    if *md == 0 {
        return false;
    }
    match store(md).get(&key(name)) {
        Some(Val::Str(s)) => {
            *out = s.as_ptr();
            true
        }
        _ => false,
    }
}
pub(crate) unsafe extern "C" fn md_count(md: *mut usize) -> usize {
    if *md == 0 {
        0
    } else {
        store(md).len()
    }
}
