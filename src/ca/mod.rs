mod asn1;
mod tpdu;
mod spdu;
mod apdu;
pub mod sys;


use {
    std::{
        path::{
            Path,
        },
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

    anyhow::{
        Result,
        Context,
    },

    nix::{
        ioctl_none,
        ioctl_read,
    },

    sys::*,
};


#[derive(Debug)]
pub struct CaDevice {
    file: File,

    // TODO: slots vec
}


impl AsRawFd for CaDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd { self.file.as_raw_fd() }
}


impl CaDevice {
    #[inline]
    pub fn reset(&mut self) -> Result<()> {
        // CA_RESET
        ioctl_none!(#[inline] ca_reset, b'o', 128);
        unsafe {
            ca_reset(self.as_raw_fd())
        }.context("CA: failed to reset")?;

        Ok(())
    }

    #[inline]
    pub fn get_caps(&self, caps: &mut CaCaps) -> Result<()> {
        // CA_GET_CAP
        ioctl_read!(#[inline] ca_get_cap, b'o', 129, CaCaps);
        unsafe {
            ca_get_cap(self.as_raw_fd(), caps as *mut _)
        }.context("CA: failed to get caps")?;

        Ok(())
    }

    /// Gets CA slot information
    ///
    /// If slot is available but not ready returns `false`
    /// If slot is ready returns `true`
    pub fn get_slot_info(&self, slot_info: &mut CaSlotInfo) -> Result<bool> {
        // CA_GET_SLOT_INFO
        ioctl_read!(#[inline] ca_get_slot_info, b'o', 130, CaSlotInfo);
        unsafe {
            ca_get_slot_info(self.as_raw_fd(), slot_info as *mut _)
        }.context("CA: failed to get slot info")?;

        if slot_info.slot_type != CA_CI_LINK {
            return Err(anyhow!("CA: incompatible interface"));
        }

        match slot_info.flags {
            CA_CI_MODULE_PRESENT => {
                Ok(false)
            }
            CA_CI_MODULE_READY => {
                Ok(true)
            }
            CA_CI_MODULE_NOT_FOUND => {
                Err(anyhow!("CA: module not found"))
            }
            _ => {
                Err(anyhow!("CA: invalid slot flags"))
            }
        }
    }

    pub fn open(path: &Path) -> Result<CaDevice> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(path)
            .with_context(|| format!("CA: failed to open device {}", path.display()))?;

        let mut ca = CaDevice {
            file,
        };

        ca.reset()?;

        let delay = Duration::from_millis(50);
        thread::sleep(delay);

        //

        let mut caps = CaCaps::default();

        for _ in 0 .. 5 {
            ca.get_caps(&mut caps)?;

            if caps.slot_num != 0 {
                break;
            }

            thread::sleep(delay);
        }

        if caps.slot_num == 0 {
            return Err(anyhow!("CA: device has no slots"));
        }

        // TODO: slots vec

        let mut slot_info = CaSlotInfo::default();

        for slot_id in 0 .. caps.slot_num {
            slot_info.slot_num = slot_id;
            slot_info.slot_type = 0;
            slot_info.flags = 0;

            ca.get_slot_info(&mut slot_info)?;

            tpdu::init(&ca, slot_id as u8)?;
        }

        Ok(ca)
    }
}
