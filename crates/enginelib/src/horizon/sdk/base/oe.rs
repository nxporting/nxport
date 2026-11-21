// nn::oe.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2oe10InitializeEv", func!(crate::horizon::oe::initialize())),
    Entry("_ZN2nn2oe15SetCpuBoostModeENS0_12CpuBoostModeE", func!(crate::horizon::oe::set_cpu_boost_mode(_))),
    // oe
    Entry("_ZN2nn2oe16GetOperationModeEv", func!(crate::horizon::oe::get_operation_mode())),
    Entry("_ZN2nn2oe17FinishStartupLogoEv", func!(crate::horizon::oe::finish_startup_logo())),
    Entry("_ZN2nn2oe17GetDisplayVersionEPNS0_14DisplayVersionE", func!(crate::guest::module_loader::oe_get_display_version(_))),
    Entry("_ZN2nn2oe23EnableGamePlayRecordingEPvm", func!(crate::guest::module_loader::oe_enable_game_play_recording(_, _))),
    Entry("_ZN2nn2oe17GetPseudoDeviceIdEPNS_4util4UuidE", func!(crate::horizon::oe::get_pseudo_device_id(_))),
    Entry("_ZN2nn2oe18GetDesiredLanguageEv", func!(crate::horizon::oe::get_desired_language())),
    Entry("_ZN2nn2oe18GetPerformanceModeEv", func!(crate::horizon::oe::get_performance_mode())),
    Entry("_ZN2nn2oe20GetCurrentFocusStateEv", func!(crate::horizon::oe::get_current_focus_state())),
    Entry("_ZN2nn2oe20SetFocusHandlingModeENS0_17FocusHandlingModeE", func!(crate::horizon::oe::set_focus_handling_mode(_))),
    Entry("_ZN2nn2oe22PopNotificationMessageEv", func!(crate::horizon::oe::pop_notification_message())),
    // nn::oe / pl / account / err
    Entry("_ZN2nn2oe24GetExpectedVolumeBalanceEPfS1_", func!(crate::horizon::oe::get_expected_volume_balance(_, _))),
    Entry("_ZN2nn2oe24SetExpectedVolumeBalanceEff", func!(crate::horizon::oe::set_expected_volume_balance(_, _))),
    Entry("_ZN2nn2oe25GetProgramTotalActiveTimeEv", func!(crate::horizon::oe::get_program_total_active_time())),
    Entry("_ZN2nn2oe21TryPopLaunchParameterEPmPvm", func!(crate::horizon::oe::try_pop_launch_parameter(_, _, _))),
    Entry("_ZN2nn2oe25TryPopNotificationMessageEPj", func!(crate::horizon::oe::try_pop_notification_message(_))),
    Entry("_ZN2nn2oe27GetPerformanceConfigurationENS0_15PerformanceModeE", func!(crate::horizon::oe::get_performance_configuration(_))),
    Entry("_ZN2nn2oe27SetPerformanceConfigurationENS0_15PerformanceModeEi", func!(crate::horizon::oe::set_performance_configuration(_, _))),
    Entry("_ZN2nn2oe27SetPerformanceConfigurationEi", func!(crate::horizon::oe::set_performance_configuration_all(_))),
    Entry("_ZN2nn2oe27GetNotificationMessageEventEv", func!(crate::horizon::oe::get_notification_message_event())),
    Entry("_ZN2nn2oe27GetDefaultDisplayResolutionEPiS1_", func!(crate::horizon::oe::get_default_display_resolution(_, _))),
    Entry("_ZN2nn2oe28SetResumeNotificationEnabledEb", func!(crate::horizon::oe::set_resume_notification_enabled(_))),
    // Tells the system how to rotate captured screenshots (IPC to the applet manager); there is none to tell.
    Entry("_ZN2nn2oe29SetScreenShotImageOrientationENS_5album16ImageOrientationE", func!(crate::horizon::nn::noop_void())),
    // Screenshot and video capture switches (IPC to the applet manager, newer SDKs call some natively): there is no capture
    // to switch. BotW 1.9.0 calls EnableRecording right after its first frame.
    Entry("_ZN2nn2oe15EnableRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe16DisableRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe13MuteRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe15UnmuteRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe22PauseGamePlayRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe24RestartGamePlayRecordingEv", func!(crate::horizon::nn::noop_void())),
    Entry("_ZN2nn2oe31EnterExitRequestHandlingSectionEv", func!(crate::horizon::oe::enter_exit_request_handling_section())),
    Entry("_ZN2nn2oe31LeaveExitRequestHandlingSectionEv", func!(crate::horizon::oe::leave_exit_request_handling_section())),
    Entry("_ZN2nn2oe38SetUserInactivityDetectionTimeExtendedEb", func!(crate::horizon::oe::set_user_inactivity_detection_time_extended(_))),
    Entry("_ZN2nn2oe42SetOperationModeChangedNotificationEnabledEb", func!(crate::horizon::oe::set_operation_mode_changed_notification_enabled(_))),
    Entry("_ZN2nn2oe44SetPerformanceModeChangedNotificationEnabledEb", func!(crate::horizon::oe::set_performance_mode_changed_notification_enabled(_))),
];
