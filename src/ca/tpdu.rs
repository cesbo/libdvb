//! Transport  Protocol  Data  Unit (TPDU)
//!
//! en50221 7.1
//! The Transport Layer of the Command Interface operates on top of a Link
//! Layer provided by the particular physical implementation used.
//! The transport protocol is a command-response protocol where the host
//! sends a command to the module, using a Command Transport Protocol Data
//! Unit (C_TPDU) and waits for a response from the module with a Response
//! Transport Protocol Data Unit (R_TPDU).
//! The module cannot initiate communication: it must wait for the host to
//! poll it or send it data first.


use {
    std::{
        io::{
            Write,
            IoSlice,
        },
    },

    anyhow::{
        Result,
        Context,
    },

    super::{
        asn1,
        CaDevice,
        spdu,
    },
};


pub use {
    ca_tpdu_tag::*,
};


pub const TPDU_SIZE_MAX: usize = 2048;


/// en50221 A.4.1.13: List of transport tags
mod ca_tpdu_tag {
    pub const TT_SB: u8                             = 0x80;
    pub const TT_RCV: u8                            = 0x81;
    pub const TT_CREATE_TC: u8                      = 0x82;
    pub const TT_CTC_REPLY: u8                      = 0x83;
    pub const TT_DELETE_TC: u8                      = 0x84;
    pub const TT_DTC_REPLY: u8                      = 0x85;
    pub const TT_REQUEST_TC: u8                     = 0x86;
    pub const TT_NEW_TC: u8                         = 0x87;
    pub const TT_TC_ERROR: u8                       = 0x88;
    pub const TT_DATA_LAST: u8                      = 0xA0;
    pub const TT_DATA_MORE: u8                      = 0xA1;
}


pub fn _read(ca: &mut CaDevice, data: &[u8]) -> Result<()> {
    // TODO: read packet

    if data.len() < 4 {
        return Err(anyhow!("CA TPDU: invalid packet size"));
    }

    if data[1] == 0 /* TODO: || data[1] > ca.slots.len() */ {
        return Err(anyhow!("CA TPDU: invalid slot id {}", data[1]));
    }

    let slot_id = data[1] - 1;
    let tag = data[2];

    match tag {
        /* Create Transport Connection reply */
        TT_CTC_REPLY => {
            // TODO: slot is active now
        }
        /* Delete Transport Connection reply */
        TT_DTC_REPLY => {
            //
        }

        TT_DATA_MORE => {
            // TODO: save data to buffer
        }
        TT_DATA_LAST => {
            // TODO: save data to buffer
            spdu::handle(ca, slot_id, &[])?;
        }

        TT_SB => {}
        _ => {
            return Err(anyhow!("CA TPDU: invalid tag 0x{:02X}", tag));
        }
    }

    Ok(())
}


/// Writes TPDU to the CA device
pub fn send(ca: &CaDevice, slot_id: u8, tag: u8, data: &[u8]) -> Result<()> {
    if data.len() >= TPDU_SIZE_MAX {
        return Err(anyhow!("CA TPDU: packet is to large"));
    }

    // TODO: queue and send messages only if module ready
    // TODO: timeout

    let t_c_id = slot_id + 1;

    let mut header: Vec<u8> = Vec::with_capacity(8);
    header.push(slot_id);
    header.push(t_c_id);
    header.push(tag);

    asn1::encode(data.len() as u16 + 1, &mut header);
    header.push(t_c_id);

    let bufs = &mut [
        IoSlice::new(&header),
        IoSlice::new(data),
    ];

    // TODO: write_all_vectored
    (&ca.file).write_vectored(bufs)
        .context("CA TPDU: write failed")?;

    Ok(())
}


/// Init transport layer for slot
pub fn init(ca: &CaDevice, slot_id: u8) -> Result<()> {
    send(ca, slot_id, TT_CREATE_TC, &[])
}
