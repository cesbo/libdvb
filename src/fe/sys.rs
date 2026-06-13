use std::{
    fmt,
    mem,
};

use bitflags::bitflags;
pub use dtv_property_cmd::*;
pub use fe_type::*;
pub use fecap_scale_params::*;

use crate::error::{
    Error,
    Result,
};

bitflags! {
    /// Frontend capabilities
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FeCaps: u32 {
        /// Can auto-detect frequency spectral band inversion
        const CAN_INVERSION_AUTO = 0x1;
        /// Supports FEC 1/2
        const CAN_FEC_1_2 = 0x2;
        /// Supports FEC 2/3
        const CAN_FEC_2_3 = 0x4;
        /// Supports FEC 3/4
        const CAN_FEC_3_4 = 0x8;
        /// Supports FEC 4/5
        const CAN_FEC_4_5 = 0x10;
        /// Supports FEC 5/6
        const CAN_FEC_5_6 = 0x20;
        /// Supports FEC 6/7
        const CAN_FEC_6_7 = 0x40;
        /// Supports FEC 7/8
        const CAN_FEC_7_8 = 0x80;
        /// Supports FEC 8/9
        const CAN_FEC_8_9 = 0x100;
        /// Can auto-detect FEC
        const CAN_FEC_AUTO = 0x200;
        /// Supports QPSK modulation
        const CAN_QPSK = 0x400;
        /// Supports 16-QAM modulation
        const CAN_QAM_16 = 0x800;
        /// Supports 32-QAM modulation
        const CAN_QAM_32 = 0x1000;
        /// Supports 64-QAM modulation
        const CAN_QAM_64 = 0x2000;
        /// Supports 128-QAM modulation
        const CAN_QAM_128 = 0x4000;
        /// Supports 256-QAM modulation
        const CAN_QAM_256 = 0x8000;
        /// Can auto-detect QAM modulation
        const CAN_QAM_AUTO = 0x10000;
        /// Can auto-detect transmission mode
        const CAN_TRANSMISSION_MODE_AUTO = 0x20000;
        /// Can auto-detect bandwidth
        const CAN_BANDWIDTH_AUTO = 0x40000;
        /// Can auto-detect guard interval
        const CAN_GUARD_INTERVAL_AUTO = 0x80000;
        /// Can auto-detect hierarchy
        const CAN_HIERARCHY_AUTO = 0x100000;
        /// Supports 8-VSB modulation
        const CAN_8VSB = 0x200000;
        /// Supports 16-VSB modulation
        const CAN_16VSB = 0x400000;
        /// Unused
        const HAS_EXTENDED_CAPS = 0x800000;
        /// Supports multistream filtering
        const CAN_MULTISTREAM = 0x4000000;
        /// Supports "turbo FEC" modulation
        const CAN_TURBO_FEC = 0x8000000;
        /// Supports "2nd generation" modulation, e. g. DVB-S2, DVB-T2, DVB-C2
        const CAN_2G_MODULATION = 0x10000000;
        /// Unused
        const NEEDS_BENDING = 0x20000000;
        /// Can recover from a cable unplug automatically
        const CAN_RECOVER = 0x40000000;
        /// Can stop spurious TS data output
        const CAN_MUTE_TS = 0x80000000;
    }
}

impl FeCaps {
    /// There's something wrong at the frontend, and it can't report its capabilities
    /// (`FE_IS_STUPID`, the empty set).
    pub const IS_STUPID: Self = Self::empty();
}

/// DEPRECATED: Should be kept just due to backward compatibility
mod fe_type {
    pub const FE_QPSK: u32 = 0;
    pub const FE_QAM: u32 = 1;
    pub const FE_OFDM: u32 = 2;
    pub const FE_ATSC: u32 = 3;
}

