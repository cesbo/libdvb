//! DVB modulator (transmit) support.
//!
//! This module holds the vendor-neutral pieces: the DVB-T channel parameter
//! types and the net TS bitrate math ([`dvbc_ts_bitrate`],
//! [`dvbt_ts_bitrate`]). Device access is vendor-specific - every modulator
//! family exposes its own UAPI - and lives in a submodule per vendor:
//!
//! * [`dd`] - DigitalDevices (Octopus MOD / RESI / SDR cards).
//!
//! Vendor UAPIs share ancestry but disagree: the same numeric property code
//! can carry different meanings (or payload types) per vendor, so no
//! cross-vendor device constants exist at this level on purpose.

pub mod dd;

use crate::{
    error::{
        Error,
        Result,
    },
    fe::sys::Modulation,
};

/// Number of 204-byte wire bytes per 188-byte TS packet: every DVB outer
/// coding appends 16 Reed-Solomon bytes, so net TS rate = gross rate * 188/204.
const RS_NET: u128 = 188;
const RS_GROSS: u128 = 204;

/// DVB-T channel bandwidth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvbtBandwidth {
    Mhz6,
    Mhz7,
    Mhz8,
}

impl DvbtBandwidth {
    pub fn hz(self) -> u32 {
        match self {
            DvbtBandwidth::Mhz6 => 6_000_000,
            DvbtBandwidth::Mhz7 => 7_000_000,
            DvbtBandwidth::Mhz8 => 8_000_000,
        }
    }
}

/// DVB-T constellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvbtConstellation {
    Qpsk,
    Qam16,
    Qam64,
}

impl DvbtConstellation {
    /// Bits per constellation symbol.
    pub fn bits(self) -> u32 {
        match self {
            DvbtConstellation::Qpsk => 2,
            DvbtConstellation::Qam16 => 4,
            DvbtConstellation::Qam64 => 6,
        }
    }
}

/// DVB-T convolutional code rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvbtCodeRate {
    Cr1_2,
    Cr2_3,
    Cr3_4,
    Cr5_6,
    Cr7_8,
}

impl DvbtCodeRate {
    /// `(numerator, denominator)` of the code rate.
    pub fn fraction(self) -> (u32, u32) {
        match self {
            DvbtCodeRate::Cr1_2 => (1, 2),
            DvbtCodeRate::Cr2_3 => (2, 3),
            DvbtCodeRate::Cr3_4 => (3, 4),
            DvbtCodeRate::Cr5_6 => (5, 6),
            DvbtCodeRate::Cr7_8 => (7, 8),
        }
    }
}

/// DVB-T guard interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvbtGuard {
    G1_32,
    G1_16,
    G1_8,
    G1_4,
}

impl DvbtGuard {
    /// The guard interval as a fraction of the useful symbol: `1/divider`.
    pub fn divider(self) -> u32 {
        match self {
            DvbtGuard::G1_32 => 32,
            DvbtGuard::G1_16 => 16,
            DvbtGuard::G1_8 => 8,
            DvbtGuard::G1_4 => 4,
        }
    }
}

/// Net TS bitrate of a DVB-C channel in bit/s:
/// `symbol_rate * bits_per_symbol * 188/204`.
///
/// `modulation` must be one of QAM_16..QAM_256.
pub fn dvbc_ts_bitrate(symbol_rate: u32, modulation: Modulation) -> Result<u64> {
    let bits: u128 = match modulation {
        Modulation::Qam16 => 4,
        Modulation::Qam32 => 5,
        Modulation::Qam64 => 6,
        Modulation::Qam128 => 7,
        Modulation::Qam256 => 8,
        _ => {
            return Err(Error::InvalidData(format!(
                "DVB-C modulation must be QAM_16..QAM_256, got {:?}",
                modulation
            )));
        }
    };

    Ok((symbol_rate as u128 * bits * RS_NET / RS_GROSS) as u64)
}

/// Net TS bitrate of a DVB-T channel in bit/s, per ETSI EN 300 744:
///
/// `R = 6048 * b * CR * (188/204) * (D/(D+1)) / 896us`, scaled by
/// `bandwidth / 8 MHz` (6048 data carriers x b bits over a 896 us useful
/// symbol in the 8 MHz raster; the mode - 2K or 8K - cancels out).
///
/// 8 MHz, 64-QAM, 7/8, 1/32 gives 31_668_449 bit/s.
pub fn dvbt_ts_bitrate(
    bandwidth: DvbtBandwidth,
    constellation: DvbtConstellation,
    code_rate: DvbtCodeRate,
    guard: DvbtGuard,
) -> u64 {
    let b = constellation.bits() as u128;
    let (cn, cd) = code_rate.fraction();
    let d = guard.divider() as u128;

    let num = 6048 * b * cn as u128 * RS_NET * d * bandwidth.hz() as u128;
    let den = cd as u128 * RS_GROSS * (d + 1) * 896 * 8;
    (num / den) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvbc_bitrates() {
        // QAM256 @ 6.9 MSym/s - the ddbridge driver's own DEFAULT_BIT_RATE_C
        assert_eq!(
            dvbc_ts_bitrate(6_900_000, Modulation::Qam256).unwrap(),
            50_870_588
        );
        // QAM64 @ 6.9 MSym/s
        assert_eq!(
            dvbc_ts_bitrate(6_900_000, Modulation::Qam64).unwrap(),
            38_152_941
        );
        // QAM64 @ 6.875 MSym/s
        assert_eq!(
            dvbc_ts_bitrate(6_875_000, Modulation::Qam64).unwrap(),
            38_014_705
        );

        assert!(dvbc_ts_bitrate(6_900_000, Modulation::Qpsk).is_err());
        assert!(dvbc_ts_bitrate(6_900_000, Modulation::QamAuto).is_err());
    }

    #[test]
    fn dvbt_bitrates() {
        use DvbtBandwidth::*;
        use DvbtCodeRate::*;
        use DvbtConstellation::*;
        use DvbtGuard::*;

        // the ddbridge driver's own DEFAULT_BIT_RATE_T
        assert_eq!(dvbt_ts_bitrate(Mhz8, Qam64, Cr7_8, G1_32), 31_668_449);
        // EN 300 744 table A.1 spot checks (8 MHz); the table rounds to the
        // nearest bit/s while this function floors the exact fraction
        assert_eq!(dvbt_ts_bitrate(Mhz8, Qpsk, Cr1_2, G1_4), 4_976_470);
        assert_eq!(dvbt_ts_bitrate(Mhz8, Qam16, Cr2_3, G1_8), 14_745_098);
        assert_eq!(dvbt_ts_bitrate(Mhz8, Qam64, Cr3_4, G1_16), 26_346_020);
        assert_eq!(dvbt_ts_bitrate(Mhz8, Qam64, Cr2_3, G1_4), 19_905_882);
        // narrower rasters scale by bandwidth (floor of the exact fraction,
        // not of the already-floored 8 MHz value)
        assert_eq!(dvbt_ts_bitrate(Mhz7, Qam64, Cr7_8, G1_32), 27_709_893);
        assert_eq!(dvbt_ts_bitrate(Mhz6, Qam64, Cr7_8, G1_32), 23_751_336);
    }
}
