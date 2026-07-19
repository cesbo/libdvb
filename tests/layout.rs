//! ABI layout checks for the hand-ported kernel structs.
//!
//! Expected sizes and offsets are taken from the C struct definitions in the
//! Linux UAPI headers: `linux/dvb/dmx.h`, `net.h`, `ca.h`.

use std::mem::offset_of;

use libdvb::{
    ca::sys::{
        CaCaps,
        CaDescr,
        CaDescrInfo,
        CaMsg,
        CaPid,
        CaSlotInfo,
    },
    dmx::sys::DmxPesFilterParams,
    net::sys::DvbNetIf,
};

#[test]
fn dmx() {
    // struct dmx_pes_filter_params
    assert_eq!(size_of::<DmxPesFilterParams>(), 20);
    assert_eq!(offset_of!(DmxPesFilterParams, input), 4);
    assert_eq!(offset_of!(DmxPesFilterParams, flags), 16);
}

#[test]
fn net() {
    // struct dvb_net_if
    assert_eq!(size_of::<DvbNetIf>(), 6);
    assert_eq!(offset_of!(DvbNetIf, feedtype), 4);
}

#[test]
fn auto_traits() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<libdvb::DvrDevice>();
    assert_send_sync::<libdvb::CiController>();
}

#[test]
fn ca() {
    // struct ca_slot_info
    assert_eq!(size_of::<CaSlotInfo>(), 12);
    // struct ca_descr_info
    assert_eq!(size_of::<CaDescrInfo>(), 8);
    // struct ca_caps
    assert_eq!(size_of::<CaCaps>(), 16);
    // struct ca_msg
    assert_eq!(size_of::<CaMsg>(), 268);
    assert_eq!(offset_of!(CaMsg, msg), 12);
    // struct ca_descr
    assert_eq!(size_of::<CaDescr>(), 16);
    assert_eq!(offset_of!(CaDescr, cw), 8);
    // struct ca_pid
    assert_eq!(size_of::<CaPid>(), 8);
}
