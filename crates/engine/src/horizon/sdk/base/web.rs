// nn::web.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::web offline pages (Kirby's information page behind A on its first screen): library applets do not exist
    // here, and the SDK's applet code dereferenced null.
    Entry("_ZN2nn3web19ShowOfflineHtmlPageEPNS0_26OfflineHtmlPageReturnValueERKNS0_22ShowOfflineHtmlPageArgE", func!(crate::horizon::nn::web_show_offline_html_page(_, _))),
];
