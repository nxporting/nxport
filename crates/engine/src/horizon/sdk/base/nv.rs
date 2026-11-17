// nv (graphics memory).

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nv18InitializeGraphicsEPvm", func!(crate::horizon::nv::initialize_graphics(_, _))),
    Entry("_ZN2nv20SetGraphicsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_", func!(crate::horizon::nv::set_graphics_allocator(_, _, _, _))),
    Entry("_ZN2nv28SetGraphicsDevtoolsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_", func!(crate::horizon::nv::set_graphics_devtools_allocator(_, _, _, _))),
];
