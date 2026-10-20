use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct InstProps: u16 {
        const NONE = 0;
        const RD = 1 << 0;
        const RD2 = 1 << 1;
        const RA = 1 << 2;
        const RB = 1 << 3;
        const RB2 = 1 << 4;
        const IB = 1 << 5;
        const RC = 1 << 6;

        const PD = 1 << 7;
        const LPD = 2 << 7;
        const SPD = 3 << 7;
        const TPD = 4 << 7;
        const VPD = 5 << 7;
        const PD_MASK = 7 << 7;

        const PDN = 1 << 10;
        const PS = 1 << 11;
        const TEX = 1 << 12;
        const TEX_B = 1 << 13;
        const BRA = 1 << 14;
        const NO_PRED = 1 << 15;
    }
}
