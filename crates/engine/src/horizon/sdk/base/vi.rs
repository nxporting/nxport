// nn::vi.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2vi10InitializeEv", func!(crate::horizon::hle::hle_nn_vi_Initialize())),
    Entry("_ZN2nn2vi11CreateLayerEPPNS0_5LayerEPNS0_7DisplayE", func!(crate::horizon::hle::hle_nn_vi_CreateLayer(_, _))),
    Entry("_ZN2nn2vi15GetNativeWindowEPPvPNS0_5LayerE", func!(crate::horizon::hle::hle_nn_vi_GetNativeWindow(_, _))),
    Entry("_ZN2nn2vi18OpenDefaultDisplayEPPNS0_7DisplayE", func!(crate::horizon::hle::hle_nn_vi_OpenDefaultDisplay(_))),
    Entry("_ZN2nn2vi19SetLayerScalingModeEPNS0_5LayerENS0_11ScalingModeE", func!(crate::horizon::hle::hle_nn_vi_SetLayerScalingMode(_, _))),
    Entry("_ZN2nn2vi20GetDisplayVsyncEventEPNS_2os15SystemEventTypeEPNS0_7DisplayE", func!(crate::horizon::hle::hle_nn_vi_GetDisplayVsyncEvent(_, _))),
];
