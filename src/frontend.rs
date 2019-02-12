/// DVB-API v5.11
/// System level frontend API

use libc;
use bitflags::bitflags;


use std::{io, mem};
use std::os::unix::io::RawFd;


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


pub const DTV_FE_CAPABILITY_COUNT: u32 = 15;
pub const DTV_FE_CAPABILITY: u32 = 16;
pub const DTV_DELIVERY_SYSTEM: u32 = 17;


pub const DTV_API_VERSION: u32 = 35;
pub const DTV_STREAM_ID: u32 = 42;


pub const DTV_SCRAMBLING_SEQUENCE_INDEX: u32 = 70;


bitflags! {
    /// Frontend capabilities
    pub struct Caps: u32 {
        /// There's something wrong at the frontend, and it can't report its capabilities
        const FE_IS_STUPID = 0;
        /// Can auto-detect frequency spectral band inversion
        const FE_CAN_INVERSION_AUTO = 0x1;
        /// Supports FEC 1/2
        const FE_CAN_FEC_1_2 = 0x2;
        /// Supports FEC 2/3
        const FE_CAN_FEC_2_3 = 0x4;
        /// Supports FEC 3/4
        const FE_CAN_FEC_3_4 = 0x8;
        /// Supports FEC 4/5
        const FE_CAN_FEC_4_5 = 0x10;
        /// Supports FEC 5/6
        const FE_CAN_FEC_5_6 = 0x20;
        /// Supports FEC 6/7
        const FE_CAN_FEC_6_7 = 0x40;
        /// Supports FEC 7/8
        const FE_CAN_FEC_7_8 = 0x80;
        /// Supports FEC 8/9
        const FE_CAN_FEC_8_9 = 0x100;
        /// Can auto-detect FEC
        const FE_CAN_FEC_AUTO = 0x200;
        /// Supports QPSK modulation
        const FE_CAN_QPSK = 0x400;
        /// Supports 16-QAM modulation
        const FE_CAN_QAM_16 = 0x800;
        /// Supports 32-QAM modulation
        const FE_CAN_QAM_32 = 0x1000;
        /// Supports 64-QAM modulation
        const FE_CAN_QAM_64 = 0x2000;
        /// Supports 128-QAM modulation
        const FE_CAN_QAM_128 = 0x4000;
        /// Supports 256-QAM modulation
        const FE_CAN_QAM_256 = 0x8000;
        /// Can auto-detect QAM modulation
        const FE_CAN_QAM_AUTO = 0x10000;
        /// Can auto-detect transmission mode
        const FE_CAN_TRANSMISSION_MODE_AUTO = 0x20000;
        /// Can auto-detect bandwidth
        const FE_CAN_BANDWIDTH_AUTO = 0x40000;
        /// Can auto-detect guard interval
        const FE_CAN_GUARD_INTERVAL_AUTO = 0x80000;
        /// Can auto-detect hierarchy
        const FE_CAN_HIERARCHY_AUTO = 0x100000;
        /// Supports 8-VSB modulation
        const FE_CAN_8VSB = 0x200000;
        /// Supports 16-VSB modulation
        const FE_CAN_16VSB = 0x400000;
        /// Unused
        const FE_HAS_EXTENDED_CAPS = 0x800000;
        /// Supports multistream filtering
        const FE_CAN_MULTISTREAM = 0x4000000;
        /// Supports "turbo FEC" modulation
        const FE_CAN_TURBO_FEC = 0x8000000;
        /// Supports "2nd generation" modulation, e. g. DVB-S2, DVB-T2, DVB-C2
        const FE_CAN_2G_MODULATION = 0x10000000;
        /// Unused
        const FE_NEEDS_BENDING = 0x20000000;
        /// Can recover from a cable unplug automatically
        const FE_CAN_RECOVER = 0x40000000;
        /// Can stop spurious TS data output
        const FE_CAN_MUTE_TS = 0x80000000;
    }
}