/// Frontend properties and capabilities
/// The frequencies are specified in Hz for Terrestrial and Cable systems.
/// The frequencies are specified in kHz for Satellite systems.
#[repr(C)]
#[derive(Debug)]
pub struct FeInfo {
    /// Name of the frontend
    pub name: [std::os::raw::c_char; 128],
    /// DEPRECATED: frontend delivery system
    pub fe_type: u32,
    /// Minimal frequency supported by the frontend
    pub frequency_min: u32,
    /// Maximal frequency supported by the frontend
    pub frequency_max: u32,
    /// All frequencies are multiple of this value
    pub frequency_stepsize: u32,
    /// Frequency tolerance
    pub frequency_tolerance: u32,
    /// Minimal symbol rate, in bauds (for Cable/Satellite systems)
    pub symbol_rate_min: u32,
    /// Maximal symbol rate, in bauds (for Cable/Satellite systems)
    pub symbol_rate_max: u32,
    /// Maximal symbol rate tolerance, in ppm (for Cable/Satellite systems)
    pub symbol_rate_tolerance: u32,
    /// DEPRECATED
    pub notifier_delay: u32,
    /// Capabilities supported by the frontend
    pub caps: u32,
}

impl Default for FeInfo {
    #[inline]
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

impl FeInfo {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut FeInfo {
        self as *mut _
    }
}

/// DiSEqC master command
/// Check out the DiSEqC bus spec available on http://www.eutelsat.org/ for
/// the possible messages that can be used.
#[repr(C)]
#[derive(Debug)]
pub struct DiseqcMasterCmd {
    /// DiSEqC message to be sent. It contains a 3 bytes header with:
    /// framing + address + command, and an optional argument
    /// of up to 3 bytes of data.
    pub msg: [u8; 6],
    /// Length of the DiSEqC message. Valid values are 3 to 6.
    pub len: u8,
}

impl Default for DiseqcMasterCmd {
    #[inline]
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

/// DiSEqC received data
#[repr(C)]
#[derive(Debug)]
pub struct DiseqcSlaveReply {
    /// DiSEqC message buffer to store a message received via DiSEqC.
    /// It contains one byte header with: framing and
    /// an optional argument of up to 3 bytes of data.
    pub msg: [u8; 4],
    /// Length of the DiSEqC message. Valid values are 0 to 4,
    /// where 0 means no message.
    pub len: u8,
    /// Return from ioctl after timeout ms with errorcode when
    /// no message was received.
    pub timeout: u32,
}

impl Default for DiseqcSlaveReply {
    #[inline]
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

bitflags! {
    /// Enumerates the possible frontend status
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FeStatusFlags: u32 {
        /// Has found something above the noise level
        const HAS_SIGNAL = 0x01;
        /// Has found a signal
        const HAS_CARRIER = 0x02;
        /// FEC inner coding (Viterbi, LDPC or other inner code) is stable.
        const HAS_VITERBI = 0x04;
        /// Synchronization bytes was found
        const HAS_SYNC = 0x08;
        /// Digital TV were locked and everything is working
        const HAS_LOCK = 0x10;
        /// Fo lock within the last about 2 seconds
        const TIMEDOUT = 0x20;
        /// Frontend was reinitialized, application is recommended
        /// to reset DiSEqC, tone and parameters
        const REINIT = 0x40;
    }
}

impl FeStatusFlags {
    /// The frontend doesn't have any kind of lock. That's the initial frontend
    /// status (`FE_NONE`, the empty set).
    pub const NONE: Self = Self::empty();
}

/// DC Voltage used to feed the LNBf
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecVoltage {
    /// Output 13V to the LNB. Vertical linear. Right circular.
    V13 = 0,
    /// Output 18V to the LNB. Horizontal linear. Left circular.
    V18 = 1,
    /// Don't feed the LNB with a DC voltage
    Off = 2,
}

impl TryFrom<u32> for SecVoltage {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(SecVoltage::V13),
            1 => Ok(SecVoltage::V18),
            2 => Ok(SecVoltage::Off),
            _ => Err(Error::InvalidData(format!(
                "invalid SecVoltage value: {}",
                value
            ))),
        }
    }
}

/// SEC tone mode
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecTone {
    /// Sends a 22kHz tone burst to the antenna
    On = 0,
    /// Don't send a 22kHz tone to the antenna (except if the FE_DISEQC_* ioctl are called)
    Off = 1,
}

impl TryFrom<u32> for SecTone {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(SecTone::On),
            1 => Ok(SecTone::Off),
            _ => Err(Error::InvalidData(format!(
                "invalid SecTone value: {}",
                value
            ))),
        }
    }
}

