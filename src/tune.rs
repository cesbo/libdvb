/// Library level frontend API

use libc;
use frontend;

use std::io;
use std::os::unix::io::RawFd;

/// Adapter
pub struct Adapter {
    /// Adapter number /dev/dvb/adapterX
    pub id: usize,
    /// Device number /dev/dvb/adapterX/frontendX
    pub device: usize,
    /// Modulation
    pub modulation: u32,
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

/// DVB-S/S2 Transponder
pub struct Transponder {
    /// Frequency
    pub frequency: usize,
    /// Polarization: SEC_VOLTAGE_13 for V/R, SEC_VOLTAGE_18 for H/L
    pub polarization: u32,
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
    pub fec: u32,
}

/// DVB-S2 Options
pub struct DvbS2 {
    pub adapter: Adapter,
    pub transponder: Transponder,
    pub lnb: Lnb,
    pub fec: u32,
    pub rof: u32,
}

/// DVB Delivery system
#[allow(non_camel_case_types)]
pub enum DvbOptions {
    DVB_S2(DvbS2),
}

/// DVB Instance
#[derive(Default)]
pub struct DvbTune {
    fd: RawFd,
    info: frontend::Info,
}

impl DvbTune {
    /// Clears frontend and event queue
    fn clear(&self) -> io::Result<()> {
        let mut cmdseq = frontend::Properties::default();
        cmdseq.num = 1;
        cmdseq.props[0].cmd = frontend::DTV_CLEAR;
        frontend::set_property(self.fd, &mut cmdseq)?;

        let mut e = frontend::Event::default();
        while let Ok(_) = frontend::get_event(self.fd, &mut e) {};

        Ok(())
    }

    /// Closes frontend
    pub fn close(&mut self) {
        if self.fd > 0 {
            self.clear().unwrap();
            unsafe { libc::close(self.fd) };
            self.fd = 0;
        }
    }

    /// Opens fronted
    fn open(&mut self, adapter: &Adapter) -> io::Result<()> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", adapter.id, adapter.device);
        self.fd = unsafe {
            libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR)
        };

        if self.fd == -1 {
            self.fd = 0;
            Err(io::Error::last_os_error())
        } else {
            frontend::get_info(self.fd, &mut self.info)
        }
    }

    pub fn new(options: &DvbOptions) -> io::Result<DvbTune> {
        let mut x = DvbTune::default();

        match options {
            DvbOptions::DVB_S2(v) => {
                x.open(&v.adapter)?;

                // TODO: continue here...
                // DvbS::tune(v)?;
            },
        };

        Ok(x)
    }
}

impl Drop for DvbTune {
    fn drop(&mut self) {
        self.close();
    }
}
