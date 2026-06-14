use super::{
    FeDevice,
    sys::*,
};
use crate::error::Result;

/// Frontend status
#[derive(Debug)]
pub struct FeStatus {
    /// `sys::frontend::fe_status`
    status: FeStatusFlags,

    /// properties
    props: [DtvPropertyRaw; 6],
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
            status: FeStatusFlags::empty(),
            props: [
                // delivery system
                DtvPropertyRaw::new(DTV_DELIVERY_SYSTEM, 0),
                // modulation
                DtvPropertyRaw::new(DTV_MODULATION, Modulation::Qpsk as u32),
                // signal level
                DtvPropertyRaw::new(DTV_STAT_SIGNAL_STRENGTH, 0),
                // signal-to-noise ratio
                DtvPropertyRaw::new(DTV_STAT_CNR, 0),
                // ber - number of bit errors
                DtvPropertyRaw::new(DTV_STAT_PRE_ERROR_BIT_COUNT, 0),
                // unc - number of block errors
                DtvPropertyRaw::new(DTV_STAT_ERROR_BLOCK_COUNT, 0),
            ],
        }
    }
}

impl FeStatus {
    /// Returns frontend status summary line.
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
    pub fn to_status_string(&self) -> String {
        if self.status.is_empty() {
            return "OFF".to_string();
        }

        let mut result = Vec::new();

        if self.status.contains(FeStatusFlags::HAS_LOCK) {
            result.push(format!("LOCK {}", self.delivery_system()));
        } else {
            result.push(format!("NO-LOCK 0x{:02X}", self.status.bits()));
        };

        if self.status.contains(FeStatusFlags::HAS_SIGNAL) {
            result.push(format!(
                "Signal {:.02}dBm ({}%)",
                self.signal_strength_decibel().unwrap_or(0.0),
                self.signal_strength().unwrap_or(0)
            ));
        }

        if self.status.contains(FeStatusFlags::HAS_CARRIER) {
            result.push(format!(
                "Quality {:.02}dB ({}%)",
                self.snr_decibel().unwrap_or(0.0),
                self.snr().unwrap_or(0)
            ));
        }

        if self.status.contains(FeStatusFlags::HAS_LOCK) {
            let ber = self.ber().map(|v| v.to_string());
            result.push(format!("BER:{}", ber.as_deref().unwrap_or("-")));

            let unc = self.unc().map(|v| v.to_string());
            result.push(format!("UNC:{}", unc.as_deref().unwrap_or("-")));
        }

        result.join(" | ")
    }

    /// Returns current delivery system
    pub fn delivery_system(&self) -> DeliverySystem {
        let v = self.props[IDX_DELIVERY_SYSTEM].data();
        DeliverySystem::try_from(v).unwrap_or(DeliverySystem::Undefined)
    }

    /// Returns current modulation
    pub fn modulation(&self) -> Modulation {
        let v = self.props[IDX_MODULATION].data();
        Modulation::try_from(v).unwrap_or(Modulation::Qpsk)
    }

    /// Returns Signal Strength in dBm
    pub fn signal_strength_decibel(&self) -> Option<f64> {
        let stat = self.props[IDX_SIGNAL_STRENGTH].stat(0)?;
        if stat.scale == FE_SCALE_DECIBEL {
            Some((stat.value as f64) / 1000.0)
        } else {
            None
        }
    }

    /// Returns Signal Strength in percentage
    pub fn signal_strength(&self) -> Option<u32> {
        let stat = self.props[IDX_SIGNAL_STRENGTH].stat(1)?;
        if stat.scale == FE_SCALE_RELATIVE {
            Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
        } else {
            None
        }
    }

    /// Returns Signal to noise ratio in dB
    pub fn snr_decibel(&self) -> Option<f64> {
        let stat = self.props[IDX_SNR].stat(0)?;
        if stat.scale == FE_SCALE_DECIBEL {
            Some((stat.value as f64) / 1000.0)
        } else {
            None
        }
    }

    /// Returns Signal Strength in percentage
    pub fn snr(&self) -> Option<u32> {
        let stat = self.props[IDX_SNR].stat(1)?;
        if stat.scale == FE_SCALE_RELATIVE {
            Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
        } else {
            None
        }
    }

    /// Returns BER value if available
    pub fn ber(&self) -> Option<u32> {
        let stat = self.props[IDX_BER].stat(0)?;
        if stat.scale == FE_SCALE_COUNTER {
            Some((stat.value & 0xFFFF_FFFF) as u32)
        } else {
            None
        }
    }

    /// Returns UNC value if available
    pub fn unc(&self) -> Option<u32> {
        let stat = self.props[IDX_UNC].stat(0)?;
        if stat.scale == FE_SCALE_COUNTER {
            Some((stat.value & 0xFFFF_FFFF) as u32)
        } else {
            None
        }
    }

