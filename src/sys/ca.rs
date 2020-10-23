#![allow(non_camel_case_types)]


use {
    std::{
        mem,
    },

    super::{
        IoctlInt,
        io_none,
        io_read,
        io_write,
    },
};


pub use {
    ca_slot_type::*,
    ca_slot_flags::*,
    ca_descr_type::*,
};


mod ca_slot_type {
    /// CI high level interface
    pub const CA_CI: i32        = 1;
    /// CI link layer level interface
    pub const CA_CI_LINK: i32   = 2;
    /// CI physical layer level interface
    pub const CA_CI_PHYS: i32   = 4;
    /// built-in descrambler
    pub const CA_DESCR: i32     = 8;
    /// simple smart card interface
    pub const CA_SC: i32        = 128;
}


mod ca_slot_flags {
    pub const CA_CI_MODULE_NOT_FOUND: u32 = 0;
    /// module (or card) inserted
    pub const CA_CI_MODULE_PRESENT: u32 = 1;
    /// module is ready for usage
    pub const CA_CI_MODULE_READY: u32   = 2;
}


/// CA slot interface types and info
#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct CaSlotInfo {
    /// slot number
    pub num: i32,
    /// slot type - ca_slot_type
    pub typ: i32,
    /// flags applicable to the slot - ca_slot_flags
    pub flags: u32,
}


impl CaSlotInfo {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut CaSlotInfo { self as *mut _ }
}


mod ca_descr_type {
    /// European Common Descrambler (ECD) hardware
    pub const CA_ECD: u32               = 1;
    /// Videoguard (NDS) hardware
    pub const CA_NDS: u32               = 2;
    /// Distributed Sample Scrambling (DSS) hardware
    pub const CA_DSS: u32               = 4;
}


/// descrambler types and info
#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct CaDescrInfo {
    /// number of available descramblers (keys)
    pub num: u32,
    /// type of supported scrambling system - ca_descr_type
    pub typ: u32,
}


/// CA slot interface capabilities
#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct CaCaps {
    /// total number of CA card and module slots
    pub slot_num: u32,
    /// bitmap with all supported types as defined at ca_slot_info
    pub slot_type: u32,
    /// total number of descrambler slots (keys)
    pub descr_num: u32,
    /// bitmap with all supported types as defined at ca_descr_info
    pub descr_type: u32,
}


impl CaCaps {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut CaCaps { self as *mut _ }
}


/// a message to/from a CI-CAM
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CaMsg {
    /// unused
    index: u32,
    /// unused
    typ: u32,
    /// length of the message
    pub length: u32,
    /// message
    pub msg: [u8; 256],
}


impl Default for CaMsg {
    #[inline]
    fn default() -> Self { unsafe { mem::zeroed::<Self>() } }
}


/// CA descrambler control words info
#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct CaDescr {
    /// CA Descrambler slot
    pub index: u32,
    /// control words parity, where 0 means even and 1 means odd
    pub parity: u32,
    /// CA Descrambler control words
    pub cw: [u8; 8],
}


#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct CaPid {
    pub pid: u32,
    /// -1 == disable
    pub index: i32,
}


pub const CA_RESET: IoctlInt = io_none(b'o', 128);
pub const CA_GET_CAP: IoctlInt = io_read::<CaCaps>(b'o', 129);
pub const CA_GET_SLOT_INFO: IoctlInt = io_read::<CaSlotInfo>(b'o', 130);
pub const CA_GET_DESCR_INFO: IoctlInt = io_read::<CaDescrInfo>(b'o', 131);
pub const CA_GET_MSG: IoctlInt = io_read::<CaMsg>(b'o', 132);
pub const CA_SEND_MSG: IoctlInt = io_write::<CaMsg>(b'o', 133);
pub const CA_SET_DESCR: IoctlInt = io_write::<CaDescr>(b'o', 134);
pub const CA_SET_PID: IoctlInt = io_write::<CaPid>(b'o', 135);
