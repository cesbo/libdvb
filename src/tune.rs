use frontend;

use std::io;
use std::os::unix::io::RawFd;

/// Adapter
pub struct Adapter {
    /// Adapter number /dev/dvb/adapterX
    pub adapter: usize,
    /// Device number /dev/dvb/adapterX/frontendX
    pub device: usize,
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
    feinfo: frontend::Info,
}

impl DvbTune {
    pub fn new(options: &DvbOptions) -> io::Result<DvbTune> {
        match options {
            DvbOptions::DVB_S2(v) => {
                let fd = frontend::open(v.adapter.adapter, v.adapter.device)?;

                let mut feinfo = frontend::Info::new();
                feinfo.read(fd)?;

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
            frontend::clear(self.fd).unwrap();
            frontend::close(self.fd);
            self.fd = 0;
        }
    }
}
