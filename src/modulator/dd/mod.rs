//! Interface for DigitalDevices modulator devices (`/dev/dvb/adapterN/modM`):
//! Octopus MOD / RESI DVB-C cards (classic ioctl and DTV property APIs) and
//! the SDR-based cards (DVB-T and DVB-C via the MCI command interface).
//!
//! A device is configured through a read-only handle ([`ModDevice::open`]),
//! then reopened write-only for the data path ([`ModDevice::open_wr`]) -
//! opening write-only starts the RF output and closing it stops it. Writes
//! must be whole 188-byte TS packets; a blocking write waits until the
//! device DMA ring has room, so the write pace follows the channel rate.
//!
//! The classic DVB-C cards insert null packets themselves when the input
//! runs below the channel rate. The SDR DVB-T path does not: the stream
//! written must be exact CBR at the net rate of the modulation parameters
//! (see [`super::dvbt_ts_bitrate`]), with PCRs restamped accordingly.

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

use super::{
    DvbtBandwidth,
    DvbtCodeRate,
    DvbtConstellation,
    DvbtGuard,
};
use crate::{
    error::{
        Error,
        Result,
    },
    fe::sys::{
        DtvProperties,
        DtvPropertyRaw,
    },
};

impl DvbtBandwidth {
    /// The matching `MOD_STANDARD_DVBT_*` for MCI stream/channel setup.
    pub fn standard(self) -> u8 {
        match self {
            DvbtBandwidth::Mhz6 => MOD_STANDARD_DVBT_6,
            DvbtBandwidth::Mhz7 => MOD_STANDARD_DVBT_7,
            DvbtBandwidth::Mhz8 => MOD_STANDARD_DVBT_8,
        }
    }

    /// The matching `MODULATOR_OUTPUT_RATE` preset.
    pub fn output_rate(self) -> u32 {
        match self {
            DvbtBandwidth::Mhz6 => SYS_DVBT_6,
            DvbtBandwidth::Mhz7 => SYS_DVBT_7,
            DvbtBandwidth::Mhz8 => SYS_DVBT_8,
        }
    }
}

impl DvbtConstellation {
    /// The matching `MOD_DVBT_*` constellation for MCI stream setup.
    pub fn mci(self) -> u8 {
        match self {
            DvbtConstellation::Qpsk => MOD_DVBT_QPSK,
            DvbtConstellation::Qam16 => MOD_DVBT_16QAM,
            DvbtConstellation::Qam64 => MOD_DVBT_64QAM,
        }
    }
}

impl DvbtCodeRate {
    /// The matching `MOD_DVBT_PR_*` puncture rate for MCI stream setup.
    pub fn mci(self) -> u8 {
        match self {
            DvbtCodeRate::Cr1_2 => MOD_DVBT_PR_1_2,
            DvbtCodeRate::Cr2_3 => MOD_DVBT_PR_2_3,
            DvbtCodeRate::Cr3_4 => MOD_DVBT_PR_3_4,
            DvbtCodeRate::Cr5_6 => MOD_DVBT_PR_5_6,
            DvbtCodeRate::Cr7_8 => MOD_DVBT_PR_7_8,
        }
    }
}

impl DvbtGuard {
    /// The matching `MOD_DVBT_GI_*` guard interval for MCI stream setup.
    pub fn mci(self) -> u8 {
        match self {
            DvbtGuard::G1_32 => MOD_DVBT_GI_1_32,
            DvbtGuard::G1_16 => MOD_DVBT_GI_1_16,
            DvbtGuard::G1_8 => MOD_DVBT_GI_1_8,
            DvbtGuard::G1_4 => MOD_DVBT_GI_1_4,
        }
    }
}

