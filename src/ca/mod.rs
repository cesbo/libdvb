//! CA device and en50221 host stack
//!
//! en50221: the Common Interface specification for conditional access and
//! other applications. The host side of the stack is layered bottom-up as:
//!
//! - [`CaDevice`] - the kernel CA device (/dev/dvb/adapterN/caN), raw link-layer frames via
//!   read(2)/write(2)

mod apdu;
mod asn1;
mod resource;
mod spdu;
pub mod sys;
mod tpdu;
mod transport;

use std::{
    fs::{
        File,
        OpenOptions,
    },
    io::{
        ErrorKind,
        Read,
        Write,
    },
    os::{
        fd::{
            AsFd,
            BorrowedFd,
        },
        unix::{
            fs::OpenOptionsExt,
            io::{
                AsRawFd,
                RawFd,
            },
        },
    },
};

use self::sys::*;
pub use self::{
    apdu::ApduTag,
    tpdu::TpduTag,
};
use crate::error::{
    Error,
    Result,
};

/// CA device of the DVB adapter (/dev/dvb/adapterN/caN)
#[derive(Debug)]
pub struct CaDevice {
    adapter: u32,
    device: u32,

    file: File,
}

impl AsRawFd for CaDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for CaDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl CaDevice {
    /// Attempts to open the CA device in non-blocking read-write mode.
    /// Non-blocking mode is required for CA device.
    pub fn open(adapter: u32, device: u32) -> Result<CaDevice> {
        let path = format!("/dev/dvb/adapter{}/ca{}", adapter, device);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(&path)?;

        Ok(CaDevice {
            adapter,
            device,
            file,
        })
    }

    /// Returns the adapter number the device was opened with
    pub fn adapter(&self) -> u32 {
        self.adapter
    }

    /// Returns the device number the device was opened with
    pub fn device(&self) -> u32 {
        self.device
    }

    /// Gets CA interface capabilities (CA_GET_CAP ioctl)
    pub fn caps(&self) -> Result<CaCaps> {
        let mut caps = CaCaps::default();
        unsafe { ca_get_cap(self.as_raw_fd(), &mut caps as *mut _) }?;

        Ok(caps)
    }

    /// Gets slot information for the given slot (CA_GET_SLOT_INFO ioctl)
    pub fn slot_info(&self, slot_id: u8) -> Result<CaSlotInfo> {
        let mut slot_info = CaSlotInfo {
            slot_num: u32::from(slot_id),
            ..CaSlotInfo::default()
        };
        unsafe { ca_get_slot_info(self.as_raw_fd(), &mut slot_info as *mut _) }?;

        Ok(slot_info)
    }

    /// Resets the CA interface (CA_RESET ioctl)
    pub fn reset(&self) -> Result<()> {
        unsafe { ca_reset(self.as_raw_fd()) }?;

        Ok(())
    }

    /// Writes one raw link frame to the device
    pub fn send_msg(&self, msg: &[u8]) -> Result<()> {
        let written = (&self.file).write(msg)?;
        if written != msg.len() {
            return Err(Error::InvalidData("ca link frame short write".to_owned()));
        }

        Ok(())
    }

    /// Reads one raw link frame from the device into `buf`
    pub fn recv_msg(&self, buf: &mut [u8]) -> Result<Option<usize>> {
        match (&self.file).read(buf) {
            Ok(0) => Err(Error::Io(std::io::Error::new(
                ErrorKind::UnexpectedEof,
                "ca device closed (zero-length read)",
            ))),
            Ok(len) => Ok(Some(len)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }
}
