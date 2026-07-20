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

        result.signal = normalize_signal_strength(status, &props[IDX_SIGNAL_STRENGTH].stats(), fe);

        result.cnr = normalize_snr(
            status,
            result.delivery_system,
            result.modulation,
            &props[IDX_SNR].stats(),
            fe,
        );

        result.ber = normalize_ber(status, &props[IDX_BER].stats(), fe);
        result.unc = normalize_unc(status, &props[IDX_UNC].stats(), fe);

        Ok(result)
    }
}

fn normalize_signal_strength(
    status: FeStatusFlags,
    stats: &DtvFrontendStats,
    fe: &FeDevice,
) -> FeLevel {
    let mut level = FeLevel::default();
    let mut decibel = None;

    for i in 0 .. usize::from(stats.len).min(stats.stat.len()) {
        let stat = stats.stat[i];
        match stat.scale {
            FE_SCALE_DECIBEL => decibel = Some(stat.value),
            FE_SCALE_RELATIVE => {
                level.relative = Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
            }
            _ => {}
        }
    }

    if let Some(value) = decibel {
        level.decibel = Some((value as f64) / 1000.0);

        // Calculates relative signal strength value
        if level.relative.is_none() && status.contains(FeStatusFlags::HAS_SIGNAL) {
            // TODO: check delivery_system

            const LO: i64 = -85000;
            const HI: i64 = -6000;

            let value = if value > HI {
                65535
            } else if value < LO {
                0
            } else {
                65535 * (LO - value) / (LO - HI)
            };

            level.relative = Some((value * 100 / 65535) as u32);
        }
    }

    if level.relative.is_none()
        && status.contains(FeStatusFlags::HAS_SIGNAL)
        && let Ok(value) = fe.read_signal_strength()
    {
        level.relative = Some(u32::from(value) * 100 / 65535);
    }

    level
}

fn normalize_snr(
    status: FeStatusFlags,
    delivery_system: DeliverySystem,
    modulation: Modulation,
    stats: &DtvFrontendStats,
    fe: &FeDevice,
) -> FeLevel {
    let mut level = FeLevel::default();
    let mut decibel = None;

    for i in 0 .. usize::from(stats.len).min(stats.stat.len()) {
        let stat = stats.stat[i];
        match stat.scale {
            FE_SCALE_DECIBEL => decibel = Some(stat.value),
            FE_SCALE_RELATIVE => {
                level.relative = Some(((stat.value & 0xFFFF) * 100 / 65535) as u32)
            }
            _ => {}
        }
    }

    if let Some(value) = decibel {
        level.decibel = Some((value as f64) / 1000.0);

        // Calculates relative SNR value
        if level.relative.is_none() && status.contains(FeStatusFlags::HAS_CARRIER) {
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

                _ => return level,
            };

            let value = if value >= hi {
                65535
            } else if value <= 0 {
                0
            } else {
                65535 * value / hi
            };

            level.relative = Some((value * 100 / 65535) as u32);
        }
    }

    if level.relative.is_none()
        && status.contains(FeStatusFlags::HAS_CARRIER)
        && let Ok(value) = fe.read_snr()
    {
        level.relative = Some(u32::from(value) * 100 / 65535);
    }

    level
}

/// Normalize BER value
fn normalize_ber(status: FeStatusFlags, stats: &DtvFrontendStats, fe: &FeDevice) -> Option<u32> {
    for i in 0 .. usize::from(stats.len).min(stats.stat.len()) {
        let stat = stats.stat[i];
        if stat.scale == FE_SCALE_COUNTER {
            return Some((stat.value & 0xFFFF_FFFF) as u32);
        }
    }

    if status.contains(FeStatusFlags::HAS_LOCK) {
        return fe.read_ber().ok();
    }

    None
}

/// Normalize UNC value
fn normalize_unc(status: FeStatusFlags, stats: &DtvFrontendStats, fe: &FeDevice) -> Option<u32> {
    for i in 0 .. usize::from(stats.len).min(stats.stat.len()) {
        let stat = stats.stat[i];
        if stat.scale == FE_SCALE_COUNTER {
            return Some((stat.value & 0xFFFF_FFFF) as u32);
        }
    }

    if status.contains(FeStatusFlags::HAS_LOCK) {
        return fe.read_unc().ok();
    }

    None
}
