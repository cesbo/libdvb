/// System level frontend API

use libc;

use std::{io, mem};
use std::os::unix::io::RawFd;

pub const FE_GET_INFO: libc::c_ulong = 2158522173; /* _IOR('o', 61, sizeof(dvb_frontend_info)) */
pub const FE_GET_EVENT: libc::c_ulong = 2150133582; /* _IOR('o', 78, struct dvb_frontend_event) */
pub const FE_SET_PROPERTY: libc::c_ulong = 1074818898; /* _IOW('o', 82, struct dtv_properties) */

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
#[repr(C, packed)]
pub struct Info {
    /// Name of the frontend
    pub name: [libc::c_char; 128],
    fe_type: i32, /* DEPRECATED. Use DTV_ENUM_DELSYS instead */
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
    notifier_delay: u32, /* DEPRECATED */
    /// Capabilities supported by the frontend
    pub caps: Caps,
}

impl Default for Info {
    fn default() -> Info {
        unsafe { mem::zeroed::<Info>() }
    }
}

impl Info {
    /// Reads frontend information
    pub fn read(&mut self, fd: RawFd) -> io::Result<()> {
        let x = unsafe {
            libc::ioctl(fd, FE_GET_INFO, self as *mut Info as *mut libc::c_void)
        };

        if x == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[repr(C, packed)]
pub struct Parameters {
    /// (absolute) frequency in Hz for DVB-C/DVB-T/ATSC
    /// intermediate frequency in kHz for DVB-S
    pub frequency: u32,
    /// Unimplemented
    reserved: [u8; 32],
}

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

#[repr(C, packed)]
pub struct Event {
    pub status: Status,
    pub parameters: Parameters,
}

impl Default for Event {
    fn default() -> Event {
        unsafe { mem::zeroed::<Event>() }
    }
}

impl Event {
    /// Reads frontend event
    pub fn read(&mut self, fd: RawFd) -> io::Result<()> {
        let x = unsafe {
            libc::ioctl(fd, FE_GET_INFO, self as *mut Event as *mut libc::c_void)
        };
        if x == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

const DTV_CLEAR: u32 = 2;

#[repr(C, packed)]
pub union PropertyData {
    pub data: u32,
    reserved: [u8; 56],
}

/// Store one of frontend command and its value
#[repr(C, packed)]
pub struct Property {
    /// Digital TV command
    pub cmd: u32,
    reserved: [u32; 3],
    /// Union with the values for the command
    pub u: PropertyData,
    /// Result of the command set (currently unused)
    pub result: i32,
}

/// Set of command/value pairs
#[repr(C, packed)]
pub struct Properties {
    /// Amount of commands stored at the struct
    pub num: u32,
    /// Commands
    pub props: [Property; 20],
}

impl Default for Properties {
    fn default() -> Properties {
        unsafe { mem::zeroed::<Properties>() }
    }
}

impl Properties {
    /// Writes properties set into frontend
    pub fn write(&self, fd: RawFd) -> io::Result<()> {
        let x = unsafe {
            libc::ioctl(fd, FE_SET_PROPERTY, self as *const Properties as *const libc::c_void)
        };
        if x == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
