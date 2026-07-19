//! External CI adapter TS device (DigitalDevices / TBS)
//!
//! External CI adapters expose a full-duplex transport stream pipe:
//! scrambled TS written to the input side is descrambled by the CAM and
//! returned on the output side.
//!
//! [`SecDevice`] is control plane only: it probes the node, identifies
//! the vendor and opens the pipe. The TS read/write data path uses the
//! exposed file descriptors directly.

use std::{
    fs::{
        File,
        OpenOptions,
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
                IntoRawFd,
                RawFd,
            },
        },
    },
};

use crate::{
    error::{
        Error,
        Result,
    },
    fe::sys::DtvPropertyRaw,
};

/// TBS vendor id (`/sys/class/dvb/dvbN.secN/device/vendor`)
const VENDOR_TBS: u32 = 0x544d;

/// modules/tbs/mod.h: TBS proprietary property for the CI input bitrate
const MODULATOR_INPUT_BITRATE: u32 = 33;

/// A reference to the external CI adapter TS device (DigitalDevices / TBS)
#[derive(Debug)]
pub struct SecDevice {
    fd_in: File,
    fd_out: File,

    vendor_id: Option<u32>,
    device_id: Option<u32>,
}

impl AsRawFd for SecDevice {
    /// Returns the output side descriptor: the one to poll for readable TS
    fn as_raw_fd(&self) -> RawFd {
        self.fd_out.as_raw_fd()
    }
}

impl AsFd for SecDevice {
    /// Borrows the output side descriptor
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd_out.as_fd()
    }
}

impl SecDevice {
    /// Attempts to open the CI adapter TS device in non-blocking mode.
    ///
    /// Probes the `/dev/dvb/adapterN/ciN` node (DigitalDevices) first,
    /// then `/dev/dvb/adapterN/secN` (TBS). Opens the node twice: the
    /// input side for writing TS into the CAM and the output side for
    /// reading the descrambled TS.
    pub fn open(adapter: u32, device: u32) -> Result<SecDevice> {
        let ci_path = format!("/dev/dvb/adapter{}/ci{}", adapter, device);
        let sec_path = format!("/dev/dvb/adapter{}/sec{}", adapter, device);

        let mut opened = None;
        for path in [&ci_path, &sec_path] {
            if let Ok(file) = OpenOptions::new()
                .write(true)
                .custom_flags(::nix::libc::O_NONBLOCK)
                .open(path)
            {
                opened = Some((path, file));
                break;
            }
        }

        let Some((path, fd_in)) = opened else {
            return Err(Error::InvalidProperty(format!(
                "ci device is not found: {} or {}",
                ci_path, sec_path
            )));
        };

        let fd_out = OpenOptions::new()
            .read(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(path)?;

        let vendor_id = crate::sysfs::read_hex_attr(&fd_out, "vendor");
        let device_id = crate::sysfs::read_hex_attr(&fd_out, "device");

        Ok(SecDevice {
            fd_in,
            fd_out,
            vendor_id,
            device_id,
        })
    }

    /// PCI vendor ID of the CI device, if reported via sysfs.
    pub fn vendor_id(&self) -> Option<u32> {
        self.vendor_id
    }

    /// PCI device ID of the CI device, if reported via sysfs.
    pub fn device_id(&self) -> Option<u32> {
        self.device_id
    }

    /// Raw fd of the input side: write TS into the CAM
    pub fn fd_in(&self) -> RawFd {
        self.fd_in.as_raw_fd()
    }

    /// Raw fd of the output side: read descrambled TS from the CAM
    pub fn fd_out(&self) -> RawFd {
        self.fd_out.as_raw_fd()
    }

    /// Transfers ownership of both file descriptors: `(fd_in, fd_out)`
    pub fn into_raw_fd_pair(self) -> (RawFd, RawFd) {
        (self.fd_in.into_raw_fd(), self.fd_out.into_raw_fd())
    }

    /// Sets the CI input bitrate in MBit/s (TBS adapters only).
    ///
    /// Applies the TBS proprietary `MODULATOR_INPUT_BITRATE` property
    /// through the frontend ioctl on the input descriptor. Does nothing
    /// for adapters of other vendors.
    pub fn set_ci_bitrate(&self, bitrate: u32) -> Result<()> {
        if !matches!(self.vendor_id, Some(VENDOR_TBS)) {
            return Ok(());
        }

        #[repr(C)]
        struct DtvProperties {
            num: u32,
            props: *mut DtvPropertyRaw,
        }

        let mut props = [DtvPropertyRaw::new(MODULATOR_INPUT_BITRATE, bitrate)];
        let cmd = DtvProperties {
            num: props.len() as u32,
            props: props.as_mut_ptr(),
        };

        // FE_SET_PROPERTY
        nix::ioctl_write_ptr!(
            #[inline]
            ioctl_call,
            b'o',
            82,
            DtvProperties
        );
        unsafe { ioctl_call(self.fd_in.as_raw_fd(), &cmd as *const _) }?;

        Ok(())
    }
}
