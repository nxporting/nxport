// nn::nfp.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn3nfp10GetTagInfoEPNS0_7TagInfoERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_fail2(_, _))),
    // nn::nfp (no NFC hardware)
    Entry("_ZN2nn3nfp10InitializeEv", func!(crate::horizon::nn::nfp_initialize())),
    Entry("_ZN2nn3nfp11ListDevicesEPNS0_12DeviceHandleEPii", func!(crate::horizon::nn::nfp_list_devices(_, _, _))),
    Entry("_ZN2nn3nfp12GetModelInfoEPNS0_9ModelInfoERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_fail2(_, _))),
    Entry("_ZN2nn3nfp13StopDetectionERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_fail1(_))),
    Entry("_ZN2nn3nfp14GetDeviceStateERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_get_device_state(_))),
    Entry("_ZN2nn3nfp14StartDetectionERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_fail1(_))),
    Entry("_ZN2nn3nfp19AttachActivateEventEPNS_2os15SystemEventTypeERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_attach_event_with_handle(_, _))),
    Entry("_ZN2nn3nfp21AttachDeactivateEventEPNS_2os15SystemEventTypeERKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_attach_event_with_handle(_, _))),
    Entry("_ZN2nn3nfp29AttachAvailabilityChangeEventEPNS_2os15SystemEventTypeE", func!(crate::horizon::nn::nfp_attach_event(_))),
    Entry("_ZN2nn3nfp5MountERKNS0_12DeviceHandleENS0_9ModelTypeENS0_11MountTargetE", func!(crate::horizon::nn::nfp_mount(_, _, _))),
    Entry("_ZN2nn3nfp8FinalizeEv", func!(crate::horizon::nn::nfp_finalize())),
    Entry("_ZN2nn3nfp8GetStateEv", func!(crate::horizon::nn::nfp_get_state())),
    Entry("_ZN2nn3nfp9GetNpadIdEPjRKNS0_12DeviceHandleE", func!(crate::horizon::nn::nfp_fail2(_, _))),
];
