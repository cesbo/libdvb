pub mod sys;

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

use self::sys::*;
use crate::error::{
    Error,
    Result,
};

/// A reference to the demux device and device information
#[derive(Debug)]
pub struct DmxDevice {
    file: File,
}

impl AsRawFd for DmxDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for DmxDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl Read for DmxDevice {
    /// Reads filtered data from the demux ring buffer.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&self.file).read(buf)
    }
}

impl DmxDevice {
    /// Attempts to open a demux device in blocking read-write mode.
    pub fn open(adapter: u32, device: u32) -> Result<Self> {
        let path = format!("/dev/dvb/adapter{}/demux{}", adapter, device);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        let dmx = DmxDevice { file };

        Ok(dmx)
    }

    /// Opens a demux device and immediately routes one transport-stream PID
    /// to the corresponding logical DVR device.
    /// Use the Linux DVB special PID `0x2000` to route the complete transport stream.
    ///
    /// The returned device owns the demux filter. It must remain open while
    /// the DVR stream is being read.
    pub fn open_ts_tap(adapter: u32, device: u32, pid: u16) -> Result<Self> {
        let dmx = Self::open(adapter, device)?;
        dmx.set_ts_tap(pid)?;

        Ok(dmx)
    }

    /// Sets up a PES filter based on the packet identifier (PID)
    pub fn set_pes_filter(&self, filter: &DmxPesFilterParams) -> Result<()> {
        // DMX_SET_PES_FILTER
        nix::ioctl_write_ptr!(
            #[inline]
            ioctl_call,
            b'o',
            44,
            DmxPesFilterParams
        );
        unsafe { ioctl_call(self.as_raw_fd(), filter) }?;

        Ok(())
    }

    /// Routes one transport-stream PID from the frontend to the corresponding
    /// logical DVR device and starts the filter immediately.
    /// Use the Linux DVB special PID `0x2000` to route the complete transport stream.
    pub fn set_ts_tap(&self, pid: u16) -> Result<()> {
        if pid > 0x2000 {
            return Err(Error::InvalidData(format!(
                "transport-stream PID must be in range 0..=8191 or 8192 for all PIDs, got {pid}"
            )));
        }

        let filter = DmxPesFilterParams {
            pid,
            input: DMX_IN_FRONTEND,
            output: DMX_OUT_TS_TAP,
            pes_type: DMX_PES_OTHER,
            flags: DmxFilterFlags::IMMEDIATE_START.bits(),
        };
        self.set_pes_filter(&filter)
    }

    /// Sets the size of the circular buffer used for filtered data.
    /// Recommended to use values that are multiples of 4096 bytes.
    /// The default size is 2 * 4096 bytes.
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

    /// Starts the filtering operation.
    pub fn start(&self) -> Result<()> {
        // DMX_START
        nix::ioctl_none!(
            #[inline]
            ioctl_call,
            b'o',
            41
        );
        unsafe { ioctl_call(self.as_raw_fd()) }?;

        Ok(())
    }

    /// Stops the filtering operation.
    pub fn stop(&self) -> Result<()> {
        // DMX_STOP
        nix::ioctl_none!(
            #[inline]
            ioctl_call,
            b'o',
            42
        );
        unsafe { ioctl_call(self.as_raw_fd()) }?;

        Ok(())
    }
}