/// Type of mini burst to be sent
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecMiniCmd {
    /// Sends a mini-DiSEqC 22kHz '0' Tone Burst to select satellite-A
    A = 0,
    /// Sends a mini-DiSEqC 22kHz '1' Data Burst to select satellite-B
    B = 1,
}

impl TryFrom<u32> for SecMiniCmd {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(SecMiniCmd::A),
            1 => Ok(SecMiniCmd::B),
            _ => Err(Error::InvalidData(format!(
                "invalid SecMiniCmd value: {}",
                value
            ))),
        }
    }
}

/// Spectral band inversion
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inversion {
    Off = 0,
    On = 1,
    Auto = 2,
}

impl TryFrom<u32> for Inversion {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Inversion::Off),
            1 => Ok(Inversion::On),
            2 => Ok(Inversion::Auto),
            _ => Err(Error::InvalidData(format!(
                "invalid Inversion value: {}",
                value
            ))),
        }
    }
}

/// Pilot tone mode
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pilot {
    On = 0,
    Off = 1,
    Auto = 2,
}

impl TryFrom<u32> for Pilot {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Pilot::On),
            1 => Ok(Pilot::Off),
            2 => Ok(Pilot::Auto),
            _ => Err(Error::InvalidData(format!(
                "invalid Pilot value: {}",
                value
            ))),
        }
    }
}

/// Rolloff factor
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rolloff {
    /// Roll-off factor 0.35 (DVB-S)
    R35 = 0,
    /// Roll-off factor 0.20
    R20 = 1,
    /// Roll-off factor 0.25
    R25 = 2,
    Auto = 3,
    /// Roll-off factor 0.15
    R15 = 4,
    /// Roll-off factor 0.10
    R10 = 5,
    /// Roll-off factor 0.05
    R5 = 6,
}

impl TryFrom<u32> for Rolloff {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Rolloff::R35),
            1 => Ok(Rolloff::R20),
            2 => Ok(Rolloff::R25),
            3 => Ok(Rolloff::Auto),
            4 => Ok(Rolloff::R15),
            5 => Ok(Rolloff::R10),
            6 => Ok(Rolloff::R5),
            _ => Err(Error::InvalidData(format!(
                "invalid Rolloff value: {}",
                value
            ))),
        }
    }
}

/// Guard interval
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardInterval {
    Gi1_32 = 0,
    Gi1_16 = 1,
    Gi1_8 = 2,
    Gi1_4 = 3,
    Auto = 4,
    Gi1_128 = 5,
    Gi19_128 = 6,
    Gi19_256 = 7,
    Pn420 = 8,
    Pn595 = 9,
    Pn945 = 10,
}

impl TryFrom<u32> for GuardInterval {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(GuardInterval::Gi1_32),
            1 => Ok(GuardInterval::Gi1_16),
            2 => Ok(GuardInterval::Gi1_8),
            3 => Ok(GuardInterval::Gi1_4),
            4 => Ok(GuardInterval::Auto),
            5 => Ok(GuardInterval::Gi1_128),
            6 => Ok(GuardInterval::Gi19_128),
            7 => Ok(GuardInterval::Gi19_256),
            8 => Ok(GuardInterval::Pn420),
            9 => Ok(GuardInterval::Pn595),
            10 => Ok(GuardInterval::Pn945),
            _ => Err(Error::InvalidData(format!(
                "invalid GuardInterval value: {}",
                value
            ))),
        }
    }
}

/// Transmission mode
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransmitMode {
    Tm2K = 0,
    Tm8K = 1,
    Auto = 2,
    Tm4K = 3,
    Tm1K = 4,
    Tm16K = 5,
    Tm32K = 6,
    C1 = 7,
    C3780 = 8,
}

