// nn::mii.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn3mii8Database10InitializeEv", func!(crate::horizon::mii::db_initialize)),
    Entry("_ZN2nn3mii8Database11BuildRandomEPNS0_8CharInfoERKNS0_10RandomDescE", func!(crate::horizon::mii::db_build_random)),
    Entry("_ZN2nn3mii8Database3GetEPiPNS0_15CharInfoElementEii", func!(crate::horizon::mii::db_get_elements)),
    Entry("_ZN2nn3mii8Database3GetEPiPNS0_8CharInfoEii", func!(crate::horizon::mii::db_get_infos)),
    Entry("_ZN2nn3mii8Database6ImportEPKvm", func!(crate::horizon::mii::db_import)),
    Entry("_ZN2nn3mii8Database8FinalizeEv", func!(crate::horizon::mii::db_finalize)),
    Entry("_ZN2nn3mii9AppendMiiEPi", func!(crate::horizon::mii::append_mii)),
    Entry("_ZNK2nn3mii8Database12UpdateLatestEPNS0_8CharInfoEi", func!(crate::horizon::mii::db_not_found)),
    Entry("_ZNK2nn3mii8Database14IsFullDatabaseEv", func!(crate::horizon::mii::db_is_full)),
    Entry("_ZNK2nn3mii8Database8GetIndexEPiRKNS0_8CharInfoE", func!(crate::horizon::mii::db_not_found)),
];
