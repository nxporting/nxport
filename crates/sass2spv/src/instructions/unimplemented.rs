// Instructions the original does not implement: they only log (InstEmit.cs).

use crate::translation::emitter::{Emit, EmitterContext as Ctx};

macro_rules! unimplemented_insts {
    ($($name:ident => $text:literal),* $(,)?) => {$(
        pub fn $name(c: &mut Ctx) {
            c.log(concat!("Shader instruction ", $text, " is not implemented."));
        }
    )*};
}

unimplemented_insts!(
    atom_cas => "AtomCas",
    atoms_cas => "AtomsCas",
    b2r => "B2r",
    bpt => "Bpt",
    cctl => "Cctl",
    cctll => "Cctll",
    cctlt => "Cctlt",
    cs2r => "Cs2r",
    fchk_r => "FchkR",
    fchk_i => "FchkI",
    fchk_c => "FchkC",
    getcrsptr => "Getcrsptr",
    getlmembase => "Getlmembase",
    ide => "Ide",
    idp_r => "IdpR",
    idp_c => "IdpC",
    imadsp_r => "ImadspR",
    imadsp_i => "ImadspI",
    imadsp_c => "ImadspC",
    imadsp_rc => "ImadspRc",
    jcal => "Jcal",
    jmp => "Jmp",
    jmx => "Jmx",
    ld => "Ld",
    lepc => "Lepc",
    longjmp => "Longjmp",
    pexit => "Pexit",
    pixld => "Pixld",
    plongjmp => "Plongjmp",
    pret => "Pret",
    prmt_r => "PrmtR",
    prmt_i => "PrmtI",
    prmt_c => "PrmtC",
    prmt_rc => "PrmtRc",
    r2b => "R2b",
    ram => "Ram",
    rtt => "Rtt",
    sam => "Sam",
    setcrsptr => "Setcrsptr",
    setlmembase => "Setlmembase",
    st => "St",
    stp => "Stp",
    txa => "Txa",
    vabsdiff => "Vabsdiff",
    vabsdiff4 => "Vabsdiff4",
    vadd => "Vadd",
    votevtg => "Votevtg",
    vset => "Vset",
    vshl => "Vshl",
    vshr => "Vshr",
);

/// No operation (InstEmitNop).
pub fn nop(_c: &mut Ctx) {}
