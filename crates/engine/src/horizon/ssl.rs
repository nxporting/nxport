// nn::ssl as the game calls it: accepted, never used for a connection.

/// nn::ssl without a network still works locally on a console: the library initializes, contexts are created and
/// certificates imported; connections fail at the socket level (above). Interposed on the SDK too (nn::ssl::Initialize
/// is called from inside it).
pub(crate) unsafe extern "C" fn ssl_ok() -> u32 {
    0
}
