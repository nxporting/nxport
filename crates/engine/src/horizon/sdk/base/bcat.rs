// nn::bcat.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::bcat
    Entry("_ZN2nn4bcat10InitializeEv", func!(crate::horizon::nn::bcat_initialize())),
    Entry("_ZN2nn4bcat17DeliveryCacheFile4OpenEPKc", func!(crate::horizon::nn::bcat_open_not_found(_, _))),
    Entry("_ZN2nn4bcat17DeliveryCacheFile4ReadEPmlPvm", func!(crate::horizon::nn::bcat_file_read(_, _, _, _, _))),
    Entry("_ZN2nn4bcat17DeliveryCacheFile5CloseEv", func!(crate::horizon::nn::bcat_close(_))),
    Entry("_ZN2nn4bcat21DeliveryCacheProgress6UpdateEv", func!(crate::horizon::nn::bcat_progress_update(_))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory4OpenEPKc", func!(crate::horizon::nn::bcat_open_not_found(_, _))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory4ReadEPiPNS0_27DeliveryCacheDirectoryEntryEi", func!(crate::horizon::nn::bcat_directory_read(_, _, _, _))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory5CloseEv", func!(crate::horizon::nn::bcat_close(_))),
    Entry("_ZN2nn4bcat24RequestSyncDeliveryCacheEPNS0_21DeliveryCacheProgressE", func!(crate::horizon::nn::bcat_request_sync_delivery_cache(_))),
    Entry("_ZN2nn4bcat25MountDeliveryCacheStorageEv", func!(crate::horizon::nn::bcat_mount_delivery_cache_storage())),
    Entry("_ZN2nn4bcat27UnmountDeliveryCacheStorageEv", func!(crate::horizon::nn::bcat_unmount_delivery_cache_storage())),
    Entry("_ZN2nn4bcat31EnumerateDeliveryCacheDirectoryEPiPNS0_13DirectoryNameEi", func!(crate::horizon::nn::bcat_enumerate_delivery_cache_directory(_, _, _))),
    Entry("_ZNK2nn4bcat21DeliveryCacheProgress9GetResultEv", func!(crate::horizon::nn::bcat_progress_get_result(_))),
    Entry("_ZNK2nn4bcat21DeliveryCacheProgress9GetStatusEv", func!(crate::horizon::nn::bcat_progress_get_status(_))),
];
