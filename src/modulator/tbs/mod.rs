//! Interface for TBS modulator devices (`/dev/tbsmodN/modM`, the tbsmod
//! driver): DVB-C (6004, 6008, 6001, 6032), DVB-T (6104), J.83B (6014),
//! ATSC (6034), ISDB-T (6214) and ASI (690b) PCIe cards.
//!
//! One write-only handle carries both the ioctls and the TS data
//! ([`ModDevice::open`]); the driver has no read-only configuration path.
//! Opening enables the RF channel, closing stops the DMA and disables it.
//!
//! Modulation, symbol rate, frequency, bandwidth and gain are card-level:
//! the driver applies them only through channel 0 (channel 4 as well on the
//! 6008, which carries a second DAC) and silently ignores them on every
//! other channel. `MODULATOR_INPUT_BITRATE` is the only per-channel
//! property; it sets the DMA drain pace of the channel FIFO and takes
//! effect when the first write starts the DMA. It is a ceiling, not a rate
//! the input must match: on a 6004 an input below the channel rate is padded
//! with null packets by the card, and one above it is held back to the
//! channel rate by the FIFO, in both cases without losing a packet.
//!
//! The 6032 and 6001 are configured by the vendor SPI tool and must not
//! receive card-level properties: `config_srate` and `config_gain` write
//! AD9789 registers without checking the card, and those cards carry a
//! MAX5862 instead.
//!
//! A blocking write waits up to 1 s for FIFO room and then returns 0
//! instead of an error, which `Write::write_all` reports as `WriteZero`
//! (reopening the device restarts the DMA). Writes must be whole 188-byte
//! TS packets.

pub mod sys;

use std::{
    fs::{
        File,
        OpenOptions,
    },
    io,
    io::Write,
    os::fd::{
        AsFd,
        AsRawFd,
        BorrowedFd,
        RawFd,
    },
};

use sys::*;

use crate::{
    error::{
        Error,
        Result,
    },
    fe::sys::FeInfo,
};

/// A TBS modulator device node.
#[derive(Debug)]
pub struct ModDevice {
    file: File,
    adapter: u32,
    device: u32,
}

impl AsRawFd for ModDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for ModDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

/// The TS data path. `write_all` retries interrupted and partial writes and
/// fails with `WriteZero` on the driver's 1 s full-FIFO timeout.
impl Write for ModDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl ModDevice {
    /// Opens `/dev/tbsmod{adapter}/mod{device}` write-only. The driver
    /// enables the RF channel here and disables it when the handle closes.
    pub fn open(adapter: u32, device: u32) -> Result<ModDevice> {
        let path = format!("/dev/tbsmod{}/mod{}", adapter, device);
        let file = OpenOptions::new().write(true).open(&path)?;

        Ok(ModDevice {
            file,
            adapter,
            device,
        })
    }

    pub fn adapter(&self) -> u32 {
        self.adapter
    }

    pub fn device(&self) -> u32 {
        self.device
    }

    /// Card id (PCI subsystem vendor, e.g. `0x6032`) parsed from the
    /// `TBS-<id hex>:<card index>` name of `FE_GET_INFO`.
    pub fn card(&self) -> Result<u16> {
        let info = FeInfo::read(self)?;
        parse_card(&info.name_lossy())
    }

    /// Applies one `MODULATOR_*` property through `FE_SET_PROPERTY`; the
    /// driver rejects a property list longer than one.
    pub fn set_property(&self, cmd: u32, data: u32) -> Result<()> {
        super::set_properties(self.as_raw_fd(), &[(cmd, data)])
    }

    /// DVB-T setup (`DVBMOD_SET_PARAMETERS`); the driver honors it on
    /// channel 0 only.
    pub fn set_dvbt_parameters(&self, params: &DvbModulatorParameters) -> Result<()> {
        unsafe { dvbmod_set_parameters(self.as_raw_fd(), params) }?;
        Ok(())
    }
}

fn parse_card(name: &str) -> Result<u16> {
    name.strip_prefix("TBS-")
        .and_then(|s| s.split_once(':'))
        .and_then(|(id, _)| u16::from_str_radix(id, 16).ok())
        .ok_or_else(|| Error::InvalidData(format!("unexpected TBS modulator name {:?}", name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_name() {
        assert_eq!(parse_card("TBS-6032:0").unwrap(), CARD_6032);
        assert_eq!(parse_card("TBS-6004:1").unwrap(), CARD_6004);
        assert_eq!(parse_card("TBS-690B:0").unwrap(), CARD_690B);
        assert!(parse_card("Foo").is_err());
        assert!(parse_card("TBS-XYZ:0").is_err());
        assert!(parse_card("TBS-6032").is_err());
        assert!(parse_card("").is_err());
    }
}
