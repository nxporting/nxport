// nn::irsensor: the Joy-Con IR camera. Our controllers have none, so the camera reports itself unsupported and every
// state query answers ResultIrsensorUnsupported with no data. The SDK's own versions open the irs service over IPC
// (Super Mario Party Jamboree aborted at GetIrCameraHandle).

/// ResultIrsensorUnsupported: module 205, description 111.
const RESULT_UNSUPPORTED: u32 = 205 | 111 << 9;
/// IrCameraStatus_Unsupported.
const STATUS_UNSUPPORTED: u32 = 1;

/// GetIrCameraHandle(const NpadIdType&): IrCameraHandle { u32 } naming the Npad.
pub(crate) unsafe extern "C" fn get_ir_camera_handle(npad_id: *const u32) -> u32 {
    if npad_id.is_null() {
        0
    } else {
        *npad_id
    }
}

/// Initialize / Finalize / StopImageProcessorAsync / Run*Processor (void): nothing to start or stop.
pub(crate) unsafe extern "C" fn camera_noop() {}

/// GetIrCameraStatus(const IrCameraHandle&).
pub(crate) unsafe extern "C" fn get_ir_camera_status(_handle: *const u32) -> u32 {
    STATUS_UNSUPPORTED
}

/// Get*ProcessorStates(State*, int* count, int max, ...) and GetHandAnalysis*State(..., int* count, ...): no states.
pub(crate) unsafe extern "C" fn get_states(_out: *mut u8, count: *mut i32) -> u32 {
    if !count.is_null() {
        *count = 0;
    }
    RESULT_UNSUPPORTED
}

/// GetImageTransferProcessorState(State*, void* image, size_t, const IrCameraHandle&): no image.
pub(crate) unsafe extern "C" fn get_image_state() -> u32 {
    RESULT_UNSUPPORTED
}