/// Frontend properties and capabilities
/// The frequencies are specified in Hz for Terrestrial and Cable systems.
/// The frequencies are specified in kHz for Satellite systems.
#[repr(C)]
pub struct Info {
    /// Name of the frontend
    pub name: [libc::c_char; 128],
    /// DEPRECATED. Use DTV_ENUM_DELSYS instead
    fe_type: i32,
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
    notifier_delay: u32,
    /// Capabilities supported by the frontend
    pub caps: Caps,
}


impl Default for Info {
    #[inline]
    fn default() -> Info {
        unsafe { mem::zeroed::<Info>() }
    }
}


/// Output 13V to the LNB. Vertical linear. Right circular.
pub const SEC_VOLTAGE_13: u32 = 0x00;
/// Output 18V to the LNB. Horizontal linear. Left circular.
pub const SEC_VOLTAGE_18: u32 = 0x01;
/// Don't feed the LNB with a DC voltage
pub const SEC_VOLTAGE_OFF: u32 = 0x02;


pub const SEC_TONE_ON: u32 = 0x00;
pub const SEC_TONE_OFF: u32 = 0x01;


pub const SEC_MINI_A: u32 = 0x00;
pub const SEC_MINI_B: u32 = 0x01;


bitflags! {
    /// Enumerates the possible frontend status
    pub struct Status: u32 {
        /// The frontend doesn't have any kind of lock. That's the initial frontend status
        const FE_NONE = 0x00;
        /// Has found something above the noise level
        const FE_HAS_SIGNAL = 0x01;
        /// Has found a signal
        const FE_HAS_CARRIER = 0x02;
        /// FEC inner coding (Viterbi, LDPC or other inner code) is stable.
        const FE_HAS_VITERBI = 0x04;
        /// Synchronization bytes was found
        const FE_HAS_SYNC = 0x08;
        /// Digital TV were locked and everything is working
        const FE_HAS_LOCK = 0x10;
        /// Fo lock within the last about 2 seconds
        const FE_TIMEDOUT = 0x20;
        /// Frontend was reinitialized, application is recommended
        /// to reset DiSEqC, tone and parameters
        const FE_REINIT = 0x40;
    }
}


impl Default for Status {
    fn default() -> Status {
        Status::FE_NONE
    }
}


pub const INVERSION_OFF: u32 = 0x00;
pub const INVERSION_ON: u32 = 0x01;
pub const INVERSION_AUTO: u32 = 0x02;


pub const FEC_NONE: u32 = 0x00;
pub const FEC_1_2: u32 = 0x01;
pub const FEC_2_3: u32 = 0x02;
pub const FEC_3_4: u32 = 0x03;
pub const FEC_4_5: u32 = 0x04;
pub const FEC_5_6: u32 = 0x05;
pub const FEC_6_7: u32 = 0x06;
pub const FEC_7_8: u32 = 0x07;
pub const FEC_8_9: u32 = 0x08;
pub const FEC_AUTO: u32 = 0x09;
pub const FEC_3_5: u32 = 0x10;
pub const FEC_9_10: u32 = 0x11;
pub const FEC_2_5: u32 = 0x12;
pub const FEC_1_4: u32 = 0x13;
pub const FEC_1_3: u32 = 0x14;


pub const MODULATION_QPSK: u32 = 0x00;
pub const MODULATION_QAM_16: u32 = 0x01;
pub const MODULATION_QAM_32: u32 = 0x02;
pub const MODULATION_QAM_64: u32 = 0x03;
pub const MODULATION_QAM_128: u32 = 0x04;
pub const MODULATION_QAM_256: u32 = 0x05;
pub const MODULATION_QAM_AUTO: u32 = 0x06;
pub const MODULATION_VSB_8: u32 = 0x07;
pub const MODULATION_VSB_16: u32 = 0x08;
pub const MODULATION_PSK_8: u32 = 0x09;
pub const MODULATION_APSK_16: u32 = 0x10;
pub const MODULATION_APSK_32: u32 = 0x11;
pub const MODULATION_DQPSK: u32 = 0x12;
pub const MODULATION_QAM_4_NR: u32 = 0x13;
pub const MODULATION_APSK_64: u32 = 0x14;
pub const MODULATION_APSK_128: u32 = 0x15;
pub const MODULATION_APSK_256: u32 = 0x16;


