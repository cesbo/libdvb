//! ABI layout checks for the hand-ported kernel structs.
//!
//! Expected sizes and offsets are taken from the C struct definitions in the
//! Linux UAPI headers: `linux/dvb/frontend.h`, `dmx.h`, `net.h`, `ca.h`.
//! Pointer-sized fields are computed from the target pointer width, so the
//! checks hold on both 64-bit and 32-bit targets.

use {
    std::{
        ffi::c_void,
        mem::offset_of,
    },

    libdvb::{
        ca::sys::{
            CaCaps,
            CaDescr,
            CaDescrInfo,
            CaMsg,
            CaPid,
            CaSlotInfo,
        },
        dmx::sys::DmxPesFilterParams,
        fe::sys::{
            DiseqcMasterCmd,
            DiseqcSlaveReply,
            DtvFrontendStats,
            DtvProperty,
            DtvPropertyBuffer,
            DtvPropertyData,
            DtvStats,
            FeEvent,
            FeInfo,
            FeParameters,
        },
        net::sys::DvbNetIf,
    },
};


const PTR_SIZE: usize = size_of::<*mut c_void>();

/// struct dtv_property buffer member: __u8 data[32] + __u32 len + __u32 reserved1[3] + void *reserved2
const DTV_PROPERTY_BUFFER_SIZE: usize = 32 + 4 + 12 + PTR_SIZE;

/// union with `data: u32`, `st` (37 bytes packed) and `buffer`; buffer dominates
const DTV_PROPERTY_DATA_SIZE: usize = DTV_PROPERTY_BUFFER_SIZE;

/// struct dtv_property is packed: __u32 cmd + __u32 reserved[3] + union + int result
const DTV_PROPERTY_SIZE: usize = 4 + 12 + DTV_PROPERTY_DATA_SIZE + 4;


#[test]
fn fe_info() {
    // struct dvb_frontend_info
    assert_eq!(size_of::<FeInfo>(), 168);
    assert_eq!(offset_of!(FeInfo, fe_type), 128);
    assert_eq!(offset_of!(FeInfo, frequency_min), 132);
    assert_eq!(offset_of!(FeInfo, symbol_rate_min), 148);
    assert_eq!(offset_of!(FeInfo, caps), 164);
}


#[test]
fn fe_diseqc() {
    // struct dvb_diseqc_master_cmd
    assert_eq!(size_of::<DiseqcMasterCmd>(), 7);
    assert_eq!(offset_of!(DiseqcMasterCmd, len), 6);

    // struct dvb_diseqc_slave_reply
    assert_eq!(size_of::<DiseqcSlaveReply>(), 12);
    assert_eq!(offset_of!(DiseqcSlaveReply, len), 4);
    assert_eq!(offset_of!(DiseqcSlaveReply, timeout), 8);
}


#[test]
fn fe_stats() {
    // struct dtv_stats (packed)
    assert_eq!(size_of::<DtvStats>(), 9);
    assert_eq!(align_of::<DtvStats>(), 1);
    assert_eq!(offset_of!(DtvStats, value), 1);

    // struct dtv_fe_stats (packed)
    assert_eq!(size_of::<DtvFrontendStats>(), 37);
    assert_eq!(align_of::<DtvFrontendStats>(), 1);
    assert_eq!(offset_of!(DtvFrontendStats, stat), 1);
}


#[test]
fn fe_property() {
    assert_eq!(size_of::<DtvPropertyBuffer>(), DTV_PROPERTY_BUFFER_SIZE);
    assert_eq!(size_of::<DtvPropertyData>(), DTV_PROPERTY_DATA_SIZE);

    // struct dtv_property (packed)
    assert_eq!(size_of::<DtvProperty>(), DTV_PROPERTY_SIZE);
    assert_eq!(align_of::<DtvProperty>(), 1);
    assert_eq!(offset_of!(DtvProperty, cmd), 0);
    assert_eq!(offset_of!(DtvProperty, u), 16);
    assert_eq!(offset_of!(DtvProperty, result), 16 + DTV_PROPERTY_DATA_SIZE);
}


#[test]
fn fe_event() {
    // struct dvb_frontend_parameters: frequency + inversion + 28-byte parameters union
    assert_eq!(size_of::<FeParameters>(), 36);

    // struct dvb_frontend_event
    assert_eq!(size_of::<FeEvent>(), 40);
    assert_eq!(offset_of!(FeEvent, parameters), 4);
}


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
