mod asn1;
mod tpdu;
mod spdu;
mod apdu;


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
            io::AsRawFd,
        },
        time::Duration,
        thread,
    },

    anyhow::{
        Result,
        Context,
    },

    crate::{
        sys::{
            ioctl,
            IoctlInt,
            ca::*,
        },
    },
};


#[derive(Debug)]
pub struct CaDevice {
    file: File,

    slot_id: u8
}


impl CaDevice {
    #[inline]
    pub fn ioctl<T>(&self, request: IoctlInt, argp: T) -> Result<()> {
        ioctl(self.file.as_raw_fd(), request, argp)?;
        Ok(())
    }

    #[inline]
    pub fn reset(&mut self) -> Result<()> {
        self.ioctl(CA_RESET, 0).context("CA: failed to reset slot")?;
        Ok(())
    }

    #[inline]
    pub fn get_slot_id(&self) -> u8 { self.slot_id }

    fn get_info(&mut self) -> Result<()> {
        let mut caps = CaCaps::default();
        self.ioctl(CA_GET_CAP, caps.as_mut_ptr())
            .context("CA: failed to get ca caps")?;

        if caps.slot_num == 0 {
            return Ok(());
        }

        self.reset()?;

        let delay = Duration::from_millis(10);
        thread::sleep(delay);

        /* Only 1 slot with ID 0 */

        let mut slot_info = CaSlotInfo::default();
        slot_info.num = self.slot_id as i32;
        self.ioctl(CA_GET_SLOT_INFO, slot_info.as_mut_ptr())
            .context("CA: failed to get slot info")?;

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
            .custom_flags(::libc::O_NONBLOCK)
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