pub const TRANSMISSION_MODE_2K: u32 = 0x00;
pub const TRANSMISSION_MODE_8K: u32 = 0x01;
pub const TRANSMISSION_MODE_AUTO: u32 = 0x02;
pub const TRANSMISSION_MODE_4K: u32 = 0x03;
pub const TRANSMISSION_MODE_1K: u32 = 0x04;
pub const TRANSMISSION_MODE_16K: u32 = 0x05;
pub const TRANSMISSION_MODE_32K: u32 = 0x06;
pub const TRANSMISSION_MODE_C1: u32 = 0x07;
pub const TRANSMISSION_MODE_C3780: u32 = 0x08;


pub const GUARD_INTERVAL_1_32: u32 = 0x00;
pub const GUARD_INTERVAL_1_16: u32 = 0x01;
pub const GUARD_INTERVAL_1_8: u32 = 0x02;
pub const GUARD_INTERVAL_1_4: u32 = 0x03;
pub const GUARD_INTERVAL_AUTO: u32 = 0x04;
pub const GUARD_INTERVAL_1_128: u32 = 0x05;
pub const GUARD_INTERVAL_19_128: u32 = 0x06;
pub const GUARD_INTERVAL_19_256: u32 = 0x07;
pub const GUARD_INTERVAL_PN420: u32 = 0x08;
pub const GUARD_INTERVAL_PN595: u32 = 0x09;
pub const GUARD_INTERVAL_PN945: u32 = 0x10;


pub const HIERARCHY_NONE: u32 = 0x00;
pub const HIERARCHY_1: u32 = 0x01;
pub const HIERARCHY_2: u32 = 0x02;
pub const HIERARCHY_4: u32 = 0x03;
pub const HIERARCHY_AUTO: u32 = 0x04;


pub const PILOT_ON: u32 = 0x00;
pub const PILOT_OFF: u32 = 0x01;
pub const PILOT_AUTO: u32 = 0x02;


pub const ROLLOFF_35: u32 = 0x00;
pub const ROLLOFF_20: u32 = 0x01;
pub const ROLLOFF_25: u32 = 0x02;
pub const ROLLOFF_AUTO: u32 = 0x03;
pub const ROLLOFF_15: u32 = 0x04;
pub const ROLLOFF_10: u32 = 0x05;
pub const ROLLOFF_5: u32 = 0x06;


pub const SYS_UNDEFINED: u32 = 0x00;
pub const SYS_DVBC_ANNEX_A: u32 = 0x01;
pub const SYS_DVBC_ANNEX_B: u32 = 0x02;
pub const SYS_DVBT: u32 = 0x03;
pub const SYS_DSS: u32 = 0x04;
pub const SYS_DVBS: u32 = 0x05;
pub const SYS_DVBS2: u32 = 0x06;
pub const SYS_DVBH: u32 = 0x07;
pub const SYS_ISDBT: u32 = 0x08;
pub const SYS_ISDBS: u32 = 0x09;
pub const SYS_ISDBC: u32 = 0x10;
pub const SYS_ATSC: u32 = 0x11;
pub const SYS_ATSCMH: u32 = 0x12;
pub const SYS_DTMB: u32 = 0x13;
pub const SYS_CMMB: u32 = 0x14;
pub const SYS_DAB: u32 = 0x15;
pub const SYS_DVBT2: u32 = 0x16;
pub const SYS_TURBO: u32 = 0x17;
pub const SYS_DVBC_ANNEX_C: u32 = 0x18;
pub const SYS_DVBC2: u32 = 0x19;


#[repr(C)]
pub union PropertyData {
    pub data: u32,
    _reserved: [u8; 56],
}


