// nn::os locks: mutexes, condition variables, reader / writer locks and semaphores, as the game calls them (their
// state lives in objects.rs).

use super::*;

pub(crate) unsafe extern "C" fn initialize_mutex(
    mutex: *mut NnMutex,
    recursive: bool,
    level: i32,
) {
    if (mutex as usize) >= 0x10000 {
        (*mutex).state = 1;
        (*mutex).is_recursive = recursive;
        (*mutex)._pad1 = [0; 2];
        (*mutex).lock_level = level;
        (*mutex).nest_count = 0;
        (*mutex)._pad2 = [0; 4];
        (*mutex).owner_thread = std::ptr::null_mut();
        (*mutex).lock = 0;
        (*mutex)._pad3 = 0;
    }
}

pub(crate) unsafe extern "C" fn lock_mutex(mutex: *mut NnMutex) {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).owner_thread == cur && (*mutex).nest_count > 0 {
        if !(*mutex).is_recursive {
            // Real hardware asserts here. Treat as recursive so we can see who does it.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                let lr = crate::guest_reg!(x30);
                eprintln!("[!] nn::os::LockMutex: non-recursive mutex {:p} relocked by its owner (guest_lr={:#x}); treating as recursive", mutex, lr);
            }
        }
        (*mutex).nest_count += 1;
        return;
    }
    let word = mutex_lock_word(mutex);
    let tok = mutex_token(cur);
    if word.compare_exchange(0, tok, Ordering::Acquire, Ordering::Relaxed).is_err() {
        mutex_lock_contended(word, tok);
    }
    (*mutex).owner_thread = cur;
    (*mutex).nest_count = 1;
}

pub(crate) unsafe extern "C" fn unlock_mutex(mutex: *mut NnMutex) {
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).owner_thread != cur {
        return;
    }
    (*mutex).nest_count -= 1;
    if (*mutex).nest_count == 0 {
        (*mutex).owner_thread = std::ptr::null_mut();
        let word = mutex_lock_word(mutex);
        if word.swap(0, std::sync::atomic::Ordering::Release) & MUTEX_WAITERS != 0 {
            mutex_unpark(word);
        }
    }
}

pub(crate) unsafe extern "C" fn try_lock_mutex(mutex: *mut NnMutex) -> bool {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return false;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).is_recursive && (*mutex).owner_thread == cur && (*mutex).nest_count > 0 {
        (*mutex).nest_count += 1;
        return true;
    }
    if mutex_lock_word(mutex)
        .compare_exchange(0, mutex_token(cur), Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        (*mutex).owner_thread = cur;
        (*mutex).nest_count = 1;
        true
    } else {
        false
    }
}

pub(crate) unsafe extern "C" fn finalize_mutex(mutex: *mut NnMutex) {
    if (mutex as usize) >= 0x10000 {
        (*mutex).state = 0;
        (*mutex).owner_thread = std::ptr::null_mut();
        (*mutex).nest_count = 0;
        (*mutex).lock = 0;
    }
}

