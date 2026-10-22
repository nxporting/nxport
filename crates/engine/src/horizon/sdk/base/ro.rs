// nn::ro.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2ro12LookupSymbolEPmPKc", func!(crate::guest::module_loader::ro_lookup_symbol(_, _))),
];
