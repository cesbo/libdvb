use crate::{
    error::Result,
    fe::{
        FeDevice,
        sys::*,
    },
};

const IDX_DELIVERY_SYSTEM: usize = 0;
const IDX_MODULATION: usize = 1;
const IDX_SIGNAL_STRENGTH: usize = 2;
const IDX_SNR: usize = 3;
const IDX_BER: usize = 4;
const IDX_UNC: usize = 5;

/// Level of the signal strength or carrier-to-noise ratio.
///
/// Contains both representations reported by the driver: an absolute
/// decibel value and a relative value in percent. Either may be missing
/// if the driver does not provide it and it cannot be derived.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FeLevel {
    /// Absolute level in dBm (signal strength) or dB (CNR)
    decibel: Option<f64>,
    /// Relative level in percent (0..=100)
    relative: Option<u32>,
}

impl FeLevel {
    /// Absolute level in dBm for the signal strength or dB for the CNR,
    /// `None` if not available
    pub fn decibel(&self) -> Option<f64> {
        self.decibel
    }

    /// Relative level in percent (0..=100), `None` if not available
    pub fn relative(&self) -> Option<u32> {
        self.relative
    }

    /// Returns `true` if neither the decibel nor the relative value is available
    pub fn is_empty(&self) -> bool {
        self.decibel.is_none() && self.relative.is_none()
    }
}

/// A consistent snapshot of the frontend statistics.
///
/// Returned by [`FeDevice::get_stats`] in a single call, so all values
/// are read from the same point in time.
#[derive(Debug, Clone, Copy)]
pub struct FeStats {
    status: FeStatusFlags,
    delivery_system: DeliverySystem,
    modulation: Modulation,
    signal: FeLevel,
    cnr: FeLevel,
    ber: Option<u32>,
    unc: Option<u32>,
}

impl Default for FeStats {
    fn default() -> FeStats {
        FeStats {
            status: FeStatusFlags::empty(),
            delivery_system: DeliverySystem::Undefined,
            modulation: Modulation::Qpsk,
            signal: FeLevel::default(),
            cnr: FeLevel::default(),
            ber: None,
            unc: None,
        }
    }
}

impl FeStats {
    /// Frontend status flags
    pub fn status(&self) -> FeStatusFlags {
        self.status
    }

    /// Returns `true` if the frontend has lock
    pub fn has_lock(&self) -> bool {
        self.status.contains(FeStatusFlags::HAS_LOCK)
    }

    /// Current delivery system
    pub fn delivery_system(&self) -> DeliverySystem {
        self.delivery_system
    }

    /// Current modulation
    pub fn modulation(&self) -> Modulation {
        self.modulation
    }

    /// Signal strength level
    pub fn signal(&self) -> FeLevel {
        self.signal
    }

    /// Carrier-to-noise ratio level
    pub fn cnr(&self) -> FeLevel {
        self.cnr
    }

    /// Bit error counter, `None` if not available
    pub fn ber(&self) -> Option<u32> {
        self.ber
    }

    /// Uncorrected blocks counter, `None` if not available
    pub fn unc(&self) -> Option<u32> {
        self.unc
    }

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
            result.push(format!("LOCK {}", self.delivery_system));
        } else {
            result.push(format!("NO-LOCK 0x{:02X}", self.status.bits()));
        };

        if self.status.contains(FeStatusFlags::HAS_SIGNAL) {
            result.push(format!(
                "Signal {:.02}dBm ({}%)",
                self.signal.decibel().unwrap_or(0.0),
                self.signal.relative().unwrap_or(0)
            ));
        }

        if self.status.contains(FeStatusFlags::HAS_CARRIER) {
            result.push(format!(
                "Quality {:.02}dB ({}%)",
                self.cnr.decibel().unwrap_or(0.0),
                self.cnr.relative().unwrap_or(0)
            ));
        }

        if self.status.contains(FeStatusFlags::HAS_LOCK) {
            let ber = self.ber.map(|v| v.to_string());
            result.push(format!("BER:{}", ber.as_deref().unwrap_or("-")));

            let unc = self.unc.map(|v| v.to_string());
            result.push(format!("UNC:{}", unc.as_deref().unwrap_or("-")));
        }

        result.join(" | ")
    }

    /// Reads frontend statistics with fallback to the DVBv3 API
    pub(crate) fn read(fe: &FeDevice) -> Result<FeStats> {
        let status = fe.read_status()?;

        let mut result = FeStats {
            status,
            ..FeStats::default()
        };

        if status.is_empty() {
            return Ok(result);
        }

        let mut props = [
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
        ];

        fe.get_properties(&mut props)?;

        result.delivery_system = DeliverySystem::try_from(props[IDX_DELIVERY_SYSTEM].data())
            .unwrap_or(DeliverySystem::Undefined);
        result.modulation =
            Modulation::try_from(props[IDX_MODULATION].data()).unwrap_or(Modulation::Qpsk);

        let stats = normalize_signal_strength(status, props[IDX_SIGNAL_STRENGTH].stats());
        result.signal = level_from_stats(&stats);

        let stats = normalize_snr(
            status,
            result.delivery_system,
            result.modulation,
            props[IDX_SNR].stats(),
        );
        result.cnr = level_from_stats(&stats);

        let stats = normalize_ber(status, props[IDX_BER].stats(), fe);
        result.ber = counter_from_stats(&stats);

        let stats = normalize_unc(status, props[IDX_UNC].stats(), fe);
        result.unc = counter_from_stats(&stats);

        Ok(result)
    }
}

