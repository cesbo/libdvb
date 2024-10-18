mod asn1;
mod tpdu;
mod spdu;
mod apdu;
pub mod sys;


use {
    std::{
        fs::{
            File,
            OpenOptions,
        },
        os::unix::{
            fs::OpenOptionsExt,
            io::{
                AsRawFd,
                RawFd,
            },
        },
        time::Duration,
        thread,
    },

    crate::error::{
        Error,
        Result,
    },

    self::sys::*,
};


const CA_DELAY: Duration = Duration::from_millis(100);


#[derive(Debug)]
pub struct CaDevice {
    adapter: u32,
    device: u32,

    file: File,
    slot: CaSlotInfo,
}


impl AsRawFd for CaDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd { self.file.as_raw_fd() }
}


impl CaDevice {
    /// Sends reset command to CA device
    #[inline]
    pub fn reset(&mut self) -> Result<()> {
        // CA_RESET
        nix::ioctl_none!(#[inline] ca_reset, b'o', 128);
        unsafe {
            ca_reset(self.as_raw_fd())
        }?;

        Ok(())
    }

    /// Gets CA capabilities
    #[inline]
    pub fn get_caps(&self, caps: &mut CaCaps) -> Result<()> {
        // CA_GET_CAP
        nix::ioctl_read!(#[inline] ca_get_cap, b'o', 129, CaCaps);
        unsafe {
            ca_get_cap(self.as_raw_fd(), caps as *mut _)
        }?;

        Ok(())
    }

    /// Gets CA slot information
    #[inline]
    pub fn get_slot_info(&mut self) -> Result<()> {
        // CA_GET_SLOT_INFO
        nix::ioctl_read!(#[inline] ca_get_slot_info, b'o', 130, CaSlotInfo);
        unsafe {
            ca_get_slot_info(self.as_raw_fd(), &mut self.slot as *mut _)
        }?;

        Ok(())
    }

    /// Attempts to open a CA device
    pub fn open(adapter: u32, device: u32, slot: u32) -> Result<CaDevice> {
        let path = format!("/dev/dvb/adapter{}/ca{}", adapter, device);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(&path)?;

        let mut ca = CaDevice {
            adapter,
            device,

            file,
            slot: CaSlotInfo::default(),
        };

        ca.reset()?;

        thread::sleep(CA_DELAY);

        let mut caps = CaCaps::default();

        for _ in 0 .. 5 {
            ca.get_caps(&mut caps)?;

            if caps.slot_num != 0 {
                break;
            }

            thread::sleep(CA_DELAY);
        }

        if slot >= caps.slot_num {
            return Err(Error::InvalidProperty("ca slot not found".to_owned()));
        }

        ca.slot.slot_num = slot;
        ca.get_slot_info()?;

        if ca.slot.slot_type != CA_CI_LINK {
            return Err(Error::InvalidProperty("incompatible ca interface".to_owned()));
        }

        // reset flags
        ca.slot.flags = CA_CI_MODULE_NOT_FOUND;

        Ok(ca)
    }

    fn poll_timer(&mut self) -> Result<()> {
        let flags = self.slot.flags;

        self.get_slot_info()?;

        match self.slot.flags {
            CA_CI_MODULE_PRESENT => {
                if flags == CA_CI_MODULE_READY {
                    // TODO: de-init
                }
                return Ok(())
            }
            CA_CI_MODULE_READY => {
                if flags != CA_CI_MODULE_READY {
                    tpdu::init(self, self.slot.slot_num as u8)?;
                }
            }
            CA_CI_MODULE_NOT_FOUND => {
                return Err(Error::InvalidData("ca module not found".to_owned()));
            }
            _ => {
                return Err(Error::InvalidData("ca module invalid slot flags".to_owned()));
            }
        };

        // TODO: check queue?

        Ok(())
    }

    fn poll_event(&mut self) -> Result<()> {
        // TODO: tpdu read

        Ok(())
    }
}