impl TryFrom<u32> for TransmitMode {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(TransmitMode::Tm2K),
            1 => Ok(TransmitMode::Tm8K),
            2 => Ok(TransmitMode::Auto),
            3 => Ok(TransmitMode::Tm4K),
            4 => Ok(TransmitMode::Tm1K),
            5 => Ok(TransmitMode::Tm16K),
            6 => Ok(TransmitMode::Tm32K),
            7 => Ok(TransmitMode::C1),
            8 => Ok(TransmitMode::C3780),
            _ => Err(Error::InvalidData(format!(
                "invalid TransmitMode value: {}",
                value
            ))),
        }
    }
}

/// Hierarchy
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hierarchy {
    None = 0,
    H1 = 1,
    H2 = 2,
    H4 = 3,
    Auto = 4,
}

impl TryFrom<u32> for Hierarchy {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Hierarchy::None),
            1 => Ok(Hierarchy::H1),
            2 => Ok(Hierarchy::H2),
            3 => Ok(Hierarchy::H4),
            4 => Ok(Hierarchy::Auto),
            _ => Err(Error::InvalidData(format!(
                "invalid Hierarchy value: {}",
                value
            ))),
        }
    }
}

/// Interleaving
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interleaving {
    None = 0,
    Auto = 1,
    I240 = 2,
    I720 = 3,
}

impl TryFrom<u32> for Interleaving {
    type Error = Error;

    #[inline]
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Interleaving::None),
            1 => Ok(Interleaving::Auto),
            2 => Ok(Interleaving::I240),
            3 => Ok(Interleaving::I720),
            _ => Err(Error::InvalidData(format!(
                "invalid Interleaving value: {}",
                value
            ))),
        }
    }
}

/// DVB delivery system (`fe_delivery_system`, from `linux/dvb/frontend.h`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliverySystem {
    Undefined = 0,
    DvbcAnnexA = 1,
    DvbcAnnexB = 2,
    Dvbt = 3,
    Dss = 4,
    Dvbs = 5,
    Dvbs2 = 6,
    Dvbh = 7,
    Isdbt = 8,
    Isdbs = 9,
    Isdbc = 10,
    Atsc = 11,
    Atscmh = 12,
    Dtmb = 13,
    Cmmb = 14,
    Dab = 15,
    Dvbt2 = 16,
    Turbo = 17,
    DvbcAnnexC = 18,
    Dvbc2 = 19,
}

impl DeliverySystem {
    #[inline]
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for DeliverySystem {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        let result = match value {
            0 => DeliverySystem::Undefined,
            1 => DeliverySystem::DvbcAnnexA,
            2 => DeliverySystem::DvbcAnnexB,
            3 => DeliverySystem::Dvbt,
            4 => DeliverySystem::Dss,
            5 => DeliverySystem::Dvbs,
            6 => DeliverySystem::Dvbs2,
            7 => DeliverySystem::Dvbh,
            8 => DeliverySystem::Isdbt,
            9 => DeliverySystem::Isdbs,
            10 => DeliverySystem::Isdbc,
            11 => DeliverySystem::Atsc,
            12 => DeliverySystem::Atscmh,
            13 => DeliverySystem::Dtmb,
            14 => DeliverySystem::Cmmb,
            15 => DeliverySystem::Dab,
            16 => DeliverySystem::Dvbt2,
            17 => DeliverySystem::Turbo,
            18 => DeliverySystem::DvbcAnnexC,
            19 => DeliverySystem::Dvbc2,
            _ => {
                return Err(Error::InvalidData(format!(
                    "invalid DeliverySystem value: {}",
                    value
                )));
            }
        };
        Ok(result)
    }
}