/// Store one of frontend command and its value
#[repr(C)]
pub struct Property {
    /// Digital TV command
    pub cmd: u32,
    _reserved: [u32; 3],
    /// Union with the values for the command
    pub u: PropertyData,
    /// Result of the command set (currently unused)
    pub result: i32,
}


impl Default for Property {
    #[inline]
    fn default() -> Property {
        unsafe { mem::zeroed::<Property>() }
    }
}


impl Property {
    pub fn new(cmd: u32, data: u32) -> Property {
        let mut prop = Property::default();
        prop.cmd = cmd;
        prop.u.data = data;
        prop
    }
}


#[repr(C)]
pub struct Parameters {
    /// (absolute) frequency in Hz for DVB-C/DVB-T/ATSC
    /// intermediate frequency in kHz for DVB-S
    pub frequency: u32,
    /// Unimplemented
    _reserved: [u8; 32],
}


#[repr(C)]
pub struct Event {
    pub status: Status,
    pub parameters: Parameters,
}


impl Default for Event {
    #[inline]
    fn default() -> Event {
        unsafe { mem::zeroed::<Event>() }
    }
}


pub fn get_event(fd: RawFd, event: &mut Event) -> io::Result<()> {
    const FE_GET_EVENT: libc::c_ulong = 2150133582;
    cvt!(libc::ioctl(fd, FE_GET_EVENT, event as *mut Event))
}

pub fn get_info(fd: RawFd, info: &mut Info) -> io::Result<()> {
    const FE_GET_INFO: libc::c_ulong = 2158522173;
    cvt!(libc::ioctl(fd, FE_GET_INFO, info as *mut Info))
}

pub fn set_property(fd: RawFd, props: &[Property]) -> io::Result<()> {
    const FE_SET_PROPERTY: libc::c_ulong = 1074818898;

    #[repr(C)] struct Properties(u32, *const Property);
    let properties = Properties(props.len() as u32, props.as_ptr());

    cvt!(libc::ioctl(fd, FE_SET_PROPERTY, &properties as *const Properties))
}

pub fn read_status(fd: RawFd, status: &mut Status) -> io::Result<()> {
    const FE_READ_STATUS: libc::c_ulong = 2147774277;
    status.bits = 0;
    cvt!(libc::ioctl(fd, FE_READ_STATUS, &mut status.bits as *mut u32))
}

pub fn read_signal(fd: RawFd, value: &mut u32) -> io::Result<()> {
    const FE_READ_SIGNAL_STRENGTH: libc::c_ulong = 2147643207;
    *value = 0;
    cvt!(libc::ioctl(fd, FE_READ_SIGNAL_STRENGTH, value as *mut u32))
}

pub fn read_snr(fd: RawFd, value: &mut u32) -> io::Result<()> {
    const FE_READ_SNR: libc::c_ulong = 2147643208;
    *value = 0;
    cvt!(libc::ioctl(fd, FE_READ_SNR, value as *mut u32))
}

pub fn read_ber(fd: RawFd, value: &mut u32) -> io::Result<()> {
    const FE_READ_BER: libc::c_ulong = 2147774278;
    *value = 0;
    cvt!(libc::ioctl(fd, FE_READ_BER, value as *mut u32))
}

pub fn read_unc(fd: RawFd, value: &mut u32) -> io::Result<()> {
    const FE_READ_UNCORRECTED_BLOCKS: libc::c_ulong = 2147774281;
    *value = 0;
    cvt!(libc::ioctl(fd, FE_READ_UNCORRECTED_BLOCKS, value as *mut u32))
}

pub fn set_tone(fd: RawFd, tone: u32) -> io::Result<()> {
    const FE_SET_TONE: libc::c_ulong = 28482;
    cvt!(libc::ioctl(fd, FE_SET_TONE, tone))
}

pub fn set_voltage(fd: RawFd, voltage: u32) -> io::Result<()> {
    const FE_SET_VOLTAGE: libc::c_ulong = 28483;
    cvt!(libc::ioctl(fd, FE_SET_VOLTAGE, voltage))
}
