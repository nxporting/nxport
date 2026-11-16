// nn::vi.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2vi10InitializeEv", func!(crate::horizon::vi::initialize())),
    Entry("_ZN2nn2vi11CreateLayerEPPNS0_5LayerEPNS0_7DisplayE", func!(crate::horizon::vi::create_layer(_, _))),
    Entry("_ZN2nn2vi15GetNativeWindowEPPvPNS0_5LayerE", func!(crate::horizon::vi::get_native_window(_, _))),
    Entry("_ZN2nn2vi18OpenDefaultDisplayEPPNS0_7DisplayE", func!(crate::horizon::vi::open_default_display(_))),
    Entry("_ZN2nn2vi19SetLayerScalingModeEPNS0_5LayerENS0_11ScalingModeE", func!(crate::horizon::vi::set_layer_scaling_mode(_, _))),
    Entry("_ZN2nn2vi20GetDisplayVsyncEventEPNS_2os15SystemEventTypeEPNS0_7DisplayE", func!(crate::horizon::vi::get_display_vsync_event(_, _))),
];