/// Builds an [`FeLevel`] from a normalized stats pair: decibel in `stat[0]`,
/// relative in `stat[1]`.
fn level_from_stats(stats: &DtvFrontendStats) -> FeLevel {
    let mut level = FeLevel::default();

    for i in 0 .. usize::from(stats.len).min(stats.stat.len()) {
        let stat = stats.stat[i];
        match stat.scale {
            FE_SCALE_DECIBEL => level.decibel = Some((stat.value as f64) / 1000.0),
            FE_SCALE_RELATIVE => {
                level.relative = Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
            }
            _ => {}
        }
    }

    level
}

/// Extracts a counter value from normalized stats, `None` if not available.
fn counter_from_stats(stats: &DtvFrontendStats) -> Option<u32> {
    if stats.len == 0 || stats.stat[0].scale != FE_SCALE_COUNTER {
        return None;
    }

    Some((stats.stat[0].value & 0xFFFF_FFFF) as u32)
}

fn normalize_signal_strength(
    status: FeStatusFlags,
    mut stats: DtvFrontendStats,
) -> DtvFrontendStats {
    for i in usize::from(stats.len) .. 2 {
        stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
        stats.stat[i].value = 0;
    }

    stats.len = 2;

    if stats.stat[0].scale == FE_SCALE_RELATIVE {
        stats.stat.swap(0, 1);
        return stats;
    }

    if stats.stat[1].scale == FE_SCALE_RELATIVE || !status.contains(FeStatusFlags::HAS_SIGNAL) {
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

fn normalize_snr(
    status: FeStatusFlags,
    delivery_system: DeliverySystem,
    modulation: Modulation,
    mut stats: DtvFrontendStats,
) -> DtvFrontendStats {
    for i in usize::from(stats.len) .. 2 {
        stats.stat[i].scale = FE_SCALE_NOT_AVAILABLE;
        stats.stat[i].value = 0;
    }

    stats.len = 2;

    if stats.stat[0].scale == FE_SCALE_RELATIVE {
        stats.stat.swap(0, 1);
        return stats;
    }

    if stats.stat[1].scale == FE_SCALE_RELATIVE || !status.contains(FeStatusFlags::HAS_CARRIER) {
        return stats;
    }

    // Calculates relative SNR value
    if stats.stat[0].scale != FE_SCALE_DECIBEL {
        return stats;
    }

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
fn normalize_ber(
    status: FeStatusFlags,
    mut stats: DtvFrontendStats,
    fe: &FeDevice,
) -> DtvFrontendStats {
    if stats.len == 0 {
        stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
        stats.stat[0].value = 0;
        stats.len = 1;
    }

    if stats.stat[0].scale != FE_SCALE_COUNTER && status.contains(FeStatusFlags::HAS_LOCK) {
        stats.stat[0].scale = FE_SCALE_COUNTER;
        stats.stat[0].value = fe.read_ber().map(i64::from).unwrap_or(-1);
    }

    stats
}

/// Normalize UNC value
fn normalize_unc(
    status: FeStatusFlags,
    mut stats: DtvFrontendStats,
    fe: &FeDevice,
) -> DtvFrontendStats {
    if stats.len == 0 {
        stats.stat[0].scale = FE_SCALE_NOT_AVAILABLE;
        stats.stat[0].value = 0;
        stats.len = 1;
    }

    if stats.stat[0].scale != FE_SCALE_COUNTER && status.contains(FeStatusFlags::HAS_LOCK) {
        stats.stat[0].scale = FE_SCALE_COUNTER;
        stats.stat[0].value = fe.read_unc().map(i64::from).unwrap_or(-1);
    }

    stats
}