/// Builds a `MOD_SETUP_CHANNELS` command from 1..=4 channel-plan regions.
/// The builder owns the region flags: every region is marked valid and the
/// first/last markers frame the list.
pub fn setup_channels_cmd(regions: &[ModSetupChannels]) -> Result<MciCommand> {
    if regions.is_empty() || regions.len() > 4 {
        return Err(Error::InvalidData(format!(
            "MOD_SETUP_CHANNELS takes 1..=4 regions, got {}",
            regions.len()
        )));
    }

    let mut cmd = MciCommand::new(MOD_SETUP_CHANNELS, 0, 0);
    for (i, region) in regions.iter().enumerate() {
        let mut region = *region;
        region.flags |= MOD_SETUP_FLAG_VALID;
        if i == 0 {
            region.flags |= MOD_SETUP_FLAG_FIRST;
        }
        if i == regions.len() - 1 {
            region.flags |= MOD_SETUP_FLAG_LAST;
        }
        region.write_to(&mut cmd.params[i * ModSetupChannels::SIZE ..]);
    }

    Ok(cmd)
}

/// Builds a `MOD_SETUP_OUTPUT` command (card-level RF output setup).
pub fn setup_output_cmd(output: &ModSetupOutput) -> MciCommand {
    let mut cmd = MciCommand::new(MOD_SETUP_OUTPUT, 0, 0);
    output.write_to(&mut cmd.params);
    cmd
}

/// Builds a `MOD_SETUP_STREAM` command mapping mod device `stream` onto RF
/// channel slot `channel` with the given modulation parameters.
pub fn setup_stream_cmd(channel: u8, stream: u8, setup: &ModSetupStream) -> MciCommand {
    let mut cmd = MciCommand::new(MOD_SETUP_STREAM, channel, stream);
    setup.write_to(&mut cmd.params);
    cmd
}

/// A DigitalDevices modulator device node.
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

impl ModDevice {
    fn open_mode(adapter: u32, device: u32, write: bool) -> Result<ModDevice> {
        let path = format!("/dev/dvb/adapter{}/mod{}", adapter, device);
        let file = OpenOptions::new().read(!write).write(write).open(&path)?;

        Ok(ModDevice {
            file,
            adapter,
            device,
        })
    }

    /// Opens the device read-only: the configuration handle for the ioctl
    /// APIs. Does not start the RF output.
    pub fn open(adapter: u32, device: u32) -> Result<ModDevice> {
        Self::open_mode(adapter, device, false)
    }

    /// Opens the device write-only: the TS data path. The driver starts the
    /// RF output on this open and stops it when the handle closes.
    pub fn open_wr(adapter: u32, device: u32) -> Result<ModDevice> {
        Self::open_mode(adapter, device, true)
    }

    pub fn adapter(&self) -> u32 {
        self.adapter
    }

    pub fn device(&self) -> u32 {
        self.device
    }

    /// Classic API card-level setup (`DVB_MOD_SET`). On FSM cards this
    /// resets every stream to its defaults - legacy path only.
    pub fn set_params(&self, params: &DvbModParams) -> Result<()> {
        unsafe { dvb_mod_set(self.as_raw_fd(), params) }?;
        Ok(())
    }

    /// Classic API per-channel setup (`DVB_MOD_CHANNEL_SET`).
    pub fn set_channel_params(&self, params: &DvbModChannelParams) -> Result<()> {
        unsafe { dvb_mod_channel_set(self.as_raw_fd(), params) }?;
        Ok(())
    }

    /// Applies `(command, value)` DTV properties in order through
    /// `FE_SET_PROPERTY` on the mod node (the `MODULATOR_*` command set).
    ///
    /// Only `u32`-payload commands are expressible; properties carried in
    /// `u.data64` (`MODULATOR_INPUT_BITRATE`, a Q32.32 bits/s value) need a
    /// dedicated call.
    pub fn set_properties(&self, props: &[(u32, u32)]) -> Result<()> {
        let raw: Vec<DtvPropertyRaw> = props
            .iter()
            .map(|&(cmd, data)| DtvPropertyRaw::new(cmd, data))
            .collect();

        let cmd = DtvProperties {
            num: raw.len() as u32,
            props: raw.as_ptr() as *mut _,
        };

        // FE_SET_PROPERTY
        nix::ioctl_write_ptr!(
            #[inline]
            ioctl_call,
            b'o',
            82,
            DtvProperties
        );
        unsafe { ioctl_call(self.as_raw_fd(), &cmd as *const _) }?;

        Ok(())
    }

