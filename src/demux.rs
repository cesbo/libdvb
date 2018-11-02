/// DVB-API v5.11
/// System level demux API

use libc;

use base::cvt;
use std::{io, mem};
use std::os::unix::io::RawFd;

pub const DMX_FILTER_SIZE: u32 = 16;

pub const DMX_OUT_DECODER: u32 = 0;
pub const DMX_OUT_TAP: u32 = 1;
pub const DMX_OUT_TS_TAP: u32 = 2;
pub const DMX_OUT_TSDEMUX_TAP: u32 = 3;

pub const DMX_IN_FRONTEND: u32 = 0;
pub const DMX_IN_DVR: u32 = 1;

pub const DMX_PES_AUDIO0: u32 = 0;
pub const DMX_PES_VIDEO0: u32 = 1;
pub const DMX_PES_TELETEXT0: u32 = 2;
pub const DMX_PES_SUBTITLE0: u32 = 3;
pub const DMX_PES_PCR0: u32 = 4;

pub const DMX_PES_AUDIO1: u32 = 5;
pub const DMX_PES_VIDEO1: u32 = 6;
pub const DMX_PES_TELETEXT1: u32 = 7;
pub const DMX_PES_SUBTITLE1: u32 = 8;
pub const DMX_PES_PCR1: u32 = 9;

pub const DMX_PES_AUDIO2: u32 = 10;
pub const DMX_PES_VIDEO2: u32 = 11;
pub const DMX_PES_TELETEXT2: u32 = 12;
pub const DMX_PES_SUBTITLE2: u32 = 13;
pub const DMX_PES_PCR2: u32 = 14;

pub const DMX_PES_AUDIO3: u32 = 15;
pub const DMX_PES_VIDEO3: u32 = 16;
pub const DMX_PES_TELETEXT3: u32 = 17;
pub const DMX_PES_SUBTITLE3: u32 = 18;
pub const DMX_PES_PCR3: u32 = 19;

pub const DMX_PES_OTHER: u32 = 20;

pub const DMX_PES_AUDIO: u32 = DMX_PES_AUDIO0;
pub const DMX_PES_VIDEO: u32 = DMX_PES_VIDEO0;
pub const DMX_PES_TELETEXT: u32 = DMX_PES_TELETEXT0;
pub const DMX_PES_SUBTITLE: u32 = DMX_PES_SUBTITLE0;
pub const DMX_PES_PCR: u32 = DMX_PES_PCR0;

/// only deliver sections where the CRC check succeeded
pub const DMX_CHECK_CRC: u32 = 1;
/// disable the section filter after one section has been delivered
pub const DMX_ONESHOT: u32 = 2;
// Start filter immediately without requiring a DMX_START
pub const DMX_IMMEDIATE_START: u32 = 4;

#[repr(C)]
pub struct PesFilterParams {
    pub pid: u16,
    pub input: u32,
    pub output: u32,
    pub pes_type: u32,
    pub flags: u32,
}

impl Default for PesFilterParams {
    #[inline]
    fn default() -> PesFilterParams {
        unsafe { mem::zeroed::<PesFilterParams>() }
    }
}

// ioctl

pub fn start(fd: RawFd) -> io::Result<()> {
    const DMX_START: libc::c_ulong = 28457;

    cvt(unsafe {
        libc::ioctl(fd, DMX_START)
    })
}

pub fn stop(fd: RawFd) -> io::Result<()> {
    const DMX_STOP: libc::c_ulong = 28458;

    cvt(unsafe {
        libc::ioctl(fd, DMX_STOP)
    })
}

pub fn set_pes_filter(fd: RawFd, params: &PesFilterParams) -> io::Result<()> {
    const DMX_SET_PES_FILTER: libc::c_ulong = 1075081004;

    cvt(unsafe {
        libc::ioctl(fd, DMX_SET_PES_FILTER, params as *const PesFilterParams)
    })
}

pub fn set_buffer_size(fd: RawFd, size: u64) -> io::Result<()> {
    const DMX_SET_BUFFER_SIZE: libc::c_ulong = 28461;

    cvt(unsafe {
        libc::ioctl(fd, DMX_SET_BUFFER_SIZE, size)
    })
}

pub fn add_pid(fd: RawFd, pid: u16) -> io::Result<()> {
    const DMX_ADD_PID: libc::c_ulong = 1073901363;

    cvt(unsafe {
        libc::ioctl(fd, DMX_ADD_PID, u32::from(pid))
    })
}

pub fn remove_pid(fd: RawFd, pid: u16) -> io::Result<()> {
    const DMX_REMOVE_PID: libc::c_ulong = 1073901364;

    cvt(unsafe {
        libc::ioctl(fd, DMX_REMOVE_PID, u32::from(pid))
    })
}
