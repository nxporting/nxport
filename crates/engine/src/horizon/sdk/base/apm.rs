// nn::apm.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn3apm18GetPerformanceModeEv", func!(crate::horizon::hle::hle_nn_oe_GetPerformanceMode)),
    Entry("_ZN2nn3apm27GetPerformanceConfigurationENS0_15PerformanceModeE", func!(crate::horizon::hle::hle_nn_oe_GetPerformanceConfiguration)),
];
