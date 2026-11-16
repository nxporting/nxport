// nn::bcat.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::bcat
    Entry("_ZN2nn4bcat10InitializeEv", func!(crate::horizon::bcat::initialize())),
    Entry("_ZN2nn4bcat17DeliveryCacheFile4OpenEPKc", func!(crate::horizon::bcat::open_not_found(_, _))),
    Entry("_ZN2nn4bcat17DeliveryCacheFile4ReadEPmlPvm", func!(crate::horizon::bcat::file_read(_, _, _, _, _))),
    Entry("_ZN2nn4bcat17DeliveryCacheFile5CloseEv", func!(crate::horizon::bcat::close(_))),
    Entry("_ZN2nn4bcat21DeliveryCacheProgress6UpdateEv", func!(crate::horizon::bcat::progress_update(_))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory4OpenEPKc", func!(crate::horizon::bcat::open_not_found(_, _))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory4ReadEPiPNS0_27DeliveryCacheDirectoryEntryEi", func!(crate::horizon::bcat::directory_read(_, _, _, _))),
    Entry("_ZN2nn4bcat22DeliveryCacheDirectory5CloseEv", func!(crate::horizon::bcat::close(_))),
    Entry("_ZN2nn4bcat24RequestSyncDeliveryCacheEPNS0_21DeliveryCacheProgressE", func!(crate::horizon::bcat::request_sync_delivery_cache(_))),
    Entry("_ZN2nn4bcat25MountDeliveryCacheStorageEv", func!(crate::horizon::bcat::mount_delivery_cache_storage())),
    Entry("_ZN2nn4bcat27UnmountDeliveryCacheStorageEv", func!(crate::horizon::bcat::unmount_delivery_cache_storage())),
    Entry("_ZN2nn4bcat31EnumerateDeliveryCacheDirectoryEPiPNS0_13DirectoryNameEi", func!(crate::horizon::bcat::enumerate_delivery_cache_directory(_, _, _))),
    Entry("_ZNK2nn4bcat21DeliveryCacheProgress9GetResultEv", func!(crate::horizon::bcat::progress_get_result(_))),
    Entry("_ZNK2nn4bcat21DeliveryCacheProgress9GetStatusEv", func!(crate::horizon::bcat::progress_get_status(_))),
];
