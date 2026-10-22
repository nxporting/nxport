// nn::time.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::time
    Entry("_ZN2nn4time10InitializeEv", func!(crate::horizon::nn::time_initialize())),
    Entry("_ZN2nn4time13ClockSnapshot29CreateWithStandardSystemClockEPS1_", func!(crate::horizon::nn::time_clock_snapshot_create(_))),
    Entry("_ZN2nn4time13ClockSnapshotC1Ev", func!(crate::horizon::nn::time_clock_snapshot_ctor(_))),
    Entry("_ZN2nn4time13IsInitializedEv", func!(crate::horizon::nn::time_is_initialized())),
    Entry("_ZN2nn4time14ToCalendarTimeEPNS0_12CalendarTimeEPNS0_22CalendarAdditionalInfoERKNS0_9PosixTimeE", func!(crate::horizon::nn::time_to_calendar_time(_, _, _))),
    Entry("_ZN2nn4time18ToPosixTimeFromUtcERKNS0_12CalendarTimeE", func!(crate::horizon::nn::time_to_posix_time_from_utc(_))),
    Entry("_ZN2nn4time19StandardSteadyClock3nowEv", func!(crate::horizon::nn::time_steady_clock_now())),
    Entry("_ZN2nn4time19ToCalendarTimeInUtcERKNS0_9PosixTimeE", func!(crate::horizon::nn::time_to_calendar_time_in_utc(_))),
    Entry("_ZN2nn4time20CalculateSpanBetweenEPNS_8TimeSpanERKNS0_13ClockSnapshotES5_", func!(crate::horizon::nn::time_calculate_span_between(_, _, _))),
    Entry("_ZN2nn4time21GetDeviceLocationNameEPNS0_12LocationNameE", func!(crate::horizon::nn::time_get_device_location_name(_))),
    Entry("_ZN2nn4time23StandardUserSystemClock14GetCurrentTimeEPNS0_9PosixTimeE", func!(crate::horizon::nn::time_clock_get_current_time(_))),
    Entry("_ZN2nn4time26StandardNetworkSystemClock14GetCurrentTimeEPNS0_9PosixTimeE", func!(crate::horizon::nn::time_clock_get_current_time(_))),
    Entry("_ZN2nn4time48CalculateStandardUserSystemClockDifferenceByUserERKNS0_13ClockSnapshotES3_", func!(crate::horizon::nn::time_user_clock_difference(_, _))),
    Entry("_ZN2nn4time8FinalizeEv", func!(crate::horizon::nn::time_finalize())),
];