    fn normalize_signal_strength(&self, mut stats: DtvFrontendStats) -> DtvFrontendStats {
        for i in usize::from(stats.len) .. 2 {
            stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[i].value = 0;
        }

        stats.len = 2;

        if stats.stat[0].scale == FE_SCALE_RELATIVE {
            stats.stat.swap(0, 1);
            return stats;
        }

        if stats.stat[1].scale == FE_SCALE_RELATIVE
            || !self.status.contains(FeStatusFlags::HAS_SIGNAL)
        {
            return stats;
        }

        // Calculates relative signal strength value
        if stats.stat[0].scale != FE_SCALE_DECIBEL {
            return stats;
        }

        // TODO: check delivery_system

        let lo: i64 = -85000;
        let hi: i64 = -6000;

        stats.stat[1].scale = FE_SCALE_RELATIVE;
        stats.stat[1].value = {
            if stats.stat[0].value > hi {
                65535
            } else if stats.stat[0].value < lo {
                0
            } else {
                65535 * (lo - stats.stat[0].value) / (lo - hi)
            }
        };

        stats
    }

    fn normalize_snr(&self, mut stats: DtvFrontendStats) -> DtvFrontendStats {
        for i in usize::from(stats.len) .. 2 {
            stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[i].value = 0;
        }

        stats.len = 2;

        if stats.stat[0].scale == FE_SCALE_RELATIVE {
            stats.stat.swap(0, 1);
            return stats;
        }

        if stats.stat[1].scale == FE_SCALE_RELATIVE
            || !self.status.contains(FeStatusFlags::HAS_CARRIER)
        {
            return stats;
        }

        // Calculates relative SNR value
        if stats.stat[0].scale != FE_SCALE_DECIBEL {
            return stats;
        }

        let delivery_system = self.delivery_system();
        let modulation = self.modulation();
        let hi = match delivery_system {
            DeliverySystem::Dvbs | DeliverySystem::Dvbs2 => 15000,

            DeliverySystem::DvbcAnnexA
            | DeliverySystem::DvbcAnnexB
            | DeliverySystem::DvbcAnnexC
            | DeliverySystem::Dvbc2 => 28000,

            DeliverySystem::Dvbt | DeliverySystem::Dvbt2 => 19000,

            DeliverySystem::Atsc => match modulation {
                Modulation::Vsb8 | Modulation::Vsb16 => 19000,
                _ => 28000,
            },

            _ => return stats,
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

        stats
    }

    /// Normalize BER value
    fn normalize_ber(&self, mut stats: DtvFrontendStats, fe: &FeDevice) -> DtvFrontendStats {
        if stats.len == 0 {
            stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[0].value = 0;
            stats.len = 1;
        }

        if stats.stat[0].scale != FE_SCALE_COUNTER && self.status.contains(FeStatusFlags::HAS_LOCK)
        {
            stats.stat[0].scale = FE_SCALE_COUNTER;
            stats.stat[0].value = fe.read_ber().map(i64::from).unwrap_or(-1);
        }

        stats
    }

    /// Normalize UNC value
    fn normalize_unc(&self, mut stats: DtvFrontendStats, fe: &FeDevice) -> DtvFrontendStats {
        if stats.len == 0 {
            stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
            stats.stat[0].value = 0;
            stats.len = 1;
        }

        if stats.stat[0].scale != FE_SCALE_COUNTER && self.status.contains(FeStatusFlags::HAS_LOCK)
        {
            stats.stat[0].scale = FE_SCALE_COUNTER;
            stats.stat[0].value = fe.read_unc().map(i64::from).unwrap_or(-1);
        }

        stats
    }

    /// set decibel to `stat[0]` and relative to `stat[1]` and fallback to DVBv3 API
    fn normalize_props(&mut self, fe: &FeDevice) -> Result<()> {
        let stats = self.normalize_signal_strength(self.props[IDX_SIGNAL_STRENGTH].stats());
        self.props[IDX_SIGNAL_STRENGTH].set_stats(stats);

        let stats = self.normalize_snr(self.props[IDX_SNR].stats());
        self.props[IDX_SNR].set_stats(stats);

        let stats = self.normalize_ber(self.props[IDX_BER].stats(), fe);
        self.props[IDX_BER].set_stats(stats);

        let stats = self.normalize_unc(self.props[IDX_UNC].stats(), fe);
        self.props[IDX_UNC].set_stats(stats);

        Ok(())
    }

    /// Reads frontend status with fallback to DVBv3 API
    pub fn read(&mut self, fe: &FeDevice) -> Result<()> {
        self.status = fe.read_status()?;

        if self.status.is_empty() {
            return Ok(());
        }

        fe.get_properties(&mut self.props)?;
        self.normalize_props(fe)
    }
}
