// nn::web as the game calls it: the offline web applet, closed with Back.

use crate::horizon::nn::RESULT_SUCCESS;
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
