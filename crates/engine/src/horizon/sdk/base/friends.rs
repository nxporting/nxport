// nn::friends.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::friends (offline, no friends)
    Entry("_ZN2nn7friends10InitializeEv", func!(crate::horizon::nn::friends_void)),
    Entry("_ZN2nn7friends12AsyncContext7HasDoneEPb", func!(crate::horizon::nn::friends_out_true)),
    Entry("_ZN2nn7friends12AsyncContextC1Ev", func!(crate::horizon::nn::friends_void)),
    Entry("_ZN2nn7friends12AsyncContextD1Ev", func!(crate::horizon::nn::friends_void)),
    Entry("_ZN2nn7friends12SetAllocatorEPFPvmEPFvS1_mE", func!(crate::horizon::nn::friends_void)),
    Entry("_ZN2nn7friends13GetFriendListEPiPNS0_6FriendERKNS_7account3UidEiiRKNS0_12FriendFilterE", func!(crate::horizon::nn::friends_get_friend_list)),
    Entry("_ZN2nn7friends14AddPlayHistoryERKNS_7account3UidERKNS0_26PlayHistoryRegistrationKeyERKNS0_15InAppScreenNameESA_", func!(crate::horizon::nn::friends_ok)),
    Entry("_ZN2nn7friends25EnsureFriendListAvailableEPNS0_12AsyncContextERKNS_7account3UidE", func!(crate::horizon::nn::friends_ok)),
    Entry("_ZN2nn7friends27CheckFriendListAvailabilityEPbRKNS_7account3UidE", func!(crate::horizon::nn::friends_out_true)),
    Entry("_ZN2nn7friends29GetPlayHistoryRegistrationKeyEPNS0_26PlayHistoryRegistrationKeyERKNS_7account3UidEb", func!(crate::horizon::nn::friends_get_play_history_key)),
    Entry("_ZN2nn7friends6FriendC1Ev", func!(crate::horizon::nn::friends_void)),
    Entry("_ZNK2nn7friends14FriendPresence9GetStatusEv", func!(crate::horizon::nn::friends_zero)),
    Entry("_ZNK2nn7friends6Friend11GetNicknameEv", func!(crate::horizon::nn::friends_empty_ref)),
    Entry("_ZNK2nn7friends6Friend11GetPresenceEv", func!(crate::horizon::nn::friends_empty_ref)),
    Entry("_ZNK2nn7friends6Friend12GetAccountIdEv", func!(crate::horizon::nn::friends_zero)),
    Entry("_ZNK2nn7friends6Friend15GetProfileImageEPmPvm", func!(crate::horizon::nn::friends_not_found)),
    Entry("_ZNK2nn7friends6Friend7IsValidEv", func!(crate::horizon::nn::friends_false)),
];
