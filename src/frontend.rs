use libc;

use std::{io, mem};
use std::os::unix::io::RawFd;

pub const FE_GET_INFO: libc::c_ulong = 2158522173; /* _IOR('o', 61, sizeof(dvb_frontend_info)) */
pub const FE_GET_EVENT: libc::c_ulong = 2150133582; /* _IOR('o', 78, struct dvb_frontend_event) */
pub const FE_SET_PROPERTY: libc::c_ulong = 1074818898; /* _IOW('o', 82, struct dtv_properties) */

//

bitflags! {
    pub struct Caps: u32 {
        const FE_IS_STUPID = 0;
        const FE_CAN_INVERSION_AUTO = 0x1;
        const FE_CAN_FEC_1_2 = 0x2;
        const FE_CAN_FEC_2_3 = 0x4;
        const FE_CAN_FEC_3_4 = 0x8;
        const FE_CAN_FEC_4_5 = 0x10;
        const FE_CAN_FEC_5_6 = 0x20;
        const FE_CAN_FEC_6_7 = 0x40;
        const FE_CAN_FEC_7_8 = 0x80;
        const FE_CAN_FEC_8_9 = 0x100;
        const FE_CAN_FEC_AUTO = 0x200;
        const FE_CAN_QPSK = 0x400;
        const FE_CAN_QAM_16 = 0x800;
        const FE_CAN_QAM_32 = 0x1000;
        const FE_CAN_QAM_64 = 0x2000;
        const FE_CAN_QAM_128 = 0x4000;
        const FE_CAN_QAM_256 = 0x8000;
        const FE_CAN_QAM_AUTO = 0x10000;
        const FE_CAN_TRANSMISSION_MODE_AUTO = 0x20000;
        const FE_CAN_BANDWIDTH_AUTO = 0x40000;
        const FE_CAN_GUARD_INTERVAL_AUTO = 0x80000;
        const FE_CAN_HIERARCHY_AUTO = 0x100000;
        const FE_CAN_8VSB = 0x200000;
        const FE_CAN_16VSB = 0x400000;
        const FE_HAS_EXTENDED_CAPS = 0x800000;
        const FE_CAN_MULTISTREAM = 0x4000000;
        const FE_CAN_TURBO_FEC = 0x8000000;
        const FE_CAN_2G_MODULATION = 0x10000000;
        const FE_NEEDS_BENDING = 0x20000000;
        const FE_CAN_RECOVER = 0x40000000;
        const FE_CAN_MUTE_TS 	 = 0x80000000;
    }
}

#[repr(C, packed)]
pub struct Info {
    pub name: [libc::c_char; 128],
	pub fe_type: i32, /* DEPRECATED. Use DTV_ENUM_DELSYS instead */
	pub frequency_min: u32,
	pub frequency_max: u32,
	pub frequency_stepsize: u32,
	pub frequency_tolerance: u32,
	pub symbol_rate_min: u32,
	pub symbol_rate_max: u32,
	pub symbol_rate_tolerance: u32,
	pub notifier_delay: u32, /* DEPRECATED */
	pub caps: Caps,
}

impl Info {
    pub fn new() -> Info {
        unsafe { mem::zeroed::<Info>() }
    }

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

//

#[repr(C, packed)]
pub struct Parameters {
    pub frequency: u32,
    reserved: [u8; 32],
}

bitflags! {
    pub struct Status: u32 {
        const FE_NONE = 0x00;
        const FE_HAS_SIGNAL = 0x01;
        const FE_HAS_CARRIER = 0x02;
        const FE_HAS_VITERBI = 0x04;
        const FE_HAS_SYNC = 0x08;
        const FE_HAS_LOCK = 0x10;
        const FE_TIMEDOUT = 0x20;
        const FE_REINIT = 0x40;
    }
}

#[repr(C, packed)]
pub struct Event {
    pub status: Status,
    pub parameters: Parameters,
}

impl Event {
    pub fn new() -> Event {
        unsafe { mem::zeroed::<Event>() }
    }

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

//

const DTV_CLEAR: u32 = 2;

#[repr(C, packed)]
pub union PropertyData {
    pub data: u32,
    reserved: [u8; 56],
}

#[repr(C, packed)]
pub struct Property {
    pub cmd: u32,
    reserved: [u32; 3],
    pub u: PropertyData,
    pub result: i32,
}

#[repr(C, packed)]
pub struct Properties {
    pub num: u32,
    pub props: [Property; 20],
}

impl Properties {
    pub fn new() -> Properties {
        unsafe { mem::zeroed::<Properties>() }
    }

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

//

pub fn open(adapter: usize, device: usize) -> io::Result<RawFd> {
    let path = format!("/dev/dvb/adapter{}/frontend{}", adapter, device);
    let fd = unsafe {
        libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR)
    };

    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

pub fn clear(fd: RawFd) -> io::Result<()> {
    let mut cmdseq = Properties::new();
    cmdseq.num = 1;
    cmdseq.props[0].cmd = DTV_CLEAR;
    cmdseq.write(fd)?;

    let mut e = Event::new();
    while let Ok(_) = e.read(fd) {};

    Ok(())
}

pub fn close(fd: RawFd) {
    clear(fd).unwrap();
    unsafe { libc::close(fd) };
}
