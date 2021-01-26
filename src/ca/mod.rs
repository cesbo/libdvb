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

    slot_id: u8
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
    pub fn get_slot_id(&self) -> u8 { self.slot_id }

    fn get_info(&mut self) -> Result<()> {
        let mut caps = CaCaps::default();

        // CA_GET_CAP
        ioctl_read!(#[inline] ca_get_cap, b'o', 129, CaCaps);
        unsafe {
            ca_get_cap(self.as_raw_fd(), &mut caps as *mut _)
        }.context("CA: failed to get caps")?;

        if caps.slot_num == 0 {
            return Ok(());
        }

        self.reset()?;

        let delay = Duration::from_millis(10);
        thread::sleep(delay);

        /* Only 1 slot with ID 0 */

        let mut slot_info = CaSlotInfo::default();
        slot_info.num = self.slot_id as i32;

        // CA_GET_SLOT_INFO
        ioctl_read!(#[inline] ca_get_slot_info, b'o', 130, CaSlotInfo);
        unsafe {
            ca_get_slot_info(self.as_raw_fd(), &mut slot_info as *mut _)
        }.context("CA: failed to get slot info")?;

        if slot_info.flags == CA_CI_MODULE_NOT_FOUND {
            println!("CA: module not found");
            return Ok(());
        }

        if slot_info.typ != CA_CI_LINK {
            return Err(anyhow!("CA: incompatible interface"));
        }

        Ok(())
    }

    pub fn new(path: &Path, slot_id: u8) -> Result<CaDevice> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(path)
            .with_context(|| format!("CA: failed to open device {}", path.display()))?;

        let mut ca = CaDevice {
            file,
            slot_id,
        };

        ca.get_info()?;

        tpdu::init(&ca)?;

        Ok(ca)
    }
}
