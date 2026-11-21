// nn::os fibers where guest code runs lifted (Windows). The SDK's own SwitchToFiber saves one register context and
// resumes another inside its code, which lifted code cannot do: each guest function is a host function on the host
// stack. Here every guest fiber runs on a host fiber of its own (its own host stack, its own guest stack), and a switch
// saves the thread's lifted CPU state for the fiber left and restores the one entered. Natively (Apple) the SDK's code
// runs as it is.
//
// SDK: InitializeFiber(FiberType*, FiberType* (*)(void*), void* argument, void* stack, size_t stack_size, int flags);
// SwitchToFiber(FiberType*); FinalizeFiber(FiberType*); GetCurrentFiber() reads ThreadType::_currentFiber, kept here.
// A fiber's function returns the fiber to switch to next.

use nxport_lifterlib::abi::Cpu;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

#[link(name = "kernel32")]
extern "system" {
    fn ConvertThreadToFiber(param: *mut c_void) -> *mut c_void;
    fn CreateFiberEx(commit: usize, reserve: usize, flags: u32, start: unsafe extern "system" fn(*mut c_void), param: *mut c_void) -> *mut c_void;
    fn SwitchToFiber(fiber: *mut c_void);
    fn DeleteFiber(fiber: *mut c_void);
}

/// Host stack reserved for each fiber: lifted code nests as deeply as the guest's (a guest thread gets 256 MiB).
const HOST_STACK: usize = 64 << 20;

struct Fiber {
    function: u64,
    argument: u64,
    stack_top: u64,
    /// Its host fiber, once first entered.
    host: usize,
    /// Its guest registers while another fiber runs.
    saved: Option<Box<Cpu>>,
}

// A fiber runs on one thread at a time; its saved registers are only data (Cpu holds a thread pointer).
unsafe impl Send for Fiber {}

static FIBERS: Mutex<Option<HashMap<usize, Fiber>>> = Mutex::new(None);

thread_local! {
    /// The thread's own host fiber (ConvertThreadToFiber), and its guest registers while one of its fibers runs.
    static THREAD_HOST: Cell<usize> = const { Cell::new(0) };
    static THREAD_SAVED: Cell<*mut Cpu> = const { Cell::new(std::ptr::null_mut()) };
}

fn with_fibers<R>(f: impl FnOnce(&mut HashMap<usize, Fiber>) -> R) -> R {
    f(FIBERS.lock().unwrap().get_or_insert_with(HashMap::new))
}

unsafe fn current_thread_fiber() -> usize {
    let t = crate::horizon::os::get_current_thread();
    if t.is_null() {
        0
    } else {
        (*t).current_fiber as usize
    }
}

unsafe fn set_current_fiber(f: usize) {
    let t = crate::horizon::os::get_current_thread();
    if !t.is_null() {
        (*t).current_fiber = f as *mut ();
    }
}

/// The guest registers now, as a fiber leaving them keeps them.
unsafe fn save_cpu() -> Box<Cpu> {
    Box::new(std::ptr::read(crate::guest::exec::lifted::thread_cpu() as *const Cpu))
}

unsafe fn restore_cpu(c: Box<Cpu>) {
    std::ptr::write(crate::guest::exec::lifted::thread_cpu() as *mut Cpu, *c);
}

pub(crate) unsafe extern "C" fn initialize_fiber(fiber: *mut u8, function: u64, argument: u64, stack: u64, stack_size: u64, _flags: i32) {
    if fiber.is_null() {
        return;
    }
    let stack_top = (stack + stack_size) & !15;
    let old = with_fibers(|m| m.insert(fiber as usize, Fiber { function, argument, stack_top, host: 0, saved: None }));
    // Made again after it finished: its host fiber is parked where it ended (fiber_main), not running.
    if let Some(f) = old.filter(|f| f.host != 0 && current_thread_fiber() != fiber as usize) {
        DeleteFiber(f.host as *mut c_void);
    }
}

pub(crate) unsafe extern "C" fn finalize_fiber(fiber: *mut u8) {
    let gone = with_fibers(|m| m.remove(&(fiber as usize)));
    if let Some(f) = gone.filter(|f| f.host != 0) {
        DeleteFiber(f.host as *mut c_void);
    }
}

pub(crate) unsafe extern "C" fn switch_to_fiber(to: *mut u8) {
    let from = current_thread_fiber();
    let to = to as usize;
    if to == from {
        return;
    }
    if THREAD_HOST.with(Cell::get) == 0 {
        THREAD_HOST.with(|h| h.set(ConvertThreadToFiber(std::ptr::null_mut()) as usize));
    }
    // Keep this context's registers: the thread's own (fiber 0) or the fiber's.
    let saved = save_cpu();
    if from == 0 {
        THREAD_SAVED.with(|s| s.set(Box::into_raw(saved)));
    } else {
        with_fibers(|m| m.get_mut(&from).map(|f| f.saved = Some(saved)));
    }
    set_current_fiber(to);
    let host = if to == 0 {
        THREAD_HOST.with(Cell::get)
    } else {
        with_fibers(|m| {
            let f = m.get_mut(&to).expect("SwitchToFiber: a fiber InitializeFiber did not make");
            if f.host == 0 {
                f.host = CreateFiberEx(0x1_0000, HOST_STACK, 0, fiber_main, to as *mut c_void) as usize;
                assert!(f.host != 0, "SwitchToFiber: CreateFiberEx failed: {}", std::io::Error::last_os_error());
            }
            f.host
        })
    };
    SwitchToFiber(host as *mut c_void);
    // Back here: another context switched to this one and set the thread's current fiber; its registers are ours again.
    resume(from);
}

/// Restores the registers context `me` kept when it last left.
unsafe fn resume(me: usize) {
    let saved = if me == 0 {
        let p = THREAD_SAVED.with(|s| s.replace(std::ptr::null_mut()));
        (!p.is_null()).then(|| Box::from_raw(p))
    } else {
        with_fibers(|m| m.get_mut(&me).and_then(|f| f.saved.take()))
    };
    if let Some(c) = saved {
        restore_cpu(c);
    }
}

/// A fiber's host entry: its function on its guest stack, then the fiber that returns (a fiber never ends).
unsafe extern "system" fn fiber_main(param: *mut c_void) {
    let me = param as usize;
    // A fiber whose function returned runs it again from the start when next switched to: Jamboree's job fibers return
    // to the thread after each job and are switched to again without InitializeFiber.
    loop {
        let (function, argument, stack_top) = with_fibers(|m| m.get(&me).map(|f| (f.function, f.argument, f.stack_top))).expect("fiber");
        let cpu = crate::guest::exec::lifted::thread_cpu();
        // The thread's registers (TLS pointers, FP control) with a fresh stack and no frame.
        cpu.sp = stack_top;
        cpu.x[29] = 0;
        // The fiber to go on with; none: the thread's own context (where GetCurrentFiber is null).
        let next = crate::guest::exec::lifted::library().call(cpu, function, &[argument]);
        switch_to_fiber(next as *mut u8);
    }
}