impl fmt::Display for DeliverySystem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let v = match self {
            DeliverySystem::Undefined => "none",
            DeliverySystem::DvbcAnnexA => "dvb-c",
            DeliverySystem::DvbcAnnexB => "dvb-c/b",
            DeliverySystem::Dvbt => "dvb-t",
            DeliverySystem::Dss => "dss",
            DeliverySystem::Dvbs => "dvb-s",
            DeliverySystem::Dvbs2 => "dvb-s2",
            DeliverySystem::Dvbh => "dvb-h",
            DeliverySystem::Isdbt => "isdb-t",
            DeliverySystem::Isdbs => "isdb-s",
            DeliverySystem::Isdbc => "isdb-c",
            DeliverySystem::Atsc => "atsc",
            DeliverySystem::Atscmh => "atsc-m/h",
            DeliverySystem::Dtmb => "dtmb",
            DeliverySystem::Cmmb => "cmmb",
            DeliverySystem::Dab => "dab",
            DeliverySystem::Dvbt2 => "dvb-t2",
            DeliverySystem::Turbo => "dvb-s/turbo",
            DeliverySystem::DvbcAnnexC => "dvb-c/c",
            DeliverySystem::Dvbc2 => "dvb-c2",
        };

        write!(f, "{}", v)
    }
}

/// Type of modulation/constellation (`fe_modulation`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modulation {
    Qpsk = 0,
    Qam16 = 1,
    Qam32 = 2,
    Qam64 = 3,
    Qam128 = 4,
    Qam256 = 5,
    QamAuto = 6,
    Vsb8 = 7,
    Vsb16 = 8,
    Psk8 = 9,
    Apsk16 = 10,
    Apsk32 = 11,
    Dqpsk = 12,
    Qam4Nr = 13,
    Apsk64 = 14,
    Apsk128 = 15,
    Apsk256 = 16,
}

impl Modulation {
    #[inline]
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for Modulation {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        let result = match value {
            0 => Modulation::Qpsk,
            1 => Modulation::Qam16,
            2 => Modulation::Qam32,
            3 => Modulation::Qam64,
            4 => Modulation::Qam128,
            5 => Modulation::Qam256,
            6 => Modulation::QamAuto,
            7 => Modulation::Vsb8,
            8 => Modulation::Vsb16,
            9 => Modulation::Psk8,
            10 => Modulation::Apsk16,
            11 => Modulation::Apsk32,
            12 => Modulation::Dqpsk,
            13 => Modulation::Qam4Nr,
            14 => Modulation::Apsk64,
            15 => Modulation::Apsk128,
            16 => Modulation::Apsk256,
            _ => {
                return Err(Error::InvalidData(format!(
                    "invalid Modulation value: {}",
                    value
                )));
            }
        };
        Ok(result)
    }
}

/// Inner forward error correction / code rate (`fe_code_rate`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fec {
    None = 0,
    Fec1_2 = 1,
    Fec2_3 = 2,
    Fec3_4 = 3,
    Fec4_5 = 4,
    Fec5_6 = 5,
    Fec6_7 = 6,
    Fec7_8 = 7,
    Fec8_9 = 8,
    Auto = 9,
    Fec3_5 = 10,
    Fec9_10 = 11,
    Fec2_5 = 12,
    Fec1_4 = 13,
    Fec1_3 = 14,
}

impl Fec {
    #[inline]
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for Fec {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        let result = match value {
            0 => Fec::None,
            1 => Fec::Fec1_2,
            2 => Fec::Fec2_3,
            3 => Fec::Fec3_4,
            4 => Fec::Fec4_5,
            5 => Fec::Fec5_6,
            6 => Fec::Fec6_7,
            7 => Fec::Fec7_8,
            8 => Fec::Fec8_9,
            9 => Fec::Auto,
            10 => Fec::Fec3_5,
            11 => Fec::Fec9_10,
            12 => Fec::Fec2_5,
            13 => Fec::Fec1_4,
            14 => Fec::Fec1_3,
            _ => {
                return Err(Error::InvalidData(format!(
                    "invalid code rate value: {}",
                    value
                )));
            }
        };
        Ok(result)
    }
}

/// scale types for the quality parameters
mod fecap_scale_params {
    /// That QoS measure is not available. That could indicate
    /// a temporary or a permanent condition.
    pub const FE_SCALE_NOT_AVAILABLE: u8 = 0;
    /// The scale is measured in 0.001 dB steps, typically used on signal measures.
    pub const FE_SCALE_DECIBEL: u8 = 1;
    /// The scale is a relative percentual measure,
    /// ranging from 0 (0%) to 0xffff (100%).
    pub const FE_SCALE_RELATIVE: u8 = 2;
    /// The scale counts the occurrence of an event, like
    /// bit error, block error, lapsed time.
    pub const FE_SCALE_COUNTER: u8 = 3;
}

