//! UAPI of the DigitalDevices modulator device (`/dev/dvb/adapterN/modM`),
//! as defined by the dddvb driver: `include/linux/dvb/mod.h` for the classic
//! ioctls and DTV properties, `ddbridge/ddbridge-ioctl.h` and
//! `ddbridge/ddbridge-mci.h` for the MCI command interface of the SDR-based
//! cards.

use std::mem::size_of;

/// `struct dvb_mod_params` - card-level setup of the classic API.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DvbModParams {
    /// Base frequency in Hz; streams sit at `base + N * 8 MHz`.
    pub base_frequency: u32,
    /// Output attenuator, 0..31 in 1 dB steps.
    pub attenuator: u32,
}

/// `struct dvb_mod_channel_params` - per-channel setup of the classic API.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DvbModChannelParams {
    /// `fe_modulation` value (see [`crate::fe::sys::Modulation`]).
    pub modulation: u32,
    /// Input bitrate in units of 2^-32 Hz (`bit/s << 32`); 0 leaves the
    /// hardware to measure the input rate itself.
    pub input_bitrate: u64,
    /// Nonzero enables hardware PCR restamping around the null stuffing.
    pub pcr_correction: i32,
}

// DVB_MOD_SET
nix::ioctl_write_ptr!(
    /// Card-level setup ioctl of the classic API. On FSM cards this resets
    /// every stream to its defaults, so it belongs to the legacy path only.
    #[inline]
    dvb_mod_set,
    b'o',
    208,
    DvbModParams
);

// DVB_MOD_CHANNEL_SET
nix::ioctl_write_ptr!(
    /// Per-channel setup ioctl of the classic API.
    #[inline]
    dvb_mod_channel_set,
    b'o',
    209,
    DvbModChannelParams
);

// DTV property commands accepted by FE_SET_PROPERTY on the mod device
// (mod.h). Values are in `u.data` unless noted.
pub const MODULATOR_UNDEFINED: u32 = 0;
pub const MODULATOR_START: u32 = 1;
pub const MODULATOR_STOP: u32 = 2;
/// Channel frequency in Hz.
pub const MODULATOR_FREQUENCY: u32 = 3;
/// `fe_modulation` value.
pub const MODULATOR_MODULATION: u32 = 4;
/// Symbol rate in Hz.
pub const MODULATOR_SYMBOL_RATE: u32 = 5;
/// Base frequency in Hz (SDR cards only).
pub const MODULATOR_BASE_FREQUENCY: u32 = 6;
/// 0..31 in 1 dB steps.
pub const MODULATOR_ATTENUATOR: u32 = 32;
/// Input bitrate, `u.data64` in units of 2^-32 Hz.
pub const MODULATOR_INPUT_BITRATE: u32 = 33;
/// 1 enables hardware PCR correction (classic cards; the driver applies it
/// only through `DVB_MOD_CHANNEL_SET`).
pub const MODULATOR_PCR_MODE: u32 = 34;
/// 0..255 in 0.125 dB steps.
pub const MODULATOR_GAIN: u32 = 35;
pub const MODULATOR_RESET: u32 = 36;
pub const MODULATOR_STATUS: u32 = 37;
pub const MODULATOR_INFO: u32 = 38;
/// SDR cards: TS clock-rate correction word.
pub const MODULATOR_OUTPUT_ARI: u32 = 64;
/// SDR cards: output sample-rate select, an `enum mod_output_rate` value or
/// an arbitrary rate in Hz.
pub const MODULATOR_OUTPUT_RATE: u32 = 65;

// `enum mod_output_rate` presets for MODULATOR_OUTPUT_RATE (mod.h).
pub const SYS_DVBT_6: u32 = 0;
pub const SYS_DVBT_7: u32 = 1;
pub const SYS_DVBT_8: u32 = 2;
pub const SYS_DVBC_6900: u32 = 8;
pub const SYS_ISDBT_6: u32 = 16;
pub const SYS_J83B_64_6: u32 = 24;
pub const SYS_J83B_256_6: u32 = 25;
pub const SYS_DVB_22: u32 = 32;
pub const SYS_DVB_24: u32 = 33;
pub const SYS_DVB_30: u32 = 34;
pub const SYS_ISDBS_2886: u32 = 48;

