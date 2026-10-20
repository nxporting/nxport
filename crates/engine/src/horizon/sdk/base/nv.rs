// nv (graphics memory).

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nv18InitializeGraphicsEPvm", func!(crate::horizon::hle::hle_nv_InitializeGraphics)),
    Entry("_ZN2nv20SetGraphicsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_", func!(crate::horizon::hle::hle_nv_SetGraphicsAllocator)),
    Entry("_ZN2nv28SetGraphicsDevtoolsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_", func!(crate::horizon::hle::hle_nv_SetGraphicsDevtoolsAllocator)),
];
