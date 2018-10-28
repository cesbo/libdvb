/// Library level frontend API

use libc;
use frontend;

use std::{io, time, thread};
use std::os::unix::io::RawFd;

pub trait Dvb {
    fn open(&self) -> io::Result<RawFd>;
}

/// Adapter
pub struct Adapter {
    /// Adapter number /dev/dvb/adapterX
    pub id: u32,
    /// Device number /dev/dvb/adapterX/frontendX
    pub device: u32,
    /// Modulation
    pub modulation: u32,
}

fn open(adapter: &Adapter) -> io::Result<RawFd> {
    let path = format!("/dev/dvb/adapter{}/frontend{}", adapter.id, adapter.device);
    let fd = unsafe {
        libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR)
    };

    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn clear(fd: RawFd) -> io::Result<()> {
    let cmdseq = vec![
        frontend::Property::new(frontend::DTV_CLEAR, 0),
    ];
    frontend::set_property(fd, &cmdseq)?;

    let mut event = frontend::Event::default();
    loop {
        if let Err(e) = frontend::get_event(fd, &mut event) {
            if let Some(r) = e.raw_os_error() {
                if r == libc::EWOULDBLOCK {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn close(fd: RawFd) {
    clear(fd).unwrap();
    unsafe { libc::close(fd) };
}

/// DVB-S/S2 Unicable options
pub struct Unicable10 {
    /// Slot range from 1 to 8
    pub slot: u32,
    /// Frequency range from 950 to 2150 MHz
    pub frequency: u32,
    /// Position range from 1 to 2
    pub position: u32,
}

/// DVB-S/S2 LNB mode
#[allow(non_camel_case_types)]
pub enum LnbMode {
    /// Send 22kHz tone to LNB if frequency greater or equal to slof
    AUTO,
    /// Send 22kHz tone to LNB
    TONE,
    /// Tone Burst port range from 1 to 2
    TONEBURST(u32),
    /// DiSEqC 1.0 port range from 1 to 4
    DISEQC_1_0(u32),
    /// DiSEqC 1.1 port range from 1 to 16
    DISEQC_1_1(u32),
    /// EN50494 / Unicable
    UNICABLE_1_0(Unicable10),
    /// Disable LNB
    OFF,
}

/// DVB-S/S2 Transponder
pub struct Transponder {
    /// Frequency
    pub frequency: u32,
    /// Polarization: SEC_VOLTAGE_13 for V/R, SEC_VOLTAGE_18 for H/L
    pub polarization: u32,
    /// Symbol-rate
    pub symbolrate: u32,
}

/// DVB-S/S2 LNB
pub struct Lnb {
    /// Mode
    pub mode: LnbMode,
    /// Low band frequency
    pub lof1: u32,
    /// High band frequency
    pub lof2: u32,
    /// Threshold frequency - threshold between low and high band
    pub slof: u32,
}

/// DVB-S2 Options
pub struct DvbS2 {
    pub adapter: Adapter,
    pub transponder: Transponder,
    pub lnb: Lnb,
    pub fec: u32,
    pub rof: u32,
    pub mis: u32,
}

impl Dvb for DvbS2 {
    fn open(&self) -> io::Result<RawFd> {
        let fd = open(&self.adapter)?;
        clear(fd)?;

        let mut frequency: u32 = self.transponder.frequency;
        let mut tone = frontend::SEC_TONE_OFF;

        if self.lnb.lof1 > 0 {
            if self.lnb.slof > 0 &&
                self.lnb.lof2 > 0 &&
                frequency >= self.lnb.slof
            {
                /* hiband */
                frequency -= self.lnb.lof2;
                tone = frontend::SEC_TONE_ON;
            }
            else
            {
                if self.lnb.lof1 > frequency {
                    frequency = self.lnb.lof1 - frequency;
                } else {
                    frequency -= self.lnb.lof1;
                }
            }
        } else {
            if frequency >= 950 && frequency <= 2150 {
                //
            } else if frequency >= 2500 && frequency <= 2700 {
                frequency = 3650 - frequency;
            } else if frequency >= 3400 && frequency <= 4200 {
                frequency = 5150 - frequency;
            } else if frequency >= 4500 && frequency <= 4800 {
                frequency = 5950 - frequency;
            } else if frequency >= 10700 && frequency < 11700 {
                frequency -= 9750;
            } else if frequency >= 11700 && frequency < 13250 {
                frequency -= 10600;
                tone = frontend::SEC_TONE_ON;
            } else {
                return Err(io::Error::from_raw_os_error(libc::EINVAL));
            }
        }
        frequency *= 1000;

        match self.lnb.mode {
            LnbMode::AUTO => {
                frontend::set_tone(fd, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd, self.transponder.polarization)?;
                thread::sleep(time::Duration::from_millis(100));
                frontend::set_tone(fd, tone)?;
                thread::sleep(time::Duration::from_millis(100));
            },
            LnbMode::TONE => {
                frontend::set_tone(fd, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd, self.transponder.polarization)?;
                thread::sleep(time::Duration::from_millis(100));
                frontend::set_tone(fd, frontend::SEC_TONE_ON)?;
                thread::sleep(time::Duration::from_millis(100));
            },
            LnbMode::OFF => {
                frontend::set_tone(fd, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd, frontend::SEC_VOLTAGE_OFF)?;
            },
            _ => {},
        };

        let cmdseq = vec![
            frontend::Property::new(frontend::DTV_DELIVERY_SYSTEM, frontend::SYS_DVBS2),
            frontend::Property::new(frontend::DTV_FREQUENCY, frequency),
            frontend::Property::new(frontend::DTV_MODULATION, self.adapter.modulation),
            frontend::Property::new(frontend::DTV_INVERSION, frontend::INVERSION_AUTO),
            frontend::Property::new(frontend::DTV_SYMBOL_RATE, self.transponder.symbolrate * 1000),
            frontend::Property::new(frontend::DTV_INNER_FEC, self.fec),
            frontend::Property::new(frontend::DTV_PILOT, frontend::PILOT_AUTO),
            frontend::Property::new(frontend::DTV_ROLLOFF, self.rof),
            frontend::Property::new(frontend::DTV_STREAM_ID, 0),
            frontend::Property::new(frontend::DTV_SCRAMBLING_SEQUENCE_INDEX, 0),
            frontend::Property::new(frontend::DTV_TUNE, 0),
        ];
        frontend::set_property(fd, &cmdseq)?;

        Ok(fd)
    }
}

/// DVB Instance
#[derive(Default)]
pub struct DvbTune {
    fd: RawFd,
}

impl DvbTune {
    pub fn new(dvb: &Dvb) -> io::Result<DvbTune> {
        let mut x = DvbTune::default();
        x.fd = dvb.open()?;
        Ok(x)
    }

    pub fn close(&mut self) {
        if self.fd > 0 {
            close(self.fd);
            self.fd = 0;
        }
    }
}

impl Drop for DvbTune {
    fn drop(&mut self) {
        self.close();
    }
}