// MCI modulator commands (ddbridge-mci.h).
pub const MOD_SETUP_CHANNELS: u8 = 0x60;
pub const MOD_SETUP_OUTPUT: u8 = 0x61;
pub const MOD_SETUP_STREAM: u8 = 0x62;
pub const MOD_CLOCK_CORRECTION: u8 = 0x64;

// Flags of `mod_setup_channels.flags`.
pub const MOD_SETUP_FLAG_FIRST: u8 = 0x01;
pub const MOD_SETUP_FLAG_LAST: u8 = 0x02;
pub const MOD_SETUP_FLAG_VALID: u8 = 0x80;

// Output standards (`mod_setup_channels.standard`, `mod_setup_stream.standard`).
pub const MOD_STANDARD_GENERIC: u8 = 0x00;
pub const MOD_STANDARD_DVBT_8: u8 = 0x01;
pub const MOD_STANDARD_DVBT_7: u8 = 0x02;
pub const MOD_STANDARD_DVBT_6: u8 = 0x03;
pub const MOD_STANDARD_DVBT_5: u8 = 0x04;
pub const MOD_STANDARD_DVBC_8: u8 = 0x08;
pub const MOD_STANDARD_DVBC_7: u8 = 0x09;
pub const MOD_STANDARD_DVBC_6: u8 = 0x0A;
pub const MOD_STANDARD_J83B_QAM64: u8 = 0x0B;
pub const MOD_STANDARD_J83B_QAM256: u8 = 0x0C;
pub const MOD_STANDARD_ISDBC_QAM64: u8 = 0x0D;
pub const MOD_STANDARD_ISDBC_QAM256: u8 = 0x0E;

// `mod_setup_output.connector`.
pub const MOD_CONNECTOR_OFF: u8 = 0x00;
pub const MOD_CONNECTOR_F: u8 = 0x01;
pub const MOD_CONNECTOR_SMA: u8 = 0x02;

// `mod_setup_output.unit`.
pub const MOD_UNIT_DBUV: u8 = 0x00;
pub const MOD_UNIT_DBM: u8 = 0x01;

// `mod_setup_stream.stream_format`.
pub const MOD_FORMAT_DEFAULT: u8 = 0x00;
pub const MOD_FORMAT_IQ16: u8 = 0x01;
pub const MOD_FORMAT_IQ8: u8 = 0x02;
pub const MOD_FORMAT_IDX8: u8 = 0x03;
pub const MOD_FORMAT_TS: u8 = 0x04;

// `mod_ofdm_parameter` field encodings (DVB-T).
pub const MOD_DVBT_FFT_8K: u8 = 0x01;
pub const MOD_DVBT_GI_1_32: u8 = 0x00;
pub const MOD_DVBT_GI_1_16: u8 = 0x01;
pub const MOD_DVBT_GI_1_8: u8 = 0x02;
pub const MOD_DVBT_GI_1_4: u8 = 0x03;
pub const MOD_DVBT_PR_1_2: u8 = 0x00;
pub const MOD_DVBT_PR_2_3: u8 = 0x01;
pub const MOD_DVBT_PR_3_4: u8 = 0x02;
pub const MOD_DVBT_PR_5_6: u8 = 0x03;
pub const MOD_DVBT_PR_7_8: u8 = 0x04;
pub const MOD_DVBT_QPSK: u8 = 0x00;
pub const MOD_DVBT_16QAM: u8 = 0x01;
pub const MOD_DVBT_64QAM: u8 = 0x02;

// `mod_qam_parameter.modulation` (DVB-C over MCI).
pub const MOD_QAM_DVBC_16: u8 = 0x00;
pub const MOD_QAM_DVBC_32: u8 = 0x01;
pub const MOD_QAM_DVBC_64: u8 = 0x02;
pub const MOD_QAM_DVBC_128: u8 = 0x03;
pub const MOD_QAM_DVBC_256: u8 = 0x04;