    /// Submits one MCI command to the card firmware. A nonzero result
    /// status is reported as an error.
    pub fn mci(&self, cmd: &MciCommand) -> Result<MciResult> {
        let mut msg = DdbMciMsg {
            link: 0,
            cmd: *cmd,
            res: MciResult::zeroed(),
        };

        unsafe { ddb_mci_cmd(self.as_raw_fd(), &mut msg) }?;

        if msg.res.status != 0 {
            return Err(Error::InvalidData(format!(
                "MCI command 0x{:02x} failed with status 0x{:02x}",
                cmd.command, msg.res.status
            )));
        }

        Ok(msg.res)
    }

    /// Writes the whole buffer to the device, retrying interrupted and
    /// partial writes. In blocking mode the call waits whenever the device
    /// DMA ring is full, so its pace follows the channel rate. The driver
    /// reports a signal-interrupted ring wait as `EAGAIN` even on a
    /// blocking handle, so that is retried too, after a short pause.
    pub fn write_all(&self, buf: &[u8]) -> Result<()> {
        let mut rest = buf;
        while !rest.is_empty() {
            match (&self.file).write(rest) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "mod device accepted no data",
                    )
                    .into());
                }
                Ok(n) => rest = &rest[n ..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_cmd_flags() {
        let regions = [
            ModSetupChannels {
                flags: 0,
                standard: MOD_STANDARD_DVBT_8,
                num_channels: 8,
                frequency: 474_000_000,
                offset: 0,
                bandwidth: 0,
            },
            ModSetupChannels {
                flags: 0,
                standard: MOD_STANDARD_DVBT_8,
                num_channels: 4,
                frequency: 600_000_000,
                offset: 0,
                bandwidth: 0,
            },
        ];

        let cmd = setup_channels_cmd(&regions).unwrap();
        assert_eq!(cmd.command, MOD_SETUP_CHANNELS);
        assert_eq!(cmd.params[0], MOD_SETUP_FLAG_VALID | MOD_SETUP_FLAG_FIRST);
        assert_eq!(
            cmd.params[ModSetupChannels::SIZE],
            MOD_SETUP_FLAG_VALID | MOD_SETUP_FLAG_LAST
        );

        assert!(setup_channels_cmd(&[]).is_err());
        assert!(setup_channels_cmd(&[regions[0]; 5]).is_err());

        let single = setup_channels_cmd(&regions[.. 1]).unwrap();
        assert_eq!(
            single.params[0],
            MOD_SETUP_FLAG_VALID | MOD_SETUP_FLAG_FIRST | MOD_SETUP_FLAG_LAST
        );
    }

    #[test]
    fn stream_cmd_header() {
        let cmd = setup_stream_cmd(
            3,
            5,
            &ModSetupStream {
                standard: MOD_STANDARD_DVBT_8,
                stream_format: MOD_FORMAT_TS,
                symbol_rate: 0,
                parameter: ModStreamParameter::Ofdm(ModOfdmParameter {
                    fft_size: MOD_DVBT_FFT_8K,
                    guard_interval: MOD_DVBT_GI_1_32,
                    puncture_rate: MOD_DVBT_PR_7_8,
                    constellation: MOD_DVBT_64QAM,
                    cell_identifier: 0,
                }),
            },
        );

        assert_eq!(cmd.command, MOD_SETUP_STREAM);
        assert_eq!(cmd.channel, 3);
        assert_eq!(cmd.stream, 5);
    }

    #[test]
    fn dvbt_parameter_mappings() {
        assert_eq!(DvbtBandwidth::Mhz8.standard(), MOD_STANDARD_DVBT_8);
        assert_eq!(DvbtBandwidth::Mhz6.output_rate(), SYS_DVBT_6);
        assert_eq!(DvbtConstellation::Qam64.mci(), MOD_DVBT_64QAM);
        assert_eq!(DvbtCodeRate::Cr7_8.mci(), MOD_DVBT_PR_7_8);
        assert_eq!(DvbtGuard::G1_32.mci(), MOD_DVBT_GI_1_32);
    }
}
