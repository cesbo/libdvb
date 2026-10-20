use std::{
    fs::{
        File,
        OpenOptions,
    },
    io::Read,
    os::{
        fd::{
            AsFd,
            BorrowedFd,
        },
        unix::io::{
            AsRawFd,
            RawFd,
        },
    },
};

use crate::{
    error::Result,
    fd::{
        file_status_flags,
        set_file_status_flags,
    },
};

/// A reference to the logical DVR device.
///
/// The DVR device exposes a transport stream multiplexed from demux filters
/// configured with `DMX_OUT_TS_TAP`.
#[derive(Debug)]
pub struct DvrDevice {
    file: File,
}

impl AsRawFd for DvrDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for DvrDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl Read for DvrDevice {
    /// Reads transport stream packets from the DVR ring buffer.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&self.file).read(buf)
    }
}

impl DvrDevice {
    /// Attempts to open a DVR device in blocking read-only mode.
    pub fn open(adapter: u32, device: u32) -> Result<Self> {
        let path = format!("/dev/dvb/adapter{}/dvr{}", adapter, device);
        let file = OpenOptions::new().read(true).open(&path)?;

        Ok(Self { file })
    }

    /// Sets the size of the circular buffer used by the DVR device.
    ///
    /// This uses the Linux DVB `DMX_SET_BUFFER_SIZE` ioctl, which is accepted
    /// by DVR file descriptors on supported drivers. Recommended values are
    /// multiples of 4096 bytes.
    pub fn set_buffer_size(&self, buffer_size: u64) -> Result<()> {
        // DMX_SET_BUFFER_SIZE
        nix::ioctl_write_int_bad!(
            #[inline]
            ioctl_call,
            nix::request_code_none!(b'o', 45)
        );
        unsafe { ioctl_call(self.as_raw_fd(), buffer_size as _) }?;

        Ok(())
    }

    /// Sets or clears `O_NONBLOCK` on the DVR descriptor.
    ///
    /// The device opens blocking. In non-blocking mode a read on an empty ring buffer returns
    /// `WouldBlock` instead of waiting for packets, which is what readiness-driven consumers
    /// such as an async reactor need.
    pub fn set_nonblocking(&self, enabled: bool) -> Result<()> {
        let flags = file_status_flags(self.as_raw_fd())?;
        let flags = if enabled {
            flags | ::nix::libc::O_NONBLOCK
        } else {
            flags & !::nix::libc::O_NONBLOCK
        };
        set_file_status_flags(self.as_raw_fd(), flags)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonblocking_flag() {
        let dvr = DvrDevice {
            file: File::open("/dev/null").unwrap(),
        };

        dvr.set_nonblocking(true).unwrap();
        assert_ne!(
            file_status_flags(dvr.as_raw_fd()).unwrap() & ::nix::libc::O_NONBLOCK,
            0
        );

        dvr.set_nonblocking(false).unwrap();
        assert_eq!(
            file_status_flags(dvr.as_raw_fd()).unwrap() & ::nix::libc::O_NONBLOCK,
            0
        );
    }
}
