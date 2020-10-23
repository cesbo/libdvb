//! Session Protocol Data Unit
//!
//! en50221 7.2.4
//! The session layer uses a Session Protocol Data Unit (SPDU) structure
//! to exchange data at session level either from the host to the module
//! or from the module to the host.


use {
    std::convert::TryInto,

    anyhow::{
        Result,
        Context,
    },

    super::{
        CaDevice,
        tpdu,
        apdu,
    },
};


pub use {
    ca_spdu_tag::*,
    ca_spdu_status::*,
};


pub const SPDU_HEADER_SIZE: usize = 4;


/// en50221 7.2.7: Coding of the session tags
mod ca_spdu_tag {
    pub const ST_SESSION_NUMBER: u8                 = 0x90;
    pub const ST_OPEN_SESSION_REQUEST: u8           = 0x91;
    pub const ST_OPEN_SESSION_RESPONSE: u8          = 0x92;
    pub const ST_CREATE_SESSION: u8                 = 0x93;
    pub const ST_CREATE_SESSION_RESPONSE: u8        = 0x94;
    pub const ST_CLOSE_SESSION_REQUEST: u8          = 0x95;
    pub const ST_CLOSE_SESSION_RESPONSE: u8         = 0x96;
}


/// en50221 Table 7: Open Session Status values
mod ca_spdu_status {
    pub const SS_OK: u8                             = 0x00;
    pub const SS_NOT_ALLOCATED: u8                  = 0xF0;
}


fn assert_size(spdu: &[u8], size: usize) -> Result<()> {
    if spdu.len() >= size && usize::from(spdu[2]) == size - 2 {
        Ok(())
    } else {
        Err(anyhow!("CA SPDU: invalid size"))
    }
}


fn handle_session_number(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    let session_id = u16::from_be_bytes(spdu[2 ..= 3].try_into().unwrap());
    apdu::handle(ca, session_id, &spdu[SPDU_HEADER_SIZE ..])
}


fn handle_open_session_request(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    assert_size(spdu, 6)?;

    let resource_id = u32::from_be_bytes(spdu[2 ..= 5].try_into().unwrap());
    let session_id = apdu::init(ca, resource_id)?;

    let response: [u8; 9] =[
        ST_OPEN_SESSION_RESPONSE,
        7,
        SS_OK,
        spdu[2],
        spdu[3],
        spdu[4],
        spdu[5],
        (session_id >> 8) as u8,
        session_id as u8
    ];

    tpdu::send(ca, tpdu::TT_DATA_LAST, &response)
}


fn handle_close_session_request(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    assert_size(spdu, 4)?;

    let session_id = u16::from_be_bytes(spdu[2 ..= 3].try_into().unwrap());
    apdu::close(ca, session_id)?;

    let response: [u8; 5] = [
        ST_CLOSE_SESSION_RESPONSE,
        3,
        SS_OK,
        spdu[2],
        spdu[3]
    ];

    tpdu::send(ca, tpdu::TT_DATA_LAST, &response)
}


fn handle_create_session_response(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    assert_size(spdu, 9)?;

    let session_id = u16::from_be_bytes(spdu[7 ..= 8].try_into().unwrap());

    if spdu[2] == SS_OK {
        apdu::open(ca, session_id)
    } else {
        println!("CA SPDU: failed to open session");
        apdu::close(ca, session_id)
    }
}


fn handle_close_session_response(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    assert_size(spdu, 5)?;

    let session_id = u16::from_be_bytes(spdu[3 ..= 4].try_into().unwrap());
    apdu::close(ca, session_id)
}



/// Process received message depends of it tag
pub fn handle(ca: &mut CaDevice, spdu: &[u8]) -> Result<()> {
    if spdu.len() < SPDU_HEADER_SIZE {
        return Err(anyhow!("CA SPDU: message is too short"));
    }

    match spdu[0] {
        ST_SESSION_NUMBER => {
            handle_session_number(ca, spdu)
                .context("ST_SESSION_NUMBER failed")
        }
        ST_OPEN_SESSION_REQUEST => {
            handle_open_session_request(ca, spdu)
                .context("ST_OPEN_SESSION_REQUEST failed")
        }
        ST_CLOSE_SESSION_REQUEST => {
            handle_close_session_request(ca, spdu)
                .context("ST_CLOSE_SESSION_REQUEST failed")
        }
        ST_CREATE_SESSION_RESPONSE => {
            handle_create_session_response(ca, spdu)
                .context("ST_CREATE_SESSION_RESPONSE failed")
        }
        ST_CLOSE_SESSION_RESPONSE => {
            handle_close_session_response(ca, spdu)
                .context("ST_CLOSE_SESSION_RESPONSE failed")
        }
        tag => Err(anyhow!("CA SPDU: invalid tag 0x{:02X}", tag)),
    }
}
