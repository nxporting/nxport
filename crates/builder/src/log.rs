// Everything this process and its child processes print, for the log view in the builder UI.

use std::sync::Mutex;

/// Everything this process prints (the steps' progress), for the log view.
pub static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(not(windows))]
pub fn capture_output() {
    use std::io::{BufRead, BufReader, Write};

    unsafe {
        let mut fds = [0; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        let console = libc::dup(1);
        libc::dup2(fds[1], 1);
        libc::dup2(fds[1], 2);
        libc::close(fds[1]);
        use std::os::fd::FromRawFd;
        let reader = std::fs::File::from_raw_fd(fds[0]);
        let mut console = std::fs::File::from_raw_fd(console);
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let _ = writeln!(console, "{line}");
                let mut log = LOG.lock().unwrap();
                log.push(line);
                let n = log.len();
                if n > 5000 {
                    log.drain(..n - 5000);
                }
            }
        });
    }
}

#[cfg(windows)]
pub fn capture_output() {
    use std::ffi::c_void;
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::io::FromRawHandle;

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    extern "system" {
        fn CreatePipe(
            read_pipe: *mut *mut c_void,
            write_pipe: *mut *mut c_void,
            pipe_attributes: *const c_void,
            size: u32,
        ) -> i32;
        fn SetStdHandle(std_handle: u32, handle: *mut c_void) -> i32;
        fn GetStdHandle(std_handle: u32) -> *mut c_void;
        fn DuplicateHandle(
            src_proc: *mut c_void,
            src_handle: *mut c_void,
            target_proc: *mut c_void,
            target_handle: *mut *mut c_void,
            desired_access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }

    extern "C" {
        fn _open_osfhandle(osfhandle: isize, flags: i32) -> i32;
        fn _dup2(src: i32, dst: i32) -> i32;
    }

    unsafe {
        let mut read_pipe = std::ptr::null_mut();
        let mut write_pipe = std::ptr::null_mut();
        if CreatePipe(&mut read_pipe, &mut write_pipe, std::ptr::null(), 0) == 0 {
            return;
        }

        let cur_proc = GetCurrentProcess();
        let mut console_handle = std::ptr::null_mut();
        let old_out = GetStdHandle(STD_OUTPUT_HANDLE);
        if !old_out.is_null() {
            DuplicateHandle(
                cur_proc,
                old_out,
                cur_proc,
                &mut console_handle,
                0,
                0,
                2, // DUPLICATE_SAME_ACCESS
            );
        }

        SetStdHandle(STD_OUTPUT_HANDLE, write_pipe);
        SetStdHandle(STD_ERROR_HANDLE, write_pipe);

        let write_fd = _open_osfhandle(write_pipe as isize, 0);
        if write_fd >= 0 {
            _dup2(write_fd, 1);
            _dup2(write_fd, 2);
        }

        let reader = std::fs::File::from_raw_handle(read_pipe);
        let mut console = if !console_handle.is_null() {
            Some(std::fs::File::from_raw_handle(console_handle))
        } else {
            None
        };

        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if let Some(c) = console.as_mut() {
                    let _ = writeln!(c, "{line}");
                }
                let mut log = LOG.lock().unwrap();
                log.push(line);
                let n = log.len();
                if n > 5000 {
                    log.drain(..n - 5000);
                }
            }
        });
    }
}
