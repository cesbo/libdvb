use std::mem;

use bitflags::bitflags;
pub use ca_slot_flags::*;

bitflags! {
    /// CA slot interface types
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CaSlotType: u32 {
        /// CI high level interface
        const CI = 1;
        /// CI link layer level interface
        const CI_LINK = 2;
        /// CI physical layer level interface
        const CI_PHYS = 4;
        /// built-in descrambler
        const DESCR = 8;
        /// simple smart card interface
        const SC = 128;
    }
}

mod ca_slot_flags {
    pub const CA_CI_MODULE_NOT_FOUND: u32 = 0;
    /// module (or card) inserted
    pub const CA_CI_MODULE_PRESENT: u32 = 1;
    /// module is ready for usage
    pub const CA_CI_MODULE_READY: u32 = 2;
}

/// CA slot interface types and info
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct CaSlotInfo {
    /// slot number
    pub slot_num: u32,
    /// slot type - [`CaSlotType`] bits
    pub slot_type: u32,
    /// flags applicable to the slot - ca_slot_flags
    pub flags: u32,
}

impl CaSlotInfo {
    /// Slot interface types as typed flags
    pub fn slot_types(&self) -> CaSlotType {
        CaSlotType::from_bits_retain(self.slot_type)
    }
}

bitflags! {
    /// descrambler types
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CaDescrType: u32 {
        /// European Common Descrambler (ECD) hardware
        const ECD = 1;
        /// Videoguard (NDS) hardware
        const NDS = 2;
        /// Distributed Sample Scrambling (DSS) hardware
        const DSS = 4;
    }
}

/// descrambler types and info
#[repr(C)]
#[derive(Default, Debug)]
pub struct CaDescrInfo {
    /// number of available descramblers (keys)
    pub descr_num: u32,
    /// type of supported scrambling system - [`CaDescrType`] bits
    pub descr_type: u32,
}

impl CaDescrInfo {
    /// Supported descrambler types as typed flags
    pub fn descr_types(&self) -> CaDescrType {
        CaDescrType::from_bits_retain(self.descr_type)
    }
}

/// CA slot interface capabilities
#[repr(C)]
#[derive(Default, Debug)]
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
    /// Supported slot interface types as typed flags
    pub fn slot_types(&self) -> CaSlotType {
        CaSlotType::from_bits_retain(self.slot_type)
    }

    /// Supported descrambler types as typed flags
    pub fn descr_types(&self) -> CaDescrType {
        CaDescrType::from_bits_retain(self.descr_type)
    }
}

/// a message to/from a CI-CAM
#[repr(C)]
#[derive(Debug)]
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
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

/// CA descrambler control words info
#[repr(C)]
#[derive(Default, Debug)]
pub struct CaDescr {
    /// CA Descrambler slot
    pub index: u32,
    /// control words parity, where 0 means even and 1 means odd
    pub parity: u32,
    /// CA Descrambler control words
    pub cw: [u8; 8],
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct CaPid {
    pub pid: u32,
    /// -1 == disable
    pub index: i32,
}

// CA_RESET
nix::ioctl_none!(
    /// Resets the CA interface
    #[inline]
    ca_reset,
    b'o',
    128
);

// CA_GET_CAP
nix::ioctl_read!(
    /// Gets CA interface capabilities
    #[inline]
    ca_get_cap,
    b'o',
    129,
    CaCaps
);

// CA_GET_SLOT_INFO
nix::ioctl_read!(
    /// Gets CA slot information
    #[inline]
    ca_get_slot_info,
    b'o',
    130,
    CaSlotInfo
);
