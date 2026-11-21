// Every SDK release games are known to use (the sdk module versions in titledb.nintendo-research.com's tables), oldest
// first. A release here with no layer of its own runs on the closest older layers; the log says
// which.

use super::SdkVersion;

pub(super) static RELEASES: &[SdkVersion] = &[
    SdkVersion(0, 12, 12), SdkVersion(0, 12, 13), SdkVersion(0, 12, 14), SdkVersion(0, 12, 16), SdkVersion(0, 12, 17),
    SdkVersion(1, 3, 0), SdkVersion(1, 3, 1), SdkVersion(1, 4, 0), SdkVersion(1, 5, 0), SdkVersion(1, 6, 0),
    SdkVersion(1, 7, 0), SdkVersion(3, 4, 0), SdkVersion(3, 4, 1), SdkVersion(3, 5, 0), SdkVersion(3, 5, 1),
    SdkVersion(3, 5, 2), SdkVersion(3, 5, 3), SdkVersion(3, 6, 0), SdkVersion(3, 7, 0), SdkVersion(3, 8, 0),
    SdkVersion(4, 4, 0), SdkVersion(4, 4, 1), SdkVersion(4, 4, 2), SdkVersion(4, 4, 3), SdkVersion(4, 5, 0),
    SdkVersion(4, 5, 2), SdkVersion(5, 3, 0), SdkVersion(5, 4, 0), SdkVersion(5, 4, 150), SdkVersion(5, 4, 151),
    SdkVersion(5, 4, 180), SdkVersion(5, 5, 0), SdkVersion(6, 4, 0), SdkVersion(6, 4, 2), SdkVersion(6, 5, 0),
    SdkVersion(6, 6, 0), SdkVersion(7, 3, 0), SdkVersion(7, 3, 2), SdkVersion(7, 4, 0), SdkVersion(7, 6, 0),
    SdkVersion(8, 2, 0), SdkVersion(8, 2, 1), SdkVersion(8, 3, 0), SdkVersion(8, 3, 1), SdkVersion(8, 3, 2),
    SdkVersion(8, 3, 3), SdkVersion(9, 3, 0), SdkVersion(9, 3, 1), SdkVersion(9, 3, 4), SdkVersion(9, 4, 0),
    SdkVersion(9, 4, 1), SdkVersion(9, 5, 0), SdkVersion(9, 5, 1), SdkVersion(9, 6, 0), SdkVersion(10, 4, 0),
    SdkVersion(10, 4, 1), SdkVersion(10, 4, 3), SdkVersion(10, 4, 4), SdkVersion(10, 4, 8), SdkVersion(10, 5, 1),
    SdkVersion(10, 6, 0), SdkVersion(10, 7, 0), SdkVersion(10, 8, 0), SdkVersion(10, 8, 1), SdkVersion(10, 8, 2),
    SdkVersion(10, 9, 1), SdkVersion(10, 9, 3), SdkVersion(10, 9, 4), SdkVersion(10, 9, 8), SdkVersion(11, 4, 0),
    SdkVersion(11, 4, 3), SdkVersion(12, 3, 1), SdkVersion(12, 3, 2), SdkVersion(12, 3, 3), SdkVersion(12, 3, 7),
    SdkVersion(12, 3, 11), SdkVersion(13, 3, 0), SdkVersion(13, 3, 2), SdkVersion(13, 3, 4), SdkVersion(13, 3, 5),
    SdkVersion(13, 3, 6), SdkVersion(13, 3, 9), SdkVersion(13, 3, 11), SdkVersion(14, 3, 0), SdkVersion(14, 3, 1),
    SdkVersion(15, 3, 0), SdkVersion(15, 3, 1), SdkVersion(15, 3, 2), SdkVersion(15, 3, 3), SdkVersion(16, 2, 0),
    SdkVersion(16, 2, 3), SdkVersion(16, 2, 6), SdkVersion(16, 2, 12), SdkVersion(17, 5, 4), SdkVersion(17, 5, 6),
    SdkVersion(17, 5, 7), SdkVersion(17, 5, 8), SdkVersion(18, 3, 0), SdkVersion(18, 3, 1), SdkVersion(18, 3, 2),
    SdkVersion(18, 3, 4), SdkVersion(18, 3, 5), SdkVersion(18, 3, 7), SdkVersion(19, 3, 0), SdkVersion(19, 3, 3),
    SdkVersion(19, 3, 5), SdkVersion(19, 3, 9), SdkVersion(19, 3, 11), SdkVersion(19, 3, 13), SdkVersion(20, 5, 3),
    SdkVersion(20, 5, 4), SdkVersion(20, 5, 6), SdkVersion(20, 5, 14), SdkVersion(20, 5, 15), SdkVersion(20, 5, 17),
    SdkVersion(20, 5, 19), SdkVersion(20, 5, 21), SdkVersion(20, 5, 27), SdkVersion(20, 5, 28), SdkVersion(20, 5, 29),
    SdkVersion(21, 4, 4), SdkVersion(21, 4, 7), SdkVersion(22, 2, 0),
];

#[cfg(test)]
mod tests {
    #[test]
    fn sorted_without_duplicates() {
        assert!(super::RELEASES.windows(2).all(|w| w[0] < w[1]));
    }
}