/// Used for reading a DTV status property
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct DtvStats {
    pub scale: u8, // fecap_scale_params
    pub value: i64,
}

impl fmt::Debug for DtvStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("DtvStats");

        const FIELD_SCALE: &str = "scale";
        const FIELD_VALUE: &str = "value";

        match self.scale {
            FE_SCALE_NOT_AVAILABLE => {
                s.field(FIELD_SCALE, &"FE_SCALE_NOT_AVAILABLE");
                s.field(FIELD_VALUE, &"not available");
            }
            FE_SCALE_DECIBEL => {
                s.field(FIELD_SCALE, &"FE_SCALE_DECIBEL");
                s.field(FIELD_VALUE, &{ (self.value as f64) / 1000.0 });
            }
            FE_SCALE_RELATIVE => {
                s.field(FIELD_SCALE, &"FE_SCALE_RELATIVE");
                s.field(FIELD_VALUE, &{ self.value as u64 });
            }
            FE_SCALE_COUNTER => {
                s.field(FIELD_SCALE, &"FE_SCALE_COUNTER");
                s.field(FIELD_VALUE, &{ self.value as u64 });
            }
            _ => {
                s.field(FIELD_SCALE, &{ self.scale });
                s.field(FIELD_VALUE, &"invalid scale format");
            }
        };
        s.finish()
    }
}

pub const MAX_DTV_STATS: usize = 4;

/// Store Digital TV frontend statistics
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct DtvFrontendStats {
    pub len: u8,
    pub stat: [DtvStats; MAX_DTV_STATS],
}

impl fmt::Debug for DtvFrontendStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let len = ::std::cmp::min(self.len as usize, self.stat.len());
        f.debug_list().entries(self.stat[0 .. len].iter()).finish()
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DtvPropertyBuffer {
    pub data: [u8; 32],
    pub len: u32,
    __reserved_1: [u32; 3],
    __reserved_2: usize,
}

#[repr(C)]
pub union DtvPropertyData {
    pub data: u32,
    pub st: DtvFrontendStats,
    pub buffer: DtvPropertyBuffer,
    __align: [u8; 56],
}

/// DVBv5 property Commands
mod dtv_property_cmd {
    pub const DTV_UNDEFINED: u32 = 0;
    pub const DTV_TUNE: u32 = 1;
    pub const DTV_CLEAR: u32 = 2;
    pub const DTV_FREQUENCY: u32 = 3;
    pub const DTV_MODULATION: u32 = 4;
    pub const DTV_BANDWIDTH_HZ: u32 = 5;
    pub const DTV_INVERSION: u32 = 6;
    pub const DTV_DISEQC_MASTER: u32 = 7;
    pub const DTV_SYMBOL_RATE: u32 = 8;
    pub const DTV_INNER_FEC: u32 = 9;
    pub const DTV_VOLTAGE: u32 = 10;
    pub const DTV_TONE: u32 = 11;
    pub const DTV_PILOT: u32 = 12;
    pub const DTV_ROLLOFF: u32 = 13;
    pub const DTV_DISEQC_SLAVE_REPLY: u32 = 14;

    /* Basic enumeration set for querying unlimited capabilities */

    pub const DTV_FE_CAPABILITY_COUNT: u32 = 15;
    pub const DTV_FE_CAPABILITY: u32 = 16;
    pub const DTV_DELIVERY_SYSTEM: u32 = 17;

    /* ISDB-T and ISDB-Tsb */

    pub const DTV_ISDBT_PARTIAL_RECEPTION: u32 = 18;
    pub const DTV_ISDBT_SOUND_BROADCASTING: u32 = 19;

    pub const DTV_ISDBT_SB_SUBCHANNEL_ID: u32 = 20;
    pub const DTV_ISDBT_SB_SEGMENT_IDX: u32 = 21;
    pub const DTV_ISDBT_SB_SEGMENT_COUNT: u32 = 22;

