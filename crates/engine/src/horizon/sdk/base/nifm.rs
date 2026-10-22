// nn::nifm.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn4nifm10InitializeEv", func!(crate::guest::module_loader::nifm_initialize())),
    Entry("_ZN2nn4nifm18IsNetworkAvailableEv", func!(crate::guest::module_loader::nifm_available())),
    // nn::nifm
    Entry("_ZN2nn4nifm20CancelNetworkRequestEv", func!(crate::horizon::nn::nifm_cancel_network_request())),
    Entry("_ZN2nn4nifm20SubmitNetworkRequestEv", func!(crate::guest::module_loader::nifm_submit_network_request())),
    Entry("_ZN2nn4nifm22GetCurrentProxySettingEPNS0_12ProxySettingE", func!(crate::horizon::nn::nifm_no_internet_connection(_))),
    Entry("_ZN2nn4nifm22IsNetworkRequestOnHoldEv", func!(crate::guest::module_loader::nifm_false())),
    Entry("_ZN2nn4nifm26GetCurrentPrimaryIpAddressEP7in_addr", func!(crate::horizon::nn::nifm_no_internet_connection(_))),
    Entry("_ZN2nn4nifm31HandleNetworkRequestErrorResultEv", func!(crate::horizon::nn::nifm_handle_network_request_error_result())),
];
