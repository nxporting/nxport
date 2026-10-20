// nn::account.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn7account10InitializeEv", func!(crate::horizon::hle::hle_nn_account_Initialize)),
    Entry("_ZN2nn7account11GetNicknameEPNS0_8NicknameERKNS0_3UidE", func!(crate::horizon::hle::hle_nn_account_GetNickname)),
    Entry("_ZN2nn7account12AsyncContext14GetSystemEventEPNS_2os11SystemEventE", func!(crate::horizon::nn::account_async_get_system_event)),
    Entry("_ZN2nn7account12AsyncContext6CancelEv", func!(crate::horizon::nn::account_async_cancel)),
    Entry("_ZN2nn7account12AsyncContext7HasDoneEPb", func!(crate::horizon::nn::account_async_has_done)),
    Entry("_ZN2nn7account12AsyncContext9GetResultEv", func!(crate::horizon::nn::account_async_get_result)),
    Entry("_ZN2nn7account12GetUserCountEPi", func!(crate::horizon::nn::account_get_user_count)),
    Entry("_ZN2nn7account12ListAllUsersEPiPNS0_3UidEi", func!(crate::horizon::nn::account_list_users)),
    // The one user, opened at startup (OpenPreselectedUser).
    Entry("_ZN2nn7account13ListOpenUsersEPiPNS0_3UidEi", func!(crate::horizon::nn::account_list_users)),
    Entry("_ZN2nn7account16GetUserExistenceEPbRKNS0_3UidE", func!(crate::horizon::nn::account_get_user_existence)),
    Entry("_ZN2nn7account16LoadProfileImageEPmPvmRKNS0_3UidE", func!(crate::horizon::nn::account_load_profile_image)),
    Entry("_ZN2nn7account16ShowUserSelectorEPNS0_3UidERKNS0_21UserSelectionSettingsE", func!(crate::horizon::nn::account_show_user_selector)),
    Entry("_ZN2nn7account17GetLastOpenedUserEPNS0_3UidE", func!(crate::horizon::hle::hle_nn_account_GetLastOpenedUser)),
    Entry("_ZN2nn7account18ListQualifiedUsersEPiPNS0_3UidEi", func!(crate::horizon::nn::account_list_users)),
    Entry("_ZN2nn7account19OpenPreselectedUserEPNS0_10UserHandleE", func!(crate::horizon::hle::hle_nn_account_OpenPreselectedUser)),
    Entry("_ZN2nn7account22TryOpenPreselectedUserEPNS0_10UserHandleE", func!(crate::horizon::hle::hle_nn_account_TryOpenPreselectedUser)),
    Entry("_ZN2nn7account26GetNetworkServiceAccountIdEPNS0_23NetworkServiceAccountIdERKNS0_10UserHandleE", func!(crate::horizon::nn::account_nsa_unavailable_2)),
    Entry("_ZN2nn7account29LoadCachedNintendoAccountInfoEPNS0_25CachedNintendoAccountInfoERKNS0_10UserHandleEPvm", func!(crate::horizon::nn::account_nsa_unavailable_4)),
    Entry("_ZN2nn7account32IsNetworkServiceAccountAvailableEPbRKNS0_10UserHandleE", func!(crate::horizon::nn::account_out_false)),
    // nn::account network service account
    Entry("_ZN2nn7account36EnsureNetworkServiceAccountAvailableERKNS0_10UserHandleE", func!(crate::horizon::nn::account_nsa_unavailable_1)),
    Entry("_ZN2nn7account37LoadNetworkServiceAccountIdTokenCacheEPmPcmRKNS0_10UserHandleE", func!(crate::horizon::nn::account_load_nsa_id_token_cache)),
    Entry("_ZN2nn7account44EnsureNetworkServiceAccountIdTokenCacheAsyncEPNS0_12AsyncContextERKNS0_10UserHandleE", func!(crate::horizon::nn::account_nsa_unavailable_2)),
    Entry("_ZN2nn7account8OpenUserEPNS0_10UserHandleERKNS0_3UidE", func!(crate::horizon::nn::account_open_user)),
    Entry("_ZN2nn7account9CloseUserERKNS0_10UserHandleE", func!(crate::horizon::hle::hle_nn_account_CloseUser)),
    Entry("_ZN2nn7account9GetUserIdEPNS0_3UidERKNS0_10UserHandleE", func!(crate::horizon::hle::hle_nn_account_GetUserId)),
];
