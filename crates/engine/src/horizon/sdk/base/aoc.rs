// nn::aoc.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // Add-on content: none is installed.
    Entry("_ZN2nn3aoc16ListAddOnContentEPiii", func!(crate::horizon::nn::aoc_list(_, _, _))),
    Entry("_ZN2nn3aoc31GetAddOnContentListChangedEventEPNS_2os11SystemEventE", func!(crate::horizon::nn::aoc_list_changed_event(_))),
];
