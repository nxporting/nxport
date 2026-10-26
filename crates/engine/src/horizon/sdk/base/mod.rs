// The base layer: everything bound whose first SDK version is not known (older than every version looked at),
// one table per library. A name ending in * matches by prefix.

mod account;
mod aoc;
mod apm;
mod audio;
mod bcat;
mod codec;
mod cxx;
mod detail;
mod diag;
mod ec;
mod err;
mod friends;
mod fs;
mod hid;
mod libc;
mod ldn;
mod mii;
mod nfp;
mod ngc;
mod nifm;
mod nv;
mod nvn;
mod oe;
mod os;
mod pdm;
mod pl;
mod prepo;
mod ro;
mod settings;
mod sm;
mod socket;
mod ssl;
mod swkbd;
mod time;
mod vi;
mod web;

use super::{Entry, Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "base",
    from: SdkVersion(0, 0, 0),
    tables: TABLES,
    nvn_api: None,
    thread_tail_shift: None,
};

static TABLES: &[&[Entry]] = &[
    account::TABLE,
    aoc::TABLE,
    apm::TABLE,
    audio::TABLE,
    bcat::TABLE,
    codec::TABLE,
    cxx::TABLE,
    detail::TABLE,
    diag::TABLE,
    ec::TABLE,
    err::TABLE,
    friends::TABLE,
    fs::TABLE,
    hid::TABLE,
    libc::TABLE,
    ldn::TABLE,
    mii::TABLE,
    nfp::TABLE,
    ngc::TABLE,
    nifm::TABLE,
    nv::TABLE,
    nvn::TABLE,
    oe::TABLE,
    os::TABLE,
    pdm::TABLE,
    pl::TABLE,
    prepo::TABLE,
    ro::TABLE,
    settings::TABLE,
    sm::TABLE,
    socket::TABLE,
    ssl::TABLE,
    swkbd::TABLE,
    time::TABLE,
    vi::TABLE,
    web::TABLE,
];
