use {
    std::{
        fmt,
    },

    crate::error::Result,

    super::{
        FeDevice,
        sys::*,
    },
};


/// Frontend status
#[derive(Debug)]
pub struct FeStatus {
    /// `sys::frontend::fe_status`
    status: u32,

    /// properties
    props: [DtvProperty; 6],
}


const IDX_DELIVERY_SYSTEM: usize = 0;
const IDX_MODULATION: usize = 1;
const IDX_SIGNAL_STRENGTH: usize = 2;
const IDX_SNR: usize = 3;
const IDX_BER: usize = 4;
const IDX_UNC: usize = 5;


impl Default for FeStatus {
    fn default() -> FeStatus {
        FeStatus {
            status: 0,
            props: [
                // delivery system
                DtvProperty::new(DTV_DELIVERY_SYSTEM, FE_NONE),
                // modulation
                DtvProperty::new(DTV_MODULATION, QPSK),
                // signal level
                DtvProperty::new(DTV_STAT_SIGNAL_STRENGTH, 0),
                // signal-to-noise ratio
                DtvProperty::new(DTV_STAT_CNR, 0),
                // ber - number of bit errors
                DtvProperty::new(DTV_STAT_PRE_ERROR_BIT_COUNT, 0),
                // unc - number of block errors
                DtvProperty::new(DTV_STAT_ERROR_BLOCK_COUNT, 0),
            ],
        }
    }
}


/// Returns an object that implements `Display` for different verbosity levels
///
/// Tuner is turned off:
///
/// ```text
/// OFF
/// ```
///
/// Tuner acquiring signal but has no lock:
///
/// ```text
/// NO-LOCK 0x01 | Signal -38.56dBm (59%)
/// NO-LOCK 0x03 | Signal -38.56dBm (59%) | Quality 5.32dB (25%)
/// ```
///
/// Hex number after `NO-LOCK` this is tuner status bit flags:
/// - 0x01 - has signal
/// - 0x02 - has carrier
/// - 0x04 - has viterbi
/// - 0x08 - has sync
/// - 0x10 - has lock
/// - 0x20 - timed-out
/// - 0x40 - re-init
///
/// Tuner has lock
///
/// ```text
/// LOCK dvb-s2 | Signal -38.56dBm (59%) | Quality 14.57dB (70%) | BER:0 | UNC:0
/// ```
impl fmt::Display for FeStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.status == FE_NONE {
            write!(f, "OFF")?;
            return Ok(());
        }

        if self.status & FE_HAS_LOCK != 0 {
            write!(f, "LOCK {}", DeliverySystemDisplay(self.get_delivery_system()))?;
        } else {
            write!(f, "NO-LOCK 0x{:02X}", self.status)?;
        }

        if self.status & FE_HAS_SIGNAL == 0 {
            return Ok(());
        }

        write!(
            f,
            " | Signal {:.02}dBm ({}%)",
            self.get_signal_strength_decibel().unwrap_or(0.0),
            self.get_signal_strength().unwrap_or(0)
        )?;

        if self.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(
            f,
            " | Quality {:.02}dB ({}%)",
            self.get_snr_decibel().unwrap_or(0.0),
            self.get_snr().unwrap_or(0)
        )?;

        if self.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, " | BER:")?;
        if let Some(ber) = self.get_ber() {
            write!(f, "{}", ber)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, " | UNC:")?;
        if let Some(unc) = self.get_unc() {
            write!(f, "{}", unc)?;
        } else {
            write!(f, "-")?;
        }

        Ok(())
    }
}


impl FeStatus {
    /// Returns current delivery system
    #[inline]
    pub fn get_delivery_system(&self) -> u32 { unsafe { self.props[IDX_DELIVERY_SYSTEM].u.data } }

    /// Returns current modulation
    #[inline]
    pub fn get_modulation(&self) -> u32 { unsafe { self.props[IDX_MODULATION].u.data } }

    /// Returns Signal Strength in dBm
    pub fn get_signal_strength_decibel(&self) -> Option<f64> {
        let stat = unsafe { &self.props[IDX_SIGNAL_STRENGTH].u.st.stat[0] };
        if stat.scale == FE_SCALE_DECIBEL {
            Some((stat.value as f64) / 1000.0)
        } else {
            None
        }
    }

    /// Returns Signal Strength in percentage
    pub fn get_signal_strength(&self) -> Option<u32> {
        let stat = unsafe { &self.props[IDX_SIGNAL_STRENGTH].u.st.stat[1] };
        if stat.scale == FE_SCALE_RELATIVE {
            Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
        } else {
            None
        }
    }

    /// Returns Signal to noise ratio in dB
    pub fn get_snr_decibel(&self) -> Option<f64> {
        let stat = unsafe { &self.props[IDX_SNR].u.st.stat[0] };
        if stat.scale == FE_SCALE_DECIBEL {
            Some((stat.value as f64) / 1000.0)
        } else {
            None
        }
    }

    /// Returns Signal Strength in percentage
    pub fn get_snr(&self) -> Option<u32> {
        let stat = unsafe { &self.props[IDX_SNR].u.st.stat[1] };
        if stat.scale == FE_SCALE_RELATIVE {
            Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
        } else {
            None
        }
    }

