// nn::err as the game calls it: error dialogs, logged instead of shown.

pub(crate) unsafe extern "C" fn err_show_error(result: u32) {
    eprintln!(
        "[!] nn::err::ShowError(result={:#x}: module {}, description {})",
        result,
        result & 0x1ff,
        result >> 9
    );
}

/// nn::err::ShowError(ErrorCode{category, number}): the error applet; logged.
pub(crate) unsafe extern "C" fn err_show_error_code(code: u64) {
    eprintln!(
        "[!] nn::err::ShowError(error code {:04}-{:04})",
        code as u32,
        (code >> 32) as u32
    );
}
