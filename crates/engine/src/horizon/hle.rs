// HLE glue the services share: the stub every unbound import lands on, sleeping on a guest thread, the timestamp
// log switch, and constants several services use.

#[no_mangle]
pub unsafe extern "C" fn hle_default_stub() -> i64 {
    let got_slot = crate::guest_reg!(x16);
    let caller_lr = crate::guest_reg!(x17);
    let guest_base = crate::guest::loader::main_base();
    let got_virtual = if got_slot >= guest_base {
        got_slot - guest_base
    } else {
        got_slot
    };
    let lr_virtual = if caller_lr >= guest_base {
        caller_lr - guest_base
    } else {
        caller_lr
    };
    println!(
        "[!] HLE UNIMPLEMENTED GOT STUB: GOT slot 0x{:08X}, caller LR=0x{:08X}",
        got_virtual, lr_virtual
    );
    0
}

/// --log-timestamps (with metal/mod.rs): guest-main's GPU syncs and sleeps next to the TIMESTAMP reports, in ms of the
/// same clock, to reconstruct a frame's timeline.
pub(crate) fn log_timestamps() -> bool {
    crate::settings::get().log.log_timestamps
}

/// Sleep on a guest thread without nanosleep(): libsystem's nanosleep makes a MIG call that
/// needs this thread's mach reply-port TSD slot, which the guest's use of tpidrro_el0 as
/// its Horizon TLS can clobber ("clock_get_time() failed: (ipc/send) invalid reply port").
/// mach_wait_until is a bare kernel trap with no reply port (crate::platform::clock::sleep_ns).
pub fn hle_sleep_ns(ns: u64) {
    crate::platform::clock::sleep_ns(ns);
}

pub static mut NVN_PRIVATE0: [u64; 64] = [0; 64];

pub(crate) const NN_RESULT_SUCCESS: u32 = 0;
