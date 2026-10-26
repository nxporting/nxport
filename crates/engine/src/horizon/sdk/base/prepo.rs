// nn::prepo.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::prepo::Initialize (the rest of prepo is below)
    Entry("_ZN2nn5prepo10InitializeEv", func!(crate::horizon::nn::prepo_initialize())),
    Entry("_ZN2nn5prepo10PlayReport10SetEventId*", func!(crate::horizon::nn::prepo_save_result(_))),
    // PlayReport::Add* / SetBuffer / SetEventId / Save(Uid) / AddInternetConnectionStatus: every overload discards its
    // arguments.
    Entry("_ZN2nn5prepo10PlayReport3Add*", func!(crate::horizon::nn::prepo_save_result(_))),
    Entry("_ZN2nn5prepo10PlayReport4SaveERK*", func!(crate::horizon::nn::prepo_save_result(_))),
    Entry("_ZN2nn5prepo10PlayReport4SaveEv", func!(crate::horizon::nn::prepo_save_result(_))),
    Entry("_ZN2nn5prepo10PlayReport5ClearEv", func!(crate::horizon::nn::noop_obj(_))),
    Entry("_ZN2nn5prepo10PlayReport7GetSizeEv", func!(crate::horizon::nn::prepo_get_size(_))),
    Entry("_ZN2nn5prepo10PlayReport9SetBuffer*", func!(crate::horizon::nn::prepo_save_result(_))),
    // nn::prepo (play reports go nowhere)
    Entry("_ZN2nn5prepo10PlayReportC1Ev", func!(crate::horizon::nn::noop_obj(_))),
    Entry("_ZN2nn5prepo27AddInternetConnectionStatus*", func!(crate::horizon::nn::prepo_save_result(_))),
    Entry("_ZN2nn5prepo28RequestImmediateTransmissionEv", func!(crate::horizon::nn::prepo_zero_u32())),
    // Reports are discarded (SetBuffer / Add* below never store), so they hold no key-value pairs.
    Entry("_ZNK2nn5prepo10PlayReport8GetCountEv", func!(crate::horizon::nn::prepo_get_size(_))),
];