/// `struct mci_command` - a 4-byte header plus a 124-byte parameter block
/// (the C declaration is a union of typed parameter structs over `u32
/// params[31]`; the typed views are serialized into `params` by the
/// builders in [`super`]).
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MciCommand {
    pub command: u8,
    /// RF channel slot for modulator commands.
    pub channel: u8,
    /// Mod device number for modulator commands; 0xff is replaced by the
    /// device the ioctl arrives on.
    pub stream: u8,
    pub rsvd: u8,
    pub params: [u8; 124],
}

impl MciCommand {
    pub fn new(command: u8, channel: u8, stream: u8) -> Self {
        MciCommand {
            command,
            channel,
            stream,
            rsvd: 0,
            params: [0; 124],
        }
    }
}

/// `struct mci_result` - a 4-byte status header, a 108-byte result block
/// and a 16-byte firmware-version tail (MCI_RESULT_SIZE = 0x80).
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MciResult {
    /// 0 = OK; 0x80 unsupported, 0xFC invalid parameter, other nonzero
    /// values are firmware errors.
    pub status: u8,
    pub mode: u8,
    pub time: u16,
    pub result: [u8; 108],
    pub version: [u32; 3],
    pub version_rsvd: u8,
    pub version_major: u8,
    pub version_minor: u8,
    pub version_sub: u8,
}

impl MciResult {
    pub fn zeroed() -> Self {
        MciResult {
            status: 0,
            mode: 0,
            time: 0,
            result: [0; 108],
            version: [0; 3],
            version_rsvd: 0,
            version_major: 0,
            version_minor: 0,
            version_sub: 0,
        }
    }
}

/// `struct ddb_mci_msg` - the IOCTL_DDB_MCI_CMD argument.
#[repr(C)]
pub struct DdbMciMsg {
    /// Card link; always 0 for modulators.
    pub link: u32,
    pub cmd: MciCommand,
    pub res: MciResult,
}

// IOCTL_DDB_MCI_CMD
nix::ioctl_readwrite!(
    /// Submits one MCI command to the card firmware and reads the result
    /// back into the same message.
    #[inline]
    ddb_mci_cmd,
    b'd',
    0x0c,
    DdbMciMsg
);

/// `struct mod_setup_channels` - one RF region of the card channel plan:
/// `num_channels` slots starting at `frequency`, spaced by the bandwidth of
/// `standard`. Up to four regions fit one MOD_SETUP_CHANNELS command.
#[derive(Debug, Clone, Copy)]
pub struct ModSetupChannels {
    pub flags: u8,
    pub standard: u8,
    pub num_channels: u8,
    /// First slot frequency in Hz.
    pub frequency: u32,
    /// Offset in Hz; used only with `MOD_STANDARD_GENERIC`.
    pub offset: u32,
    /// Bandwidth in Hz; used only with `MOD_STANDARD_GENERIC`.
    pub bandwidth: u32,
}

impl ModSetupChannels {
    pub const SIZE: usize = 16;

    /// Serializes into the C layout: `u8 flags, standard, num_channels,
    /// rsvd; u32 frequency, offset, bandwidth`.
    pub fn write_to(&self, buf: &mut [u8]) {
        buf[0] = self.flags;
        buf[1] = self.standard;
        buf[2] = self.num_channels;
        buf[3] = 0;
        buf[4 .. 8].copy_from_slice(&self.frequency.to_ne_bytes());
        buf[8 .. 12].copy_from_slice(&self.offset.to_ne_bytes());
        buf[12 .. 16].copy_from_slice(&self.bandwidth.to_ne_bytes());
    }
}

/// `struct mod_setup_output` - card-level RF output setup.
#[derive(Debug, Clone, Copy)]
pub struct ModSetupOutput {
    pub connector: u8,
    /// Maximum active channels; determines the power budget per channel.
    pub num_channels: u8,
    pub unit: u8,
    /// Power per channel in 0.01 dB units of `unit`.
    pub channel_power: i16,
}

impl ModSetupOutput {
    pub const SIZE: usize = 6;

