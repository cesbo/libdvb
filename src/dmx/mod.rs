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

    pub fn set_pes_filter(&self, filter: &DmxPesFilterParams) -> Result<()> {
        // DMX_SET_PES_FILTER
        nix::ioctl_write_ptr!(#[inline] ioctl_call, b'o', 44, DmxPesFilterParams);
        unsafe {
            ioctl_call(self.as_raw_fd(), filter)
        }?;

        Ok(())
    }

    pub fn set_buffer_size(&self, buffer_size: u64) -> Result<()> {
        // DMX_SET_BUFFER_SIZE
        nix::ioctl_write_int!(#[inline] ioctl_call, b'o', 45);
        unsafe {
            ioctl_call(self.as_raw_fd(), buffer_size)
        }?;

        Ok(())
    }

    pub fn start(&self) -> Result<()> {
        // DMX_START
        nix::ioctl_none!(#[inline] ioctl_call, b'o', 41);
        unsafe {
            ioctl_call(self.as_raw_fd())
        }?;

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        // DMX_STOP
        nix::ioctl_none!(#[inline] ioctl_call, b'o', 42);
        unsafe {
            ioctl_call(self.as_raw_fd())
        }?;

        Ok(())
    }
}
