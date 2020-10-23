//! Application Protocol Data Unit
//!
//! en50221 8.3
//! All protocols in the Application Layer use a common Application
//! Protocol Data Unit (APDU) structure to send application data between
//! module and host or between modules.


use {
    anyhow::Result,

    super::CaDevice,
};


pub const APDU_TAG_SIZE: usize = 3;


/// Init session and returns session identifier
pub fn init(ca: &mut CaDevice, resource_id: u32) -> Result<u16> {
    unimplemented!()
}


/// Sends enquiry object to the CAM and allocate session object data
pub fn open(ca: &mut CaDevice, session_id: u16) -> Result<()> {
    unimplemented!()
}


/// Close session
pub fn close(ca: &mut CaDevice, session_id: u16) -> Result<()> {
    unimplemented!()
}


/// Process CAM responses
pub fn handle(ca: &mut CaDevice, session_id: u16, msg: &[u8]) -> Result<()> {
    unimplemented!()
}


/// Periodically checks resource status
pub fn manage(ca: &mut CaDevice, session_id: u16) -> Result<()> {
    unimplemented!()
}
