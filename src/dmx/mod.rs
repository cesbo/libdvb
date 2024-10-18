pub mod sys;


use {
    std::{
        fs::{
            File,
            OpenOptions,
        },
        os::unix::{
            fs::{
                OpenOptionsExt,
            },
            io::{
                AsRawFd,
                RawFd,
            },
        },
    },

    crate::error::{
        Result,
    },

    self::sys::*,
};


/// A reference to the demux device and device information
#[derive(Debug)]
pub struct DmxDevice {
    file: File,
}


impl AsRawFd for DmxDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd { self.file.as_raw_fd() }
}


impl DmxDevice {
    /// Attempts to open frontend device in read-write mode
    pub fn open(adapter: u32, device: u32) -> Result<Self> {
        let path = format!("/dev/dvb/adapter{}/demux{}", adapter, device);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(&path)?;

        let dmx = DmxDevice {
            file,
        };

        Ok(dmx)
    }

    /// Sets up a PES filter based on the packet identifier (PID)
    pub fn set_pes_filter(&self, filter: &DmxPesFilterParams) -> Result<()> {
        // DMX_SET_PES_FILTER
        nix::ioctl_write_ptr!(#[inline] ioctl_call, b'o', 44, DmxPesFilterParams);
        unsafe {
            ioctl_call(self.as_raw_fd(), filter)
        }?;

        Ok(())
    }

    /// Sets the size of the circular buffer used for filtered data.
    /// Recommended to use values that are multiples of 4096 bytes.
    /// The default size is 2 * 4096 bytes.
    pub fn set_buffer_size(&self, buffer_size: nix::libc::c_int) -> Result<()> {
        // DMX_SET_BUFFER_SIZE
        nix::ioctl_write_int!(#[inline] ioctl_call, b'o', 45);
        unsafe {
            ioctl_call(self.as_raw_fd(), buffer_size)
        }?;

        Ok(())
    }

    /// Starts the filtering operation.
    pub fn start(&self) -> Result<()> {
        // DMX_START
        nix::ioctl_none!(#[inline] ioctl_call, b'o', 41);
        unsafe {
            ioctl_call(self.as_raw_fd())
        }?;

        Ok(())
    }

    /// Stops the filtering operation.
    pub fn stop(&self) -> Result<()> {
        // DMX_STOP
        nix::ioctl_none!(#[inline] ioctl_call, b'o', 42);
        unsafe {
            ioctl_call(self.as_raw_fd())
        }?;

        Ok(())
    }
}
