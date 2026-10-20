// nn::oe.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2oe10InitializeEv", func!(crate::horizon::hle::hle_nn_oe_Initialize)),
    Entry("_ZN2nn2oe15SetCpuBoostModeENS0_12CpuBoostModeE", func!(crate::horizon::hle::hle_nn_oe_SetCpuBoostMode)),
    // oe
    Entry("_ZN2nn2oe16GetOperationModeEv", func!(crate::horizon::hle::hle_nn_oe_GetOperationMode)),
    Entry("_ZN2nn2oe17FinishStartupLogoEv", func!(crate::horizon::nn::oe_finish_startup_logo)),
    Entry("_ZN2nn2oe17GetDisplayVersionEPNS0_14DisplayVersionE", func!(crate::guest::module_loader::oe_get_display_version)),
    Entry("_ZN2nn2oe17GetPseudoDeviceIdEPNS_4util4UuidE", func!(crate::horizon::nn::oe_get_pseudo_device_id)),
    Entry("_ZN2nn2oe18GetDesiredLanguageEv", func!(crate::horizon::hle::hle_nn_oe_GetDesiredLanguage)),
    Entry("_ZN2nn2oe18GetPerformanceModeEv", func!(crate::horizon::hle::hle_nn_oe_GetPerformanceMode)),
    Entry("_ZN2nn2oe20GetCurrentFocusStateEv", func!(crate::horizon::hle::hle_nn_oe_GetCurrentFocusState)),
    Entry("_ZN2nn2oe20SetFocusHandlingModeENS0_17FocusHandlingModeE", func!(crate::horizon::hle::hle_nn_oe_SetFocusHandlingMode)),
    Entry("_ZN2nn2oe22PopNotificationMessageEv", func!(crate::horizon::nn::oe_pop_notification_message)),
    // nn::oe / pl / account / err
    Entry("_ZN2nn2oe24GetExpectedVolumeBalanceEPfS1_", func!(crate::horizon::nn::oe_get_expected_volume_balance)),
    Entry("_ZN2nn2oe24SetExpectedVolumeBalanceEff", func!(crate::horizon::nn::oe_set_expected_volume_balance)),
    Entry("_ZN2nn2oe25GetProgramTotalActiveTimeEv", func!(crate::horizon::nn::oe_get_program_total_active_time)),
    Entry("_ZN2nn2oe25TryPopNotificationMessageEPj", func!(crate::horizon::nn::oe_try_pop_notification_message)),
    Entry("_ZN2nn2oe27GetPerformanceConfigurationENS0_15PerformanceModeE", func!(crate::horizon::hle::hle_nn_oe_GetPerformanceConfiguration)),
    Entry("_ZN2nn2oe27SetPerformanceConfigurationENS0_15PerformanceModeEi", func!(crate::horizon::hle::hle_nn_oe_SetPerformanceConfiguration)),
    Entry("_ZN2nn2oe28SetResumeNotificationEnabledEb", func!(crate::horizon::hle::hle_nn_oe_SetResumeNotificationEnabled)),
    // Tells the system how to rotate captured screenshots (IPC to the applet manager); there is none to tell.
    Entry("_ZN2nn2oe29SetScreenShotImageOrientationENS_5album16ImageOrientationE", func!(crate::horizon::nn::noop_void)),
    Entry("_ZN2nn2oe31EnterExitRequestHandlingSectionEv", func!(crate::horizon::nn::oe_enter_exit_request_handling_section)),
    Entry("_ZN2nn2oe31LeaveExitRequestHandlingSectionEv", func!(crate::horizon::nn::oe_leave_exit_request_handling_section)),
    Entry("_ZN2nn2oe38SetUserInactivityDetectionTimeExtendedEb", func!(crate::horizon::hle::hle_nn_oe_SetUserInactivityDetectionTimeExtended)),
    Entry("_ZN2nn2oe42SetOperationModeChangedNotificationEnabledEb", func!(crate::horizon::hle::hle_nn_oe_SetOperationModeChangedNotificationEnabled)),
    Entry("_ZN2nn2oe44SetPerformanceModeChangedNotificationEnabledEb", func!(crate::horizon::hle::hle_nn_oe_SetPerformanceModeChangedNotificationEnabled)),
];