pub(crate) unsafe extern "C" fn initialize_reader_writer_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_init(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn finalize_reader_writer_lock(_rwlock: *mut NnReaderWriterLock) {}

pub(crate) unsafe extern "C" fn acquire_write_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_acquire_write(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn release_write_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_release_write(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn acquire_read_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_acquire_read(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn release_read_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_release_read(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn initialize_condition_variable(cv: *mut ()) {
    condvar_table()
        .lock()
        .unwrap()
        .insert(cv as usize, condvar_new());
}

pub(crate) unsafe extern "C" fn finalize_condition_variable(cv: *mut ()) {
    condvar_table().lock().unwrap().remove(&(cv as usize));
}

pub(crate) unsafe extern "C" fn signal_condition_variable(cv: *mut ()) {
    let state = condvar_get(cv);
    {
        let mut gen = state.generation.lock().unwrap();
        *gen = gen.wrapping_add(1);
    }
    state.cv.notify_all();
}

/// Signal already wakes every waiter (generation counter), so Broadcast is the same operation.
pub(crate) unsafe extern "C" fn broadcast_condition_variable(cv: *mut ()) {
    crate::horizon::os::signal_condition_variable(cv);
}

/// nn::os::TimedWaitConditionVariable: returns ConditionVariableStatus (Timeout = 0, NoTimeout = 1).
pub(crate) unsafe extern "C" fn timed_wait_condition_variable(
    cv: *mut (),
    mutex: *mut NnMutex,
    timeout_ns: i64,
) -> i32 {
    crate::profile::main_thread_tick("timed_wait_condition_variable");
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();
    crate::horizon::os::unlock_mutex(mutex);
    let timed_out = {
        let guard = state.generation.lock().unwrap();
        let timeout = std::time::Duration::from_nanos(timeout_ns.max(0) as u64);
        state
            .cv
            .wait_timeout_while(guard, timeout, |gen| *gen == seen_gen)
            .unwrap()
            .1
            .timed_out()
    };
    crate::horizon::os::lock_mutex(mutex);
    if timed_out {
        0
    } else {
        1
    }
}

pub(crate) unsafe extern "C" fn wait_condition_variable(cv: *mut (), mutex: *mut NnMutex) {
    crate::profile::main_thread_tick("wait_condition_variable");
    let fp = crate::guest_reg!(x29);
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();

    // Real nn::os semantics: atomically release the (already-held) guest
    // mutex and block until signaled, then re-acquire it before returning.
    // The guest mutex isn't the same lock as our internal Condvar's, so we
    // can't do this in one atomic step the way Rust's Condvar::wait(guard)
    // does with its own Mutex -- release, wait on our own generation-counted
    // condvar, then re-lock, matching the same net effect.
    crate::horizon::os::unlock_mutex(mutex);
    {
        let _blocked = BlockGuard::new("CondVar", cv as usize, fp);
        let guard = state.generation.lock().unwrap();
        let _final_guard = state.cv.wait_while(guard, |gen| *gen == seen_gen).unwrap();
    }
    crate::horizon::os::lock_mutex(mutex);
}

pub(crate) unsafe extern "C" fn try_acquire_semaphore(sem: *mut ()) -> bool {
    semaphore_try_acquire(sem, 0)
}

pub(crate) unsafe extern "C" fn timed_acquire_semaphore(sem: *mut (), timeout_ns: i64) -> bool {
    crate::profile::main_thread_tick("timed_acquire_semaphore");
    semaphore_try_acquire(sem, timeout_ns)
}

pub(crate) unsafe extern "C" fn get_current_semaphore_count(sem: *const ()) -> i32 {
    semaphore_count(sem)
}

pub(crate) unsafe extern "C" fn initialize_semaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

pub(crate) unsafe extern "C" fn finalize_semaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

pub(crate) unsafe extern "C" fn acquire_semaphore(sem: *mut ()) {
    crate::profile::main_thread_tick("acquire_semaphore");
    let fp = crate::guest_reg!(x29);
    semaphore_acquire(sem, fp);
}

pub(crate) unsafe extern "C" fn release_semaphore(sem: *mut (), count: i32) {
    semaphore_release(sem, count);
}

pub(crate) unsafe extern "C" fn initialize_light_semaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

pub(crate) unsafe extern "C" fn finalize_light_semaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

pub(crate) unsafe extern "C" fn acquire_light_semaphore(sem: *mut ()) {
    crate::profile::main_thread_tick("acquire_light_semaphore");
    let fp = crate::guest_reg!(x29);
    semaphore_acquire(sem, fp);
}

pub(crate) unsafe extern "C" fn release_light_semaphore(sem: *mut ()) {
    semaphore_release(sem, 1);
}

pub(crate) unsafe extern "C" fn try_acquire_write_lock(rw: *mut u8) -> bool {
    rw_try_acquire_write(rw as usize)
}

pub(crate) unsafe extern "C" fn try_acquire_read_lock(rw: *mut u8) -> bool {
    rw_try_acquire_read(rw as usize)
}
