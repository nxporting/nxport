// nn::friends.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::friends (offline, no friends)
    Entry("_ZN2nn7friends10InitializeEv", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12AsyncContext6CancelEv", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12AsyncContext14GetSystemEventEPNS_2os11SystemEventE", func!(crate::horizon::nn::friends_get_system_event(_))),
    Entry("_ZNK2nn7friends12AsyncContext9GetResultEv", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12AsyncContext7HasDoneEPb", func!(crate::horizon::nn::friends_out_true(_))),
    Entry("_ZN2nn7friends12AsyncContextC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12AsyncContextC2Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12AsyncContextD1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12AsyncContextD2Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12SetAllocatorEPFPvmEPFvS1_mE", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends13GetFriendListEPiPNS0_6FriendERKNS_7account3UidEiiRKNS0_12FriendFilterE", func!(crate::horizon::nn::friends_get_friend_list(_))),
    Entry("_ZN2nn7friends14AddPlayHistoryERKNS_7account3UidERKNS0_26PlayHistoryRegistrationKeyERKNS0_15InAppScreenNameESA_", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends25EnsureFriendListAvailableEPNS0_12AsyncContextERKNS_7account3UidE", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends27CheckFriendListAvailabilityEPbRKNS_7account3UidE", func!(crate::horizon::nn::friends_out_true(_))),
    Entry("_ZN2nn7friends29GetPlayHistoryRegistrationKeyEPNS0_26PlayHistoryRegistrationKeyERKNS_7account3UidEb", func!(crate::horizon::nn::friends_get_play_history_key(_))),
    Entry("_ZN2nn7friends6FriendC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZNK2nn7friends14FriendPresence9GetStatusEv", func!(crate::horizon::nn::friends_zero())),
    Entry("_ZNK2nn7friends6Friend11GetNicknameEv", func!(crate::horizon::nn::friends_empty_ref())),
    Entry("_ZNK2nn7friends6Friend11GetPresenceEv", func!(crate::horizon::nn::friends_empty_ref())),
    Entry("_ZNK2nn7friends6Friend12GetAccountIdEv", func!(crate::horizon::nn::friends_zero())),
    Entry("_ZNK2nn7friends6Friend15GetProfileImageEPmPvm", func!(crate::horizon::nn::friends_not_found())),
    Entry("_ZNK2nn7friends6Friend7IsValidEv", func!(crate::horizon::nn::friends_false())),

    // UserPresence
    Entry("_ZN2nn7friends12UserPresenceC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12UserPresenceC2Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12UserPresence10InitializeERKNS_7account3UidE", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12UserPresence10InitializeEv", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12UserPresence11SetAppValueEPKcS3_", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12UserPresence14SetDescriptionEPKc", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12UserPresence15SetAppValueImplEPKcS3_", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends12UserPresence28DeclareOpenOnlinePlaySessionEv", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12UserPresence29DeclareCloseOnlinePlaySessionEv", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12UserPresence5ClearEv", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends12UserPresence6CommitEv", func!(crate::horizon::nn::friends_ok())),

    // UserPresenceView
    Entry("_ZN2nn7friends16UserPresenceViewC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends16UserPresenceViewC2Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends19GetUserPresenceViewEPNS0_16UserPresenceViewERKNS_7account3UidE", func!(crate::horizon::nn::friends_ok())),

    // NotificationQueue
    Entry("_ZN2nn7friends17NotificationQueue10InitializeERKNS_7account3UidE", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends17NotificationQueue10InitializeEv", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends17NotificationQueue5ClearEv", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends17NotificationQueueC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends17NotificationQueueC2Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends17NotificationQueueD1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn7friends17NotificationQueueD2Ev", func!(crate::horizon::nn::friends_void())),

    // IPC detail initialization stubs (prevent SDK from opening un-emulated friend:u)
    Entry("_ZN2nn7friends6detail3ipc10InitializeEv", func!(crate::horizon::nn::friends_ok())),
    Entry("_ZN2nn7friends6detail3ipc8FinalizeEv", func!(crate::horizon::nn::friends_void())),
];
