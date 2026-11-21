// nn::irsensor (horizon/irsensor.rs). MakeRect and the Get*DefaultConfig functions need no service: the SDK's own run.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn8irsensor17GetIrCameraHandleERKj", func!(crate::horizon::irsensor::get_ir_camera_handle(_))),
    Entry("_ZN2nn8irsensor10InitializeERKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor8FinalizeERKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor23StopImageProcessorAsyncERKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor18StopImageProcessorERKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor18RunMomentProcessorERKNS0_14IrCameraHandleERKNS0_21MomentProcessorConfigE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor22RunClusteringProcessorERKNS0_14IrCameraHandleERKNS0_25ClusteringProcessorConfigE", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor25RunImageTransferProcessorERKNS0_14IrCameraHandleERKNS0_30ImageTransferProcessorExConfigEPvm", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor25RunImageTransferProcessorERKNS0_14IrCameraHandleERKNS0_28ImageTransferProcessorConfigEPvm", func!(crate::horizon::irsensor::camera_noop())),
    Entry("_ZN2nn8irsensor17GetIrCameraStatusERKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_ir_camera_status(_))),
    Entry("_ZN2nn8irsensor24GetMomentProcessorStatesEPNS0_20MomentProcessorStateEPiiRKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_states(_, _))),
    Entry("_ZN2nn8irsensor28GetClusteringProcessorStatesEPNS0_24ClusteringProcessorStateEPiiRKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_states(_, _))),
    Entry("_ZN2nn8irsensor30GetImageTransferProcessorStateEPNS0_27ImageTransferProcessorStateEPvmRKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_image_state())),
    Entry("_ZN2nn8irsensor30GetHandAnalysisSilhouetteStateEPNS0_27HandAnalysisSilhouetteStateEPiilRKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_states(_, _))),
    Entry("_ZN2nn8irsensor25GetHandAnalysisImageStateEPNS0_22HandAnalysisImageStateEPiilRKNS0_14IrCameraHandleE", func!(crate::horizon::irsensor::get_states(_, _))),
];