    pub const DTV_ISDBT_LAYERA_FEC: u32 = 23;
    pub const DTV_ISDBT_LAYERA_MODULATION: u32 = 24;
    pub const DTV_ISDBT_LAYERA_SEGMENT_COUNT: u32 = 25;
    pub const DTV_ISDBT_LAYERA_TIME_INTERLEAVING: u32 = 26;

    pub const DTV_ISDBT_LAYERB_FEC: u32 = 27;
    pub const DTV_ISDBT_LAYERB_MODULATION: u32 = 28;
    pub const DTV_ISDBT_LAYERB_SEGMENT_COUNT: u32 = 29;
    pub const DTV_ISDBT_LAYERB_TIME_INTERLEAVING: u32 = 30;

    pub const DTV_ISDBT_LAYERC_FEC: u32 = 31;
    pub const DTV_ISDBT_LAYERC_MODULATION: u32 = 32;
    pub const DTV_ISDBT_LAYERC_SEGMENT_COUNT: u32 = 33;
    pub const DTV_ISDBT_LAYERC_TIME_INTERLEAVING: u32 = 34;

    pub const DTV_API_VERSION: u32 = 35;

    /* DVB-T/T2 */

    pub const DTV_CODE_RATE_HP: u32 = 36;
    pub const DTV_CODE_RATE_LP: u32 = 37;
    pub const DTV_GUARD_INTERVAL: u32 = 38;
    pub const DTV_TRANSMISSION_MODE: u32 = 39;
    pub const DTV_HIERARCHY: u32 = 40;

    pub const DTV_ISDBT_LAYER_ENABLED: u32 = 41;

    pub const DTV_STREAM_ID: u32 = 42;
    pub const DTV_DVBT2_PLP_ID_LEGACY: u32 = 43;

    pub const DTV_ENUM_DELSYS: u32 = 44;

    /* ATSC-MH */

    pub const DTV_ATSCMH_FIC_VER: u32 = 45;
    pub const DTV_ATSCMH_PARADE_ID: u32 = 46;
    pub const DTV_ATSCMH_NOG: u32 = 47;
    pub const DTV_ATSCMH_TNOG: u32 = 48;
    pub const DTV_ATSCMH_SGN: u32 = 49;
    pub const DTV_ATSCMH_PRC: u32 = 50;
    pub const DTV_ATSCMH_RS_FRAME_MODE: u32 = 51;
    pub const DTV_ATSCMH_RS_FRAME_ENSEMBLE: u32 = 52;
    pub const DTV_ATSCMH_RS_CODE_MODE_PRI: u32 = 53;
    pub const DTV_ATSCMH_RS_CODE_MODE_SEC: u32 = 54;
    pub const DTV_ATSCMH_SCCC_BLOCK_MODE: u32 = 55;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_A: u32 = 56;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_B: u32 = 57;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_C: u32 = 58;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_D: u32 = 59;

    pub const DTV_INTERLEAVING: u32 = 60;
    pub const DTV_LNA: u32 = 61;

    /* Quality parameters */

    pub const DTV_STAT_SIGNAL_STRENGTH: u32 = 62;
    pub const DTV_STAT_CNR: u32 = 63;
    pub const DTV_STAT_PRE_ERROR_BIT_COUNT: u32 = 64;
    pub const DTV_STAT_PRE_TOTAL_BIT_COUNT: u32 = 65;
    pub const DTV_STAT_POST_ERROR_BIT_COUNT: u32 = 66;
    pub const DTV_STAT_POST_TOTAL_BIT_COUNT: u32 = 67;
    pub const DTV_STAT_ERROR_BLOCK_COUNT: u32 = 68;
    pub const DTV_STAT_TOTAL_BLOCK_COUNT: u32 = 69;

    /* Physical layer scrambling */

    pub const DTV_SCRAMBLING_SEQUENCE_INDEX: u32 = 70;
    pub const DTV_INPUT: u32 = 71;
}

/// Store one of frontend command and its value
#[repr(C, packed)]
pub struct DtvPropertyRaw {
    pub cmd: u32,
    __reserved_1: [u32; 3],
    pub u: DtvPropertyData,
    pub result: i32,
}

