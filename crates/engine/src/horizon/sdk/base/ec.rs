// nn::ec.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2ec19ShopServiceAccessor10InitializeERKNS0_11ShopServiceE", func!(crate::horizon::nn::ec_accessor_initialize)),
    Entry("_ZN2nn2ec19ShopServiceAccessor13AsyncResponse3GetEPvm", func!(crate::horizon::nn::ec_response_get)),
    Entry("_ZN2nn2ec19ShopServiceAccessor13AsyncResponse6CancelEv", func!(crate::horizon::nn::ec_response_cancel)),
    Entry("_ZN2nn2ec19ShopServiceAccessor13AsyncResponse7GetSizeEPm", func!(crate::horizon::nn::ec_response_get_size)),
    Entry("_ZN2nn2ec19ShopServiceAccessor13AsyncResponse7TryWaitEv", func!(crate::horizon::nn::ec_response_try_wait)),
    Entry("_ZN2nn2ec19ShopServiceAccessor7RequestEPNS1_13AsyncResponseERKNS_7account3UidENS0_11ShopService6MethodEPKcRKNS1_8PostDataENS_8TimeSpanE", func!(crate::horizon::nn::ec_accessor_request)),
    Entry("_ZN2nn2ec19ShopServiceAccessor8FinalizeEv", func!(crate::horizon::nn::ec_accessor_finalize)),
    Entry("_ZN2nn2ec22ShowShopProductDetailsENS0_5NsUidERKNS_7account10UserHandleE", func!(crate::horizon::nn::ec_show_shop_product_details)),
    Entry("_ZN2nn2ec31FinalizeForShopServiceAccessorsEv", func!(crate::horizon::nn::ec_finalize_for_shop_service_accessors)),
    // nn::ec
    Entry("_ZN2nn2ec33InitializeForShopServiceAccessorsEPvm", func!(crate::horizon::nn::ec_initialize_for_shop_service_accessors)),
];
