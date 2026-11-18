// nn::pdm as the game calls it: play statistics, reported as none.

/// nn::pdm::QueryApplicationPlayStatistics(out, uid, appIds, count) -> number of entries written: no play records.
pub(crate) unsafe extern "C" fn pdm_query_application_play_statistics(
    _out: *mut u8,
    _uid: *const u8,
    _ids: *const u8,
    _count: i32,
) -> i32 {
    0
}