    /// Serializes into the C layout: `u8 connector, num_channels, unit,
    /// rsvd; s16 channel_power`.
    pub fn write_to(&self, buf: &mut [u8]) {
        buf[0] = self.connector;
        buf[1] = self.num_channels;
        buf[2] = self.unit;
        buf[3] = 0;
        buf[4 .. 6].copy_from_slice(&self.channel_power.to_ne_bytes());
    }
}

/// `struct mod_ofdm_parameter` - the DVB-T arm of `mod_setup_stream`.
#[derive(Debug, Clone, Copy)]
pub struct ModOfdmParameter {
    /// `MOD_DVBT_FFT_8K`; 2K is not supported by the firmware.
    pub fft_size: u8,
    pub guard_interval: u8,
    pub puncture_rate: u8,
    pub constellation: u8,
    pub cell_identifier: u16,
}

/// `struct mod_qam_parameter` - the QAM arm of `mod_setup_stream`.
#[derive(Debug, Clone, Copy)]
pub struct ModQamParameter {
    pub modulation: u8,
    /// Roll-off (12, 13, 15, 18); used only with `MOD_STANDARD_GENERIC`.
    pub rolloff: u8,
}

/// `struct mod_setup_stream` - per-stream modulation parameters. The C
/// declaration ends in a union; exactly one arm is serialized.
#[derive(Debug, Clone, Copy)]
pub struct ModSetupStream {
    pub standard: u8,
    pub stream_format: u8,
    /// Symbol rate in Hz; used only when `standard` does not fix one.
    pub symbol_rate: u32,
    pub parameter: ModStreamParameter,
}

#[derive(Debug, Clone, Copy)]
pub enum ModStreamParameter {
    Ofdm(ModOfdmParameter),
    Qam(ModQamParameter),
}

impl ModSetupStream {
    pub const SIZE: usize = 24;

    /// Serializes into the C layout: `u8 standard, stream_format,
    /// rsvd1[2]; u32 symbol_rate;` then the union arm (`ofdm`: `u8
    /// fft_size, guard_interval, puncture_rate, constellation, rsvd2[2];
    /// u16 cell_identifier`; `qam`: `u8 modulation, rolloff`).
    pub fn write_to(&self, buf: &mut [u8]) {
        buf[0] = self.standard;
        buf[1] = self.stream_format;
        buf[2] = 0;
        buf[3] = 0;
        buf[4 .. 8].copy_from_slice(&self.symbol_rate.to_ne_bytes());
        match self.parameter {
            ModStreamParameter::Ofdm(ofdm) => {
                buf[8] = ofdm.fft_size;
                buf[9] = ofdm.guard_interval;
                buf[10] = ofdm.puncture_rate;
                buf[11] = ofdm.constellation;
                buf[12] = 0;
                buf[13] = 0;
                buf[14 .. 16].copy_from_slice(&ofdm.cell_identifier.to_ne_bytes());
            }
            ModStreamParameter::Qam(qam) => {
                buf[8] = qam.modulation;
                buf[9] = qam.rolloff;
            }
        }
    }
}