impl fmt::Debug for DtvPropertyRaw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("DtvPropertyRaw");

        const FIELD_CMD: &str = "cmd";
        const FIELD_DATA: &str = "data";
        const FIELD_STATS: &str = "stats";

        match self.cmd {
            DTV_FREQUENCY => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_FREQUENCY");
                s.field(FIELD_DATA, &data);
            }
            DTV_MODULATION => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_MODULATION");
                s.field(FIELD_DATA, &data);
            }
            DTV_BANDWIDTH_HZ => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_BANDWIDTH_HZ");
                s.field(FIELD_DATA, &data);
            }
            DTV_INVERSION => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_INVERSION");
                s.field(FIELD_DATA, &data);
            }
            DTV_SYMBOL_RATE => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_SYMBOL_RATE");
                s.field(FIELD_DATA, &data);
            }
            DTV_INNER_FEC => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_INNER_FEC");
                s.field(FIELD_DATA, &data);
            }
            DTV_PILOT => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_PILOT");
                s.field(FIELD_DATA, &data);
            }
            DTV_ROLLOFF => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_ROLLOFF");
                s.field(FIELD_DATA, &data);
            }
            DTV_DELIVERY_SYSTEM => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_DELIVERY_SYSTEM");
                s.field(FIELD_DATA, &data);
            }
            DTV_API_VERSION => {
                let data = self.get_data();
                s.field(FIELD_CMD, &"DTV_API_VERSION");
                s.field(FIELD_DATA, &data);
            }

            /* Quality parameters */
            DTV_STAT_SIGNAL_STRENGTH => {
                s.field(FIELD_CMD, &"DTV_STAT_SIGNAL_STRENGTH");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_CNR => {
                s.field(FIELD_CMD, &"DTV_STAT_CNR");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }

            DTV_STAT_PRE_ERROR_BIT_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_PRE_ERROR_BIT_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_PRE_TOTAL_BIT_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_PRE_TOTAL_BIT_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_POST_ERROR_BIT_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_POST_ERROR_BIT_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_POST_TOTAL_BIT_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_POST_TOTAL_BIT_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_ERROR_BLOCK_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_ERROR_BLOCK_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }
            DTV_STAT_TOTAL_BLOCK_COUNT => {
                s.field(FIELD_CMD, &"DTV_STAT_TOTAL_BLOCK_COUNT");
                s.field(FIELD_STATS, unsafe { &self.u.st });
            }

            // TODO: more values
            _ => {}
        }

        s.field("result", &{ self.result });
        s.finish()
    }
}

impl DtvPropertyRaw {
    #[inline]
    pub fn new(cmd: u32, data: u32) -> Self {
        Self {
            cmd,
            __reserved_1: [0, 0, 0],
            u: DtvPropertyData { data },
            result: 0,
        }
    }

    #[inline]
    pub(crate) fn get_data(&self) -> u32 {
        let u_ptr = std::ptr::addr_of!(self.u);
        let u = unsafe { u_ptr.read_unaligned() };
        unsafe { u.data }
    }
}

pub const DTV_MAX_COMMAND: u32 = DTV_INPUT;

/// num of properties cannot exceed DTV_IOCTL_MAX_MSGS per ioctl
pub const DTV_IOCTL_MAX_MSGS: usize = 64;

#[repr(C)]
#[derive(Debug)]
pub struct FeParameters {
    /// (absolute) frequency in Hz for DVB-C/DVB-T/ATSC
    /// intermediate frequency in kHz for DVB-S
    pub frequency: u32,
    pub inversion: u32,
    /// unimplemented frontend parameters data
    __reserved_1: [u8; 28],
}

pub const FE_MAX_EVENT: usize = 8;

#[repr(C)]
#[derive(Debug)]
pub struct FeEvent {
    pub status: u32,
    pub parameters: FeParameters,
}

impl Default for FeEvent {
    #[inline]
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

impl FeEvent {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut FeEvent {
        self as *mut _
    }
}
