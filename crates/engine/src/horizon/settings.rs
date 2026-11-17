// nn::settings as the game calls it: language codes the settings service would answer.

/// nn::settings::detail::MakeLanguageCode(LanguageCode*, Language): the SDK asks the settings service (IPC to "set")
/// for the IETF tag of a Language value (Japanese = 0 ..
/// TraditionalChinese = 16; later versions add BrazilianPortuguese = 17).
/// Interposed onto the SDK's own definition (Kirby and the Forgotten Land calls it through
/// nn::settings::operator==(LanguageCode, Language)).
pub(crate) unsafe extern "C" fn settings_make_language_code(out: *mut [u8; 8], language: i32) -> u32 {
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