// MCI_COMMAND_SIZE and MCI_RESULT_SIZE are both 0x80 in ddbridge-mci.h
const _: () = assert!(size_of::<MciCommand>() == 128);
const _: () = assert!(size_of::<MciResult>() == 128);
const _: () = assert!(size_of::<DdbMciMsg>() == 260);

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;

    #[test]
    fn classic_struct_layout() {
        assert_eq!(size_of::<DvbModParams>(), 8);
        assert_eq!(offset_of!(DvbModParams, attenuator), 4);

        assert_eq!(size_of::<DvbModChannelParams>(), 24);
        assert_eq!(offset_of!(DvbModChannelParams, modulation), 0);
        assert_eq!(offset_of!(DvbModChannelParams, input_bitrate), 8);
        assert_eq!(offset_of!(DvbModChannelParams, pcr_correction), 16);
    }

    #[test]
    fn mci_struct_layout() {
        assert_eq!(offset_of!(MciCommand, params), 4);
        assert_eq!(offset_of!(MciResult, mode), 1);
        assert_eq!(offset_of!(MciResult, time), 2);
        assert_eq!(offset_of!(MciResult, result), 4);
        assert_eq!(offset_of!(MciResult, version), 112);
        assert_eq!(offset_of!(DdbMciMsg, cmd), 4);
        assert_eq!(offset_of!(DdbMciMsg, res), 132);
    }

    #[test]
    fn ioctl_request_codes() {
        // _IOW('o', 208, struct dvb_mod_params { 2 x u32 })
        assert_eq!(nix::request_code_write!(b'o', 208, 8), 0x4008_6FD0);
        // _IOW('o', 209, struct dvb_mod_channel_params)
        assert_eq!(nix::request_code_write!(b'o', 209, 24), 0x4018_6FD1);
        // _IOWR('d', 0x0c, struct ddb_mci_msg)
        assert_eq!(
            nix::request_code_readwrite!(b'd', 0x0c, 260),
            0xC104_640C_u32 as nix::sys::ioctl::ioctl_num_type
        );
    }

    #[test]
    fn setup_channels_bytes() {
        let mut buf = [0xAAu8; ModSetupChannels::SIZE];
        ModSetupChannels {
            flags: MOD_SETUP_FLAG_FIRST | MOD_SETUP_FLAG_LAST | MOD_SETUP_FLAG_VALID,
            standard: MOD_STANDARD_DVBT_8,
            num_channels: 16,
            frequency: 474_000_000,
            offset: 0,
            bandwidth: 0,
        }
        .write_to(&mut buf);

        assert_eq!(&buf[.. 4], &[0x83, 0x01, 16, 0]);
        assert_eq!(&buf[4 .. 8], &474_000_000u32.to_ne_bytes());
        assert_eq!(&buf[8 ..], &[0; 8]);
    }

    #[test]
    fn setup_output_bytes() {
        let mut buf = [0xAAu8; ModSetupOutput::SIZE];
        ModSetupOutput {
            connector: MOD_CONNECTOR_F,
            num_channels: 16,
            unit: MOD_UNIT_DBUV,
            channel_power: 9000,
        }
        .write_to(&mut buf);

        assert_eq!(&buf[.. 4], &[0x01, 16, 0x00, 0]);
        assert_eq!(&buf[4 .. 6], &9000i16.to_ne_bytes());
    }

    #[test]
    fn setup_stream_ofdm_bytes() {
        let mut buf = [0xAAu8; ModSetupStream::SIZE];
        ModSetupStream {
            standard: MOD_STANDARD_DVBT_8,
            stream_format: MOD_FORMAT_TS,
            symbol_rate: 0,
            parameter: ModStreamParameter::Ofdm(ModOfdmParameter {
                fft_size: MOD_DVBT_FFT_8K,
                guard_interval: MOD_DVBT_GI_1_32,
                puncture_rate: MOD_DVBT_PR_7_8,
                constellation: MOD_DVBT_64QAM,
                cell_identifier: 0x1234,
            }),
        }
        .write_to(&mut buf);

        assert_eq!(&buf[.. 8], &[0x01, 0x04, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&buf[8 .. 14], &[0x01, 0x00, 0x04, 0x02, 0, 0]);
        assert_eq!(&buf[14 .. 16], &0x1234u16.to_ne_bytes());
    }

    #[test]
    fn setup_stream_qam_bytes() {
        let mut buf = [0u8; ModSetupStream::SIZE];
        ModSetupStream {
            standard: MOD_STANDARD_DVBC_8,
            stream_format: MOD_FORMAT_TS,
            symbol_rate: 6_900_000,
            parameter: ModStreamParameter::Qam(ModQamParameter {
                modulation: MOD_QAM_DVBC_256,
                rolloff: 0,
            }),
        }
        .write_to(&mut buf);

        assert_eq!(&buf[.. 4], &[0x08, 0x04, 0, 0]);
        assert_eq!(&buf[4 .. 8], &6_900_000u32.to_ne_bytes());
        assert_eq!(buf[8], MOD_QAM_DVBC_256);
    }
}
