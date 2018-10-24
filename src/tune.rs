use libc;

use std::{io, mem, fmt, ffi};
use std::os::unix::io::RawFd;

pub const FE_GET_INFO: libc::c_ulong = 2158522173; /* '_IOR('o', 61, sizeof(dvb_frontend_info)) */

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

#[repr(C)]
#[derive(Clone, Copy)]
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

impl fmt::Debug for dvb_frontend_info {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = unsafe { ffi::CStr::from_ptr(self.name.as_ptr()).to_string_lossy() };

        f.debug_struct("dvb_frontend_info")
            .field("name", &name)
            .field("frequency_min", &self.frequency_min)
            .field("frequency_max", &self.frequency_max)
            .field("frequency_stepsize", &self.frequency_stepsize)
            .field("frequency_tolerance", &self.frequency_tolerance)
            .field("symbol_rate_min", &self.symbol_rate_min)
            .field("symbol_rate_max", &self.symbol_rate_max)
            .field("symbol_rate_tolerance", &self.symbol_rate_tolerance)
            .field("caps", &self.caps)
            .finish()
    }
}

/// Modulation
#[allow(non_camel_case_types)]
#[derive(Debug)]
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

impl Default for Modulation {
    fn default() -> Modulation { Modulation::AUTO }
}

/// FEC - Forward Error Correction
#[allow(non_camel_case_types)]
#[derive(Debug)]
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

impl Default for Fec {
    fn default() -> Fec { Fec::AUTO }
}

/// DVB-S/S2 Transponder polarization
#[derive(Debug)]
pub enum Polarization {
    /// Vertical linear. Right circular. 13 volt
    VR,
    /// Horizontal linear. Left circular. 18 volt
    HL,
    /// Disable LNB power
    OFF,
}

impl Default for Polarization {
    fn default() -> Polarization { Polarization::OFF }
}

/// DVB-S/S2 Unicable options
#[derive(Default, Debug)]
pub struct Unicable10 {
    /// Slot range from 1 to 8
    slot: usize,
    /// Frequency range from 950 to 2150 MHz
    frequency: usize,
    /// Position range from 1 to 2
    position: usize,
}

/// DVB-S/S2 LNB mode
#[allow(non_camel_case_types)]
#[derive(Debug)]
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

impl Default for LnbMode {
    fn default() -> LnbMode { LnbMode::AUTO }
}

/// DVB-S2 Roll-off
#[allow(non_camel_case_types)]
#[derive(Debug)]
pub enum Rof {
    AUTO,
    ROF_20,
    ROF_25,
    ROF_35,
}

impl Default for Rof {
    fn default() -> Rof { Rof::ROF_35 }
}

/// DVB-S/S2 Transponder
#[derive(Default, Debug)]
pub struct Transponder {
    /// Frequency
    frequency: usize,
    /// Polarization
    polarization: Polarization,
    /// Symbol-rate
    symbol_rate: usize,
}

/// DVB-S/S2 LNB
#[derive(Default, Debug)]
pub struct Lnb {
    /// Mode
    lnb: LnbMode,
    /// Low band frequency
    lof1: usize,
    /// High band frequency
    lof2: usize,
    /// Threshold frequency - threshold between low and high band
    slof: usize,
}

/// DVB-S Options
#[derive(Default, Debug)]
pub struct DvbS {
    transponder: Transponder,
    lnb: Lnb,
    modulation: Modulation,
    fec: Fec,
}

/// DVB-S2 Options
#[derive(Debug)]
pub struct DvbS2 {
    transponder: Transponder,
    lnb: Lnb,
    modulation: Modulation,
    fec: Fec,
    rof: Rof,
}

/// DVB Delivery system
#[allow(non_camel_case_types)]
#[derive(Debug)]
pub enum DvbSystem {
    NONE,
    DVB_S(DvbS),
    DVB_S2(DvbS2),
}

/// DVB Options
#[derive(Debug)]
pub struct DvbOptions {
    /// Adapter number /dev/dvb/adapterX
    adapter: usize,
    /// Device number /dev/dvb/adapterX/frontendX
    device: usize,
    /// Delivery system
    system: DvbSystem,
}

#[derive(Debug)]
pub struct DvbTune {
    fd: RawFd,
}

impl DvbTune {
    pub fn new(adapter: usize, device: usize) -> io::Result<DvbTune> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", adapter, device);

        let fd = unsafe {
            let fd = libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR);
            if fd == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(fd)
            }
        }?;

        let feinfo = unsafe {
            let mut feinfo: dvb_frontend_info = mem::zeroed();
            let x = libc::ioctl(fd, FE_GET_INFO, &mut feinfo as *mut dvb_frontend_info as *mut libc::c_void);
            if x == -1 {
                libc::close(fd);
                Err(io::Error::last_os_error())
            } else {
                Ok(feinfo)
            }
        }?;

        println!("{:#?}", feinfo);

        Ok(DvbTune {
            fd,
        })
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
