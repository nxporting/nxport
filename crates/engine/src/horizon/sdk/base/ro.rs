// nn::ro.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2ro10InitializeEv", func!(crate::guest::module_loader::ro_initialize())),
    Entry("_ZN2nn2ro8FinalizeEv", func!(crate::guest::module_loader::ro_finalize())),
    Entry("_ZN2nn2ro13GetBufferSizeEPmPKv", func!(crate::guest::module_loader::ro_get_buffer_size(_, _))),
    Entry("_ZN2nn2ro12LookupSymbolEPmPKc", func!(crate::guest::module_loader::ro_lookup_symbol(_, _))),
    Entry("_ZN2nn2ro18LookupModuleSymbolEPmPKNS0_6ModuleEPKc", func!(crate::guest::module_loader::ro_lookup_module_symbol(_, _, _))),
    Entry("_ZN2nn2ro18RegisterModuleInfoEPNS0_16RegistrationInfoEPKv", func!(crate::guest::module_loader::ro_register_module_info(_, _))),
    Entry("_ZN2nn2ro20UnregisterModuleInfoEPKNS0_16RegistrationInfoE", func!(crate::guest::module_loader::ro_unregister_module_info(_))),
    Entry("_ZN2nn2ro10LoadModuleEPNS0_6ModuleEPKvPvmi", func!(crate::guest::module_loader::ro_load_module(_, _, _, _, _))),
    Entry("_ZN2nn2ro10LoadModuleEPNS0_6ModuleEPKvPvmib", func!(crate::guest::module_loader::ro_load_module_sdk19(_, _, _, _, _, _))),
    Entry("_ZN2nn2ro12UnloadModuleEPNS0_6ModuleE", func!(crate::guest::module_loader::ro_unload_module(_))),
];
