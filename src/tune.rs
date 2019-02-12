/// Library level frontend API

use libc;


use std::{io, time, thread};
use std::os::unix::io::RawFd;


use crate::frontend;


pub trait Dvb {
    fn open(&self) -> io::Result<DvbFd>;
}


#[derive(Default)]
pub struct DvbFd {
    inner: RawFd,
}


impl Drop for DvbFd {
    fn drop(&mut self) {
        self.close();
    }
}


impl DvbFd {
    pub fn open(id: u32, device: u32) -> io::Result<Self> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", id, device);
        let fd = unsafe {
            libc::open(path.as_ptr() as *const i8, libc::O_NONBLOCK | libc::O_RDWR)
        };

        if fd == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(DvbFd{ inner: fd })
        }
    }


    pub fn clear(&self) -> io::Result<()> {
        let cmdseq = vec![
            frontend::Property::new(frontend::DTV_CLEAR, 0),
        ];
        frontend::set_property(self.inner, &cmdseq)?;

        let mut event = frontend::Event::default();
        loop {
            if let Err(e) = frontend::get_event(self.inner, &mut event) {
                if let Some(r) = e.raw_os_error() {
                    if r == libc::EWOULDBLOCK {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn close(&mut self) {
        if self.inner > 0 {
            self.clear().unwrap();
            unsafe { libc::close(self.inner) };
            self.inner = 0;
        }
    }
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


/// DVB-S/S2 Unicable options
pub struct Unicable {
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
    UNICABLE_1_0(Unicable),
    /// EN50607 / Unicable-II
    UNICABLE_2_0(Unicable),
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
    fn open(&self) -> io::Result<DvbFd> {
        let fd = DvbFd::open(self.adapter.id, self.adapter.device)?;
        fd.clear()?;

        let symbolrate = self.transponder.symbolrate * 1000;
        let mut frequency = self.transponder.frequency;
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

        let mut info = frontend::Info::default();
        frontend::get_info(fd.inner, &mut info)?;

        if ! info.caps.contains(frontend::Caps::FE_CAN_2G_MODULATION) ||
            frequency < info.frequency_min || frequency > info.frequency_max ||
            symbolrate < info.symbol_rate_min || symbolrate > info.symbol_rate_max
        {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }

        match self.lnb.mode {
            LnbMode::AUTO => {
                frontend::set_tone(fd.inner, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd.inner, self.transponder.polarization)?;
                thread::sleep(time::Duration::from_millis(100));
                frontend::set_tone(fd.inner, tone)?;
                thread::sleep(time::Duration::from_millis(100));
            },
            LnbMode::TONE => {
                frontend::set_tone(fd.inner, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd.inner, self.transponder.polarization)?;
                thread::sleep(time::Duration::from_millis(100));
                frontend::set_tone(fd.inner, frontend::SEC_TONE_ON)?;
                thread::sleep(time::Duration::from_millis(100));
            },
            LnbMode::OFF => {
                frontend::set_tone(fd.inner, frontend::SEC_TONE_OFF)?;
                frontend::set_voltage(fd.inner, frontend::SEC_VOLTAGE_OFF)?;
            },
            _ => {},
        };

        let cmdseq = vec![
            frontend::Property::new(frontend::DTV_DELIVERY_SYSTEM, frontend::SYS_DVBS2),
            frontend::Property::new(frontend::DTV_FREQUENCY, frequency),
            frontend::Property::new(frontend::DTV_MODULATION, self.adapter.modulation),
            frontend::Property::new(frontend::DTV_INVERSION, frontend::INVERSION_AUTO),
            frontend::Property::new(frontend::DTV_SYMBOL_RATE, symbolrate),
            frontend::Property::new(frontend::DTV_INNER_FEC, self.fec),
            frontend::Property::new(frontend::DTV_PILOT, frontend::PILOT_AUTO),
            frontend::Property::new(frontend::DTV_ROLLOFF, self.rof),
            frontend::Property::new(frontend::DTV_STREAM_ID, 0),
            frontend::Property::new(frontend::DTV_SCRAMBLING_SEQUENCE_INDEX, 0),
            frontend::Property::new(frontend::DTV_TUNE, 0),
        ];
        frontend::set_property(fd.inner, &cmdseq)?;

        Ok(fd)
    }
}


/// DVB Instance
#[derive(Default)]
pub struct DvbTune {
    fd: DvbFd,

    pub status: frontend::Status,
    pub signal: u32,
    pub snr: u32,
    pub ber: u32,
    pub unc: u32,
}


impl DvbTune {
    pub fn new<T: Dvb>(dvb: &T) -> io::Result<DvbTune> {
        let mut x = DvbTune::default();
        x.fd = dvb.open()?;
        Ok(x)
    }

    #[inline]
    pub fn close(&mut self) {
        self.fd.close();
    }

    pub fn status(&mut self) -> io::Result<()> {
        frontend::read_status(self.fd.inner, &mut self.status)?;
        if self.status.contains(frontend::Status::FE_HAS_LOCK) {
            frontend::read_signal(self.fd.inner, &mut self.signal)?;
            frontend::read_snr(self.fd.inner, &mut self.snr)?;
            frontend::read_ber(self.fd.inner, &mut self.ber)?;
            frontend::read_unc(self.fd.inner, &mut self.unc)?;
        } else {
            self.signal = 0;
            self.snr = 0;
            self.ber = 0;
            self.unc = 0;
        }
        Ok(())
    }
}


impl Drop for DvbTune {
    fn drop(&mut self) {
        self.close();
    }
}