    /// Returns BER value if available
    pub fn get_ber(&self) -> Option<u32> {
        let stat = unsafe { &self.props[IDX_BER].u.st.stat[0] };
        if stat.scale == FE_SCALE_COUNTER {
            Some((stat.value & 0xFFFF_FFFF) as u32)
        } else {
            None
        }
    }

    /// Returns UNC value if available
    pub fn get_unc(&self) -> Option<u32> {
        let stat = unsafe { &self.props[IDX_UNC].u.st.stat[0] };
        if stat.scale == FE_SCALE_COUNTER {
            Some((stat.value & 0xFFFF_FFFF) as u32)
        } else {
            None
        }
    }

    fn normalize_signal_strength(&mut self) -> Result<()> {
        let stats = unsafe { &mut self.props[IDX_SIGNAL_STRENGTH].u.st };

        for i in usize::from(stats.len) .. 2 {
            stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[i].value = 0;
        }

        stats.len = 2;

        if stats.stat[0].scale == FE_SCALE_RELATIVE {
            stats.stat.swap(0, 1);
            return Ok(())
        }

        if stats.stat[1].scale == FE_SCALE_RELATIVE || (self.status & FE_HAS_SIGNAL) == 0 {
            return Ok(())
        }

        // calculate relative value

        if stats.stat[0].scale == FE_SCALE_DECIBEL {
            // TODO: check delivery_system

            let lo: i64 = -85000;
            let hi: i64 = -6000;

            stats.stat[1].scale = FE_SCALE_RELATIVE;
            stats.stat[1].value = {
                if stats.stat[0].value > hi {
                    65545
                } else if stats.stat[0].value < lo {
                    0
                } else {
                    65545 * (lo - stats.stat[0].value) / (lo - hi)
                }
            };
        }

        Ok(())
    }

    fn normalize_snr(&mut self) -> Result<()> {
        let delivery_system = self.get_delivery_system();
        let modulation = self.get_modulation();

        let stats = unsafe { &mut self.props[IDX_SNR].u.st };

        for i in usize::from(stats.len) .. 2 {
            stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[i].value = 0;
        }

        stats.len = 2;

        if stats.stat[0].scale == FE_SCALE_RELATIVE {
            stats.stat.swap(0, 1);
            return Ok(())
        }

        if stats.stat[1].scale == FE_SCALE_RELATIVE || (self.status & FE_HAS_CARRIER) == 0 {
            return Ok(())
        }

        // calculate relative value

        if stats.stat[0].scale == FE_SCALE_DECIBEL {
            let hi = match delivery_system {
                SYS_DVBS |
                SYS_DVBS2 => 15000,

                SYS_DVBC_ANNEX_A |
                SYS_DVBC_ANNEX_B |
                SYS_DVBC_ANNEX_C |
                SYS_DVBC2 => 28000,

                SYS_DVBT |
                SYS_DVBT2 => 19000,

                SYS_ATSC => {
                    match modulation {
                        VSB_8 | VSB_16 => 19000,
                        _ => 28000,
                    }
                }

                _ => return Ok(()),
            };

            stats.stat[1].scale = FE_SCALE_RELATIVE;
            stats.stat[1].value = {
                if stats.stat[0].value >= hi {
                    65535
                } else if stats.stat[0].value <= 0 {
                    0
                } else {
                    65535 * stats.stat[0].value / hi
                }
            };
        }

        Ok(())
    }

    fn normalize_ber(&mut self, fe: &FeDevice) -> Result<()> {
        let stats = unsafe { &mut self.props[IDX_BER].u.st };

        if stats.len == 0 {
            stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[0].value = 0;
            stats.len = 1;
        }

        if stats.stat[0].scale == FE_SCALE_COUNTER || (self.status & FE_HAS_LOCK) == 0 {
            return Ok(())
        }

        if let Ok(value) = fe.read_ber() {
            stats.stat[0].scale = FE_SCALE_COUNTER;
            stats.stat[0].value = i64::from(value);
        }

        Ok(())
    }

    fn normalize_unc(&mut self, fe: &FeDevice) -> Result<()> {
        let stats = unsafe { &mut self.props[IDX_UNC].u.st };

        if stats.len == 0 {
            stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[0].value = 0;
            stats.len = 1;
        }

        if stats.stat[0].scale == FE_SCALE_COUNTER || (self.status & FE_HAS_LOCK) == 0 {
            return Ok(())
        }

        if let Ok(value) = fe.read_unc() {
            stats.stat[0].scale = FE_SCALE_COUNTER;
            stats.stat[0].value = i64::from(value);
        }

        Ok(())
    }

    /// set decibel to `stat[0]` and relative to `stat[1]` and fallback to DVBv3 API
    fn normalize_props(&mut self, fe: &FeDevice) -> Result<()> {
        self.normalize_signal_strength()?;
        self.normalize_snr()?;
        self.normalize_ber(fe)?;
        self.normalize_unc(fe)?;

        Ok(())
    }

    /// Reads frontend status with fallback to DVBv3 API
    pub fn read(&mut self, fe: &FeDevice) -> Result<()> {
        self.status = fe.read_status()?;

        if self.status == FE_NONE {
            return Ok(());
        }

        fe.get_properties(&mut self.props)?;
        self.normalize_props(fe)
    }
}
