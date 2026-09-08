//! UAPI of the TBS modulator device (`/dev/tbsmodN/modM`) as implemented by
//! the tbsmod driver: the `MODULATOR_*` DTV property codes of its `mod.h`,
//! carried through the standard `FE_SET_PROPERTY`, and the
//! `DVBMOD_SET_PARAMETERS` ioctl of its `dvbmod.h` for DVB-T.

use crate::{
    fe::sys::{
        Fec,
        GuardInterval,
        Modulation,
        TransmitMode,
    },
    modulator::{
        DvbtBandwidth,
        DvbtCodeRate,
        DvbtConstellation,
        DvbtGuard,
    },
};

// DTV property commands of the tbsmod driver, all with a `u.data` payload
// and exactly one per FE_SET_PROPERTY call (a longer list is EINVAL).
// Card-level commands are honored on channel 0 only (channel 4 as well on
// the 6008, which carries a second DAC) and silently ignored on the other
// channels. Codes not handled by the driver are rejected with EINVAL.

/// Base frequency in Hz.
/// - 6004 channel n sits at `base + n * bandwidth` (see [`MODULATOR_BANDWIDTH`])
/// - 6014 (J.83B) and 6034 (ATSC) tune their own way
/// - Other cards log "not this interface" and ignore it
pub const MODULATOR_FREQUENCY: u32 = 3;

/// `fe_modulation` value in `QAM_16`..`QAM_256`, anything else is EPERM.
/// - 6004/6008 program all five
/// - the 6014 programs only `QAM_64` and `QAM_256` and ignores the others.
pub const MODULATOR_MODULATION: u32 = 4;

/// Symbol rate in Hz, at least 1 MSym/s - a lower value is ignored and the
/// ioctl returns a positive EINVAL, which looks like success. Writes AD9789
/// registers without checking the card, so it must never reach a 6032/6001.
pub const MODULATOR_SYMBOL_RATE: u32 = 5;

/// Per-channel, every card. DMA drain pace of the channel FIFO: a value up
/// to 210 is in units of 2^20 bit/s, the driver's "Mbit" (at most 200), a
/// larger value in units of 2^10 bit/s (at most 200 * 1024); out of range
/// is EPERM. The driver header says Hz; the code does not. Takes effect
/// when the first write starts the DMA.
pub const MODULATOR_INPUT_BITRATE: u32 = 33;

/// Card-level. AD9789 channel gain 0..=120, anything above is EPERM.
/// Written to all four channel gain registers without checking the card, so
/// it must never reach a 6032/6001.
pub const MODULATOR_GAIN: u32 = 35;

/// Channel 0 only (not 4). J.83B interleave mode 0..=16 on the 6014,
/// anything above is EPERM; range-checked but ignored on other cards.
pub const MODULATOR_INTERLEAVE: u32 = 65;

/// Card-level. Channel spacing in MHz used by [`MODULATOR_FREQUENCY`] on
/// the 6004/6008; 8 at probe. DVB-T bandwidth goes through
/// [`dvbmod_set_parameters`] instead.
pub const MODULATOR_BANDWIDTH: u32 = 68;

// Card ids: the PCI subsystem vendor, also reported by FE_GET_INFO as
// "TBS-<id hex>:<card index>".

/// DVB-C, 4 channels (AD9789).
pub const CARD_6004: u16 = 0x6004;

/// ASI, 1/2/4 channels.
pub const CARD_690B: u16 = 0x690b;

/// DVB-T, 4 channels.
pub const CARD_6104: u16 = 0x6104;

/// J.83 Annex B, 4 channels.
pub const CARD_6014: u16 = 0x6014;

/// DVB-C, 8 channels over two AD9789 (channels 0-3 and 4-7).
pub const CARD_6008: u16 = 0x6008;

/// ISDB-T, 4 channels.
pub const CARD_6214: u16 = 0x6214;

/// ATSC, 4 channels.
pub const CARD_6034: u16 = 0x6034;

/// DVB-C, 8/16/24/32 channels (MAX5862, configured by the vendor SPI tool).
pub const CARD_6032: u16 = 0x6032;

/// DVB-C, 1 channel (configured by the vendor SPI tool).
pub const CARD_6001: u16 = 0x6001;

