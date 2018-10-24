use libc;

use std::{io, mem};
use std::os::unix::io::RawFd;

pub const FE_GET_INFO: libc::c_ulong = 2158522173; /* _IOR('o', 61, sizeof(dvb_frontend_info)) */
pub const FE_SET_PROPERTY: libc::c_ulong = 1074818898; /* _IOW('o', 82, struct dtv_properties) */

//

bitflags! {
    pub struct fe_caps: u32 {
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
pub struct dvb_frontend_info {
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
	pub caps: fe_caps,
}

unsafe fn get_dvb_frontend_info(fd: RawFd) -> io::Result<dvb_frontend_info> {
    let mut feinfo: dvb_frontend_info = mem::zeroed();
    let x = libc::ioctl(fd, FE_GET_INFO, &mut feinfo as *mut dvb_frontend_info as *mut libc::c_void);
    if x == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(feinfo)
    }
}

//

#[repr(C, packed)]
pub union dtv_property_data {
    pub data: u32,
    reserved: [u8; 56],
}

#[repr(C, packed)]
pub struct dtv_property {
    pub cmd: u32,
    reserved: [u32; 3],
    pub u: dtv_property_data,
    pub result: i32,
}

#[repr(C, packed)]
pub struct dtv_properties {
    pub num: u32,
    pub props: [dtv_property; 20],
}

unsafe fn set_dtv_properties(fd: RawFd, cmdseq: &dtv_properties) -> io::Result<()> {
    let x = libc::ioctl(fd, FE_GET_INFO, cmdseq as *const dtv_properties as *const libc::c_void);
    if x == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Adapter
pub struct Adapter {
    /// Adapter number /dev/dvb/adapterX
    pub adapter: usize,
    /// Device number /dev/dvb/adapterX/frontendX
    pub device: usize,
}

impl Adapter {
    pub fn open(&self) -> io::Result<RawFd> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", self.adapter, self.device);
        let fd = unsafe {
            libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR)
        };

        if fd == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }
}

/// Modulation
#[allow(non_camel_case_types)]
pub enum Modulation {
    /// Depend of delivery system.
    AUTO,
    NONE,
    PSK_8,
    QPSK,
    QAM_16,
    QAM_32,
    QAM_64,
    QAM_128,
    QAM_256,
    VSB_8,
    VSB_16,
    APSK_16,
    APSK_32,
    DQPSK,
}

/// FEC - Forward Error Correction
#[allow(non_camel_case_types)]
pub enum Fec {
    AUTO,
    NONE,
    FEC_1_2,
    FEC_2_3,
    FEC_3_4,
    FEC_4_5,
    FEC_5_6,
    FEC_6_7,
    FEC_7_8,
    FEC_8_9,
    FEC_3_5,
    FEC_9_10,
}

/// DVB-S/S2 Transponder polarization
pub enum Polarization {
    /// Vertical linear. Right circular. 13 volt
    VR,
    /// Horizontal linear. Left circular. 18 volt
    HL,
    /// Disable LNB power
    OFF,
}

/// DVB-S/S2 Unicable options
pub struct Unicable10 {
    /// Slot range from 1 to 8
    pub slot: usize,
    /// Frequency range from 950 to 2150 MHz
    pub frequency: usize,
    /// Position range from 1 to 2
    pub position: usize,
}

/// DVB-S/S2 LNB mode
#[allow(non_camel_case_types)]
pub enum LnbMode {
    /// Send 22kHz tone to LNB if frequency greater or equal to slof
    AUTO,
    /// Send 22kHz tone to LNB
    TONE,
    /// Tone Burst port range from 1 to 2
    TONEBURST(usize),
    /// DiSEqC 1.0 port range from 1 to 4
    DISEQC_1_0(usize),
    /// DiSEqC 1.1 port range from 1 to 16
    DISEQC_1_1(usize),
    /// EN50494 / Unicable
    UNICABLE_1_0(Unicable10),
    /// Disable LNB
    OFF,
}

/// DVB-S2 Roll-off
#[allow(non_camel_case_types)]
pub enum Rof {
    AUTO,
    ROF_20,
    ROF_25,
    ROF_35,
}

/// DVB-S/S2 Transponder
pub struct Transponder {
    /// Frequency
    pub frequency: usize,
    /// Polarization
    pub polarization: Polarization,
    /// Symbol-rate
    pub symbolrate: usize,
}

/// DVB-S/S2 LNB
pub struct Lnb {
    /// Mode
    pub mode: LnbMode,
    /// Low band frequency
    pub lof1: usize,
    /// High band frequency
    pub lof2: usize,
    /// Threshold frequency - threshold between low and high band
    pub slof: usize,
}

/// DVB-S Options
pub struct DvbS {
    pub adapter: Adapter,
    pub transponder: Transponder,
    pub lnb: Lnb,
    pub modulation: Modulation,
    pub fec: Fec,
}

/// DVB-S2 Options
pub struct DvbS2 {
    pub adapter: Adapter,
    pub transponder: Transponder,
    pub lnb: Lnb,
    pub modulation: Modulation,
    pub fec: Fec,
    pub rof: Rof,
}

/// DVB Delivery system
#[allow(non_camel_case_types)]
pub enum DvbOptions {
    DVB_S2(DvbS2),
}

/// DVB Instance
pub struct DvbTune {
    fd: RawFd,
    feinfo: dvb_frontend_info,
}

impl DvbTune {
    pub fn new(options: &DvbOptions) -> io::Result<DvbTune> {
        match options {
            DvbOptions::DVB_S2(v) => {
                let fd = v.adapter.open()?;

                let feinfo = unsafe {
                    get_dvb_frontend_info(fd)?
                };

                // TODO: continue here...
                // DvbS::tune(v)?;

                Ok(DvbTune{ fd, feinfo, })
            },
        }
    }
}

impl Drop for DvbTune {
    fn drop(&mut self) {
        if self.fd > 0 {
            unsafe { libc::close(self.fd) };
            self.fd = 0;
        }
    }
}