/// `struct dvb_modulator_parameters` - the `DVBMOD_SET_PARAMETERS` argument.
/// An enum field outside the accepted set falls back to the noted default
/// without an error.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DvbModulatorParameters {
    /// Channel frequency in kHz.
    pub frequency_khz: u32,
    /// `fe_transmit_mode`: 2K or 8K, default 8K.
    pub transmission_mode: u32,
    /// `fe_modulation`: QPSK, QAM_16 or QAM_64, default QAM_64.
    pub constellation: u32,
    /// `fe_guard_interval`: 1/4..1/32, default 1/32.
    pub guard_interval: u32,
    /// `fe_code_rate`: 1/2..7/8, default 7/8.
    pub code_rate_hp: u32,
    /// Bandwidth in kHz. The C field is `bandwidth_hz`, but the driver
    /// divides it by 1000 to get MHz, and a `u16` cannot hold a Hz value.
    pub bandwidth_khz: u16,
    pub cell_id: u16,
}

impl DvbModulatorParameters {
    /// DVB-T parameters mapped onto the kernel `fe_*` values.
    pub fn dvbt(
        frequency_khz: u32,
        bandwidth: DvbtBandwidth,
        mode: TransmitMode,
        constellation: DvbtConstellation,
        code_rate: DvbtCodeRate,
        guard: DvbtGuard,
        cell_id: u16,
    ) -> Self {
        let constellation = match constellation {
            DvbtConstellation::Qpsk => Modulation::Qpsk,
            DvbtConstellation::Qam16 => Modulation::Qam16,
            DvbtConstellation::Qam64 => Modulation::Qam64,
        };
        let code_rate = match code_rate {
            DvbtCodeRate::Cr1_2 => Fec::Fec1_2,
            DvbtCodeRate::Cr2_3 => Fec::Fec2_3,
            DvbtCodeRate::Cr3_4 => Fec::Fec3_4,
            DvbtCodeRate::Cr5_6 => Fec::Fec5_6,
            DvbtCodeRate::Cr7_8 => Fec::Fec7_8,
        };
        let guard = match guard {
            DvbtGuard::G1_32 => GuardInterval::Gi1_32,
            DvbtGuard::G1_16 => GuardInterval::Gi1_16,
            DvbtGuard::G1_8 => GuardInterval::Gi1_8,
            DvbtGuard::G1_4 => GuardInterval::Gi1_4,
        };

        DvbModulatorParameters {
            frequency_khz,
            transmission_mode: mode as u32,
            constellation: constellation.as_u32(),
            guard_interval: guard as u32,
            code_rate_hp: code_rate.as_u32(),
            bandwidth_khz: (bandwidth.hz() / 1000) as u16,
            cell_id,
        }
    }
}

// DVBMOD_SET_PARAMETERS
nix::ioctl_write_ptr!(
    /// DVB-T setup ioctl (6104). Honored on channel 0 only; a no-op on the
    /// other channels.
    #[inline]
    dvbmod_set_parameters,
    b'k',
    0x40,
    DvbModulatorParameters
);

#[cfg(test)]
mod tests {
    use std::mem::{
        offset_of,
        size_of,
    };

    use super::*;

    #[test]
    fn parameters_layout() {
        assert_eq!(size_of::<DvbModulatorParameters>(), 24);
        assert_eq!(offset_of!(DvbModulatorParameters, bandwidth_khz), 20);
        assert_eq!(offset_of!(DvbModulatorParameters, cell_id), 22);
    }

    #[test]
    fn dvbt_mapping() {
        let p = DvbModulatorParameters::dvbt(
            474_000,
            DvbtBandwidth::Mhz8,
            TransmitMode::Tm8K,
            DvbtConstellation::Qam64,
            DvbtCodeRate::Cr7_8,
            DvbtGuard::G1_32,
            0,
        );
        assert_eq!(
            p,
            DvbModulatorParameters {
                frequency_khz: 474_000,
                transmission_mode: 1,
                constellation: 3,
                guard_interval: 0,
                code_rate_hp: 7,
                bandwidth_khz: 8000,
                cell_id: 0,
            }
        );
    }
}
