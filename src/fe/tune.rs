use super::{
    DtvProperty,
    sys::{
        DeliverySystem,
        Fec,
        GuardInterval,
        Hierarchy,
        Inversion,
        Modulation,
        Pilot,
        Rolloff,
        SecTone,
        SecVoltage,
        TransmitMode,
    },
};

/// DVB-S tune parameters.
///
/// `frequency_khz` is the intermediate frequency in kHz: the transponder
/// frequency minus the LNB local oscillator frequency. Use
/// [`FeDevice::use_diseqc`](super::FeDevice::use_diseqc) to drive the
/// SEC/DiSEqC switch and obtain this value from the transponder frequency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvbSTune {
    /// Intermediate frequency in kHz
    pub frequency_khz: u32,
    /// Symbol rate in baud
    pub symbolrate: u32,
    /// LNB voltage (polarization selection)
    pub voltage: SecVoltage,
    /// 22 kHz tone (band selection)
    pub tone: SecTone,
    /// Inner FEC code rate
    pub fec: Fec,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for DvbSTune {
    fn default() -> Self {
        Self {
            frequency_khz: 0,
            symbolrate: 0,
            voltage: SecVoltage::Off,
            tone: SecTone::Off,
            fec: Fec::Auto,
            inversion: Inversion::Auto,
        }
    }
}

/// PLS (Physical Layer Signalling) mode for DVB-S2 multistream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlsMode {
    /// Root PLS code, converted to the Gold scrambling sequence index
    #[default]
    Root,
    /// Gold PLS code, used as the scrambling sequence index as-is
    Gold,
    /// Combo PLS code, used as the scrambling sequence index as-is
    Combo,
}

/// DVB-S2 multistream (MIS) / PLS parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mis {
    /// PLS mode
    pub mode: PlsMode,
    /// PLS code
    pub code: u32,
    /// Input stream identifier (`DTV_STREAM_ID`)
    pub stream_id: u32,
}

impl Mis {
    /// PLS scrambling sequence index (`DTV_SCRAMBLING_SEQUENCE_INDEX`)
    /// derived from the PLS mode and code.
    ///
    /// For [`PlsMode::Root`] the code is converted to the Gold scrambling
    /// sequence index (EN 302 307, 5.5.4); for [`PlsMode::Gold`] and
    /// [`PlsMode::Combo`] the code itself is the index.
    ///
    /// Returns `None` for the default [`PlsMode::Root`] code 0, which uses
    /// the default scrambling sequence and does not need to be set.
    pub fn pls_code(&self) -> Option<u32> {
        /// Scrambling sequences are 18-bit Gold sequences
        const PLS_CODE_MASK: u32 = 0x3FFFF;

        let code = self.code & PLS_CODE_MASK;

        if self.mode != PlsMode::Root {
            return Some(code);
        }

        // Invalid code value for PLS Root
        if code == 0 {
            return None;
        }

        let mut x: u32 = 1;
        for g in 0 .. PLS_CODE_MASK {
            if code == x {
                return Some(g);
            }
            x = (((x ^ (x >> 7)) & 1) << 17) | (x >> 1);
        }

        Some(PLS_CODE_MASK)
    }
}

/// DVB-S2 tune parameters. See [`DvbSTune`] for the frequency semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvbS2Tune {
    /// Intermediate frequency in kHz
    pub frequency_khz: u32,
    /// Symbol rate in baud
    pub symbolrate: u32,
    /// LNB voltage (polarization selection)
    pub voltage: SecVoltage,
    /// 22 kHz tone (band selection)
    pub tone: SecTone,
    /// Modulation / constellation
    pub modulation: Modulation,
    /// Inner FEC code rate
    pub fec: Fec,
    /// Spectral inversion
    pub inversion: Inversion,
    /// Pilot tone mode
    pub pilot: Pilot,
    /// Roll-off factor
    pub rolloff: Rolloff,
    /// Multistream / PLS parameters (`DTV_STREAM_ID` is always set when
    /// present, `DTV_SCRAMBLING_SEQUENCE_INDEX` only when the driver
    /// supports it)
    pub mis: Option<Mis>,
}

impl Default for DvbS2Tune {
    fn default() -> Self {
        Self {
            frequency_khz: 0,
            symbolrate: 0,
            voltage: SecVoltage::Off,
            tone: SecTone::Off,
            modulation: Modulation::Psk8,
            fec: Fec::Auto,
            inversion: Inversion::Auto,
            pilot: Pilot::Auto,
            rolloff: Rolloff::R35,
            mis: None,
        }
    }
}

/// DVB-C annex / delivery system variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DvbCAnnex {
    /// DVB-C Annex A/C (DVB-C as deployed in Europe / Japan)
    #[default]
    A,
    /// DVB-C Annex B (ITU-T J.83B, as deployed in North America)
    B,
}

/// DVB-C tune parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvbCTune {
    /// Frequency in Hz
    pub frequency_hz: u32,
    /// Symbol rate in baud
    pub symbolrate: u32,
    /// Cable annex
    pub annex: DvbCAnnex,
    /// Modulation / constellation
    pub modulation: Modulation,
    /// Inner FEC code rate
    pub fec: Fec,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for DvbCTune {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            symbolrate: 0,
            annex: DvbCAnnex::A,
            modulation: Modulation::QamAuto,
            fec: Fec::Auto,
            inversion: Inversion::Auto,
        }
    }
}

/// DVB-T tune parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvbTTune {
    /// Frequency in Hz
    pub frequency_hz: u32,
    /// Channel bandwidth in Hz
    pub bandwidth_hz: u32,
    /// Modulation / constellation
    pub modulation: Modulation,
    /// Code rate of the high-priority stream
    pub code_rate_hp: Fec,
    /// Code rate of the low-priority stream
    pub code_rate_lp: Fec,
    /// Guard interval
    pub guard_interval: GuardInterval,
    /// Transmission (FFT) mode
    pub transmission_mode: TransmitMode,
    /// Hierarchy
    pub hierarchy: Hierarchy,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for DvbTTune {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            bandwidth_hz: 8_000_000,
            modulation: Modulation::QamAuto,
            code_rate_hp: Fec::Auto,
            code_rate_lp: Fec::Auto,
            guard_interval: GuardInterval::Auto,
            transmission_mode: TransmitMode::Auto,
            hierarchy: Hierarchy::None,
            inversion: Inversion::Auto,
        }
    }
}

/// DVB-T2 tune parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvbT2Tune {
    /// Frequency in Hz
    pub frequency_hz: u32,
    /// Channel bandwidth in Hz
    pub bandwidth_hz: u32,
    /// Modulation / constellation
    pub modulation: Modulation,
    /// Code rate
    pub code_rate: Fec,
    /// Guard interval
    pub guard_interval: GuardInterval,
    /// Transmission (FFT) mode
    pub transmission_mode: TransmitMode,
    /// PLP / stream identifier (`DTV_STREAM_ID`)
    pub stream_id: Option<u32>,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for DvbT2Tune {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            bandwidth_hz: 8_000_000,
            modulation: Modulation::QamAuto,
            code_rate: Fec::Auto,
            guard_interval: GuardInterval::Auto,
            transmission_mode: TransmitMode::Auto,
            stream_id: None,
            inversion: Inversion::Auto,
        }
    }
}

/// ATSC tune parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtscTune {
    /// Frequency in Hz
    pub frequency_hz: u32,
    /// Modulation (8-VSB for terrestrial broadcast, 16-VSB for cable)
    pub modulation: Modulation,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for AtscTune {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            modulation: Modulation::Vsb8,
            inversion: Inversion::Auto,
        }
    }
}

/// ISDB-T tune parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsdbTTune {
    /// Frequency in Hz
    pub frequency_hz: u32,
    /// Channel bandwidth in Hz
    pub bandwidth_hz: u32,
    /// Spectral inversion
    pub inversion: Inversion,
}

impl Default for IsdbTTune {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            bandwidth_hz: 6_000_000,
            inversion: Inversion::Auto,
        }
    }
}

/// High-level frontend tune request.
///
/// Wraps the per-delivery-system parameters and lowers them to a DVBv5
/// property command sequence, ready for
/// [`FeDevice::set_properties`](super::FeDevice::set_properties) or
/// [`FeDevice::tune`](super::FeDevice::tune).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuneRequest {
    /// Satellite DVB-S
    DvbS(DvbSTune),
    /// Satellite DVB-S2
    DvbS2(DvbS2Tune),
    /// Cable DVB-C
    DvbC(DvbCTune),
    /// Terrestrial DVB-T
    DvbT(DvbTTune),
    /// Terrestrial DVB-T2
    DvbT2(DvbT2Tune),
    /// Terrestrial ATSC
    Atsc(AtscTune),
    /// Terrestrial ISDB-T
    IsdbT(IsdbTTune),
}

impl TuneRequest {
    /// Returns the delivery system used by this tune request.
    pub fn delivery_system(&self) -> DeliverySystem {
        match self {
            TuneRequest::DvbS(_) => DeliverySystem::Dvbs,
            TuneRequest::DvbS2(_) => DeliverySystem::Dvbs2,
            TuneRequest::DvbC(tune) => match tune.annex {
                DvbCAnnex::A => DeliverySystem::DvbcAnnexA,
                DvbCAnnex::B => DeliverySystem::DvbcAnnexB,
            },
            TuneRequest::DvbT(_) => DeliverySystem::Dvbt,
            TuneRequest::DvbT2(_) => DeliverySystem::Dvbt2,
            TuneRequest::Atsc(_) => DeliverySystem::Atsc,
            TuneRequest::IsdbT(_) => DeliverySystem::Isdbt,
        }
    }

    /// Builds the frontend property command sequence for this tune request.
    pub fn properties(&self) -> Vec<DtvProperty> {
        let mut cmdseq = Vec::with_capacity(16);

        cmdseq.push(DtvProperty::DeliverySystem(self.delivery_system()));

        match self {
            TuneRequest::DvbS(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_khz),
                    DtvProperty::Modulation(Modulation::Qpsk),
                    DtvProperty::Voltage(tune.voltage),
                    DtvProperty::Tone(tune.tone),
                    DtvProperty::Inversion(tune.inversion),
                    DtvProperty::SymbolRate(tune.symbolrate),
                    DtvProperty::InnerFec(tune.fec),
                ]);
            }
            TuneRequest::DvbS2(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_khz),
                    DtvProperty::Modulation(tune.modulation),
                    DtvProperty::Voltage(tune.voltage),
                    DtvProperty::Tone(tune.tone),
                    DtvProperty::Inversion(tune.inversion),
                    DtvProperty::SymbolRate(tune.symbolrate),
                    DtvProperty::InnerFec(tune.fec),
                    DtvProperty::Pilot(tune.pilot),
                    DtvProperty::Rolloff(tune.rolloff),
                ]);
                if let Some(mis) = &tune.mis {
                    cmdseq.push(DtvProperty::StreamId(mis.stream_id));
                    if let Some(pls_code) = mis.pls_code() {
                        cmdseq.push(DtvProperty::ScramblingSequenceIndex(pls_code));
                    }
                }
            }
            TuneRequest::DvbC(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_hz),
                    DtvProperty::Modulation(tune.modulation),
                    DtvProperty::Inversion(tune.inversion),
                    DtvProperty::SymbolRate(tune.symbolrate),
                    DtvProperty::InnerFec(tune.fec),
                ]);
            }
            TuneRequest::DvbT(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_hz),
                    DtvProperty::Modulation(tune.modulation),
                    DtvProperty::BandwidthHz(tune.bandwidth_hz),
                    DtvProperty::Inversion(tune.inversion),
                    DtvProperty::CodeRateHp(tune.code_rate_hp),
                    DtvProperty::CodeRateLp(tune.code_rate_lp),
                    DtvProperty::GuardInterval(tune.guard_interval),
                    DtvProperty::TransmissionMode(tune.transmission_mode),
                    DtvProperty::Hierarchy(tune.hierarchy),
                ]);
            }
            TuneRequest::DvbT2(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_hz),
                    DtvProperty::Modulation(tune.modulation),
                    DtvProperty::BandwidthHz(tune.bandwidth_hz),
                    DtvProperty::Inversion(tune.inversion),
                    DtvProperty::CodeRateHp(tune.code_rate),
                    DtvProperty::GuardInterval(tune.guard_interval),
                    DtvProperty::TransmissionMode(tune.transmission_mode),
                ]);
                if let Some(stream_id) = tune.stream_id {
                    cmdseq.push(DtvProperty::StreamId(stream_id));
                }
            }
            TuneRequest::Atsc(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_hz),
                    DtvProperty::Modulation(tune.modulation),
                    DtvProperty::Inversion(tune.inversion),
                ]);
            }
            TuneRequest::IsdbT(tune) => {
                cmdseq.extend_from_slice(&[
                    DtvProperty::Frequency(tune.frequency_hz),
                    DtvProperty::BandwidthHz(tune.bandwidth_hz),
                    DtvProperty::Inversion(tune.inversion),
                ]);
            }
        }

        cmdseq.push(DtvProperty::Tune);

        cmdseq
    }
}

impl From<&TuneRequest> for Vec<DtvProperty> {
    fn from(request: &TuneRequest) -> Self {
        request.properties()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvbs_tune_properties() {
        let request = TuneRequest::DvbS(DvbSTune {
            frequency_khz: 1_294_000,
            symbolrate: 27_500_000,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
            ..Default::default()
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Dvbs);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Dvbs),
                DtvProperty::Frequency(1_294_000),
                DtvProperty::Modulation(Modulation::Qpsk),
                DtvProperty::Voltage(SecVoltage::V13),
                DtvProperty::Tone(SecTone::Off),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::SymbolRate(27_500_000),
                DtvProperty::InnerFec(Fec::Auto),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn dvbs2_tune_properties() {
        let request = TuneRequest::DvbS2(DvbS2Tune {
            frequency_khz: 1_294_000,
            symbolrate: 27_500_000,
            voltage: SecVoltage::V18,
            tone: SecTone::On,
            modulation: Modulation::Psk8,
            fec: Fec::Fec3_4,
            rolloff: Rolloff::R20,
            pilot: Pilot::Auto,
            mis: Some(Mis {
                mode: PlsMode::Gold,
                code: 5,
                stream_id: 7,
            }),
            inversion: Inversion::Auto,
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Dvbs2);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Dvbs2),
                DtvProperty::Frequency(1_294_000),
                DtvProperty::Modulation(Modulation::Psk8),
                DtvProperty::Voltage(SecVoltage::V18),
                DtvProperty::Tone(SecTone::On),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::SymbolRate(27_500_000),
                DtvProperty::InnerFec(Fec::Fec3_4),
                DtvProperty::Pilot(Pilot::Auto),
                DtvProperty::Rolloff(Rolloff::R20),
                DtvProperty::StreamId(7),
                DtvProperty::ScramblingSequenceIndex(5),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn dvbs2_tune_mis_root_default() {
        let request = TuneRequest::DvbS2(DvbS2Tune {
            frequency_khz: 1_294_000,
            symbolrate: 27_500_000,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
            mis: Some(Mis {
                mode: PlsMode::Root,
                code: 0,
                stream_id: 7,
            }),
            ..Default::default()
        });

        let properties = request.properties();
        assert!(
            properties
                .iter()
                .any(|p| matches!(p, DtvProperty::StreamId(7)))
        );
        assert!(
            !properties
                .iter()
                .any(|p| matches!(p, DtvProperty::ScramblingSequenceIndex(_)))
        );
    }

    #[test]
    fn dvbs2_tune_without_mis() {
        let request = TuneRequest::DvbS2(DvbS2Tune {
            frequency_khz: 1_294_000,
            symbolrate: 27_500_000,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
            ..Default::default()
        });

        assert!(!request.properties().iter().any(|p| matches!(
            p,
            DtvProperty::StreamId(_) | DtvProperty::ScramblingSequenceIndex(_)
        )));
    }

    #[test]
    fn mis_pls_code() {
        // Default Root code 0 uses the default scrambling sequence
        assert_eq!(
            Mis {
                mode: PlsMode::Root,
                code: 0,
                stream_id: 0,
            }
            .pls_code(),
            None
        );

        // Gold and Combo codes are used as-is
        assert_eq!(
            Mis {
                mode: PlsMode::Gold,
                code: 42,
                stream_id: 0,
            }
            .pls_code(),
            Some(42)
        );
        assert_eq!(
            Mis {
                mode: PlsMode::Combo,
                code: 42,
                stream_id: 0,
            }
            .pls_code(),
            Some(42)
        );

        // Codes are masked to 18 bits
        assert_eq!(
            Mis {
                mode: PlsMode::Gold,
                code: 0xFFFFF,
                stream_id: 0,
            }
            .pls_code(),
            Some(0x3FFFF)
        );

        // Root codes are converted to the Gold scrambling sequence index
        assert_eq!(
            Mis {
                mode: PlsMode::Root,
                code: 0x00001,
                stream_id: 0,
            }
            .pls_code(),
            Some(0)
        );
        assert_eq!(
            Mis {
                mode: PlsMode::Root,
                code: 0x20000,
                stream_id: 0,
            }
            .pls_code(),
            Some(1)
        );
        assert_eq!(
            Mis {
                mode: PlsMode::Root,
                code: 0x10000,
                stream_id: 0,
            }
            .pls_code(),
            Some(2)
        );
    }

    #[test]
    fn dvbc_tune_properties() {
        let request = TuneRequest::DvbC(DvbCTune {
            frequency_hz: 346_000_000,
            symbolrate: 6_900_000,
            annex: DvbCAnnex::B,
            modulation: Modulation::Qam256,
            ..Default::default()
        });

        assert_eq!(request.delivery_system(), DeliverySystem::DvbcAnnexB);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::DvbcAnnexB),
                DtvProperty::Frequency(346_000_000),
                DtvProperty::Modulation(Modulation::Qam256),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::SymbolRate(6_900_000),
                DtvProperty::InnerFec(Fec::Auto),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn dvbt_tune_properties() {
        let request = TuneRequest::DvbT(DvbTTune {
            frequency_hz: 474_000_000,
            bandwidth_hz: 8_000_000,
            modulation: Modulation::Qam64,
            code_rate_hp: Fec::Fec2_3,
            code_rate_lp: Fec::Fec2_3,
            guard_interval: GuardInterval::Gi1_16,
            transmission_mode: TransmitMode::Tm8K,
            hierarchy: Hierarchy::None,
            inversion: Inversion::Auto,
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Dvbt);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Dvbt),
                DtvProperty::Frequency(474_000_000),
                DtvProperty::Modulation(Modulation::Qam64),
                DtvProperty::BandwidthHz(8_000_000),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::CodeRateHp(Fec::Fec2_3),
                DtvProperty::CodeRateLp(Fec::Fec2_3),
                DtvProperty::GuardInterval(GuardInterval::Gi1_16),
                DtvProperty::TransmissionMode(TransmitMode::Tm8K),
                DtvProperty::Hierarchy(Hierarchy::None),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn dvbt2_tune_properties() {
        let request = TuneRequest::DvbT2(DvbT2Tune {
            frequency_hz: 474_000_000,
            bandwidth_hz: 8_000_000,
            modulation: Modulation::Qam256,
            code_rate: Fec::Fec3_4,
            guard_interval: GuardInterval::Auto,
            transmission_mode: TransmitMode::Tm32K,
            stream_id: Some(3),
            inversion: Inversion::Auto,
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Dvbt2);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Dvbt2),
                DtvProperty::Frequency(474_000_000),
                DtvProperty::Modulation(Modulation::Qam256),
                DtvProperty::BandwidthHz(8_000_000),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::CodeRateHp(Fec::Fec3_4),
                DtvProperty::GuardInterval(GuardInterval::Auto),
                DtvProperty::TransmissionMode(TransmitMode::Tm32K),
                DtvProperty::StreamId(3),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn atsc_tune_properties() {
        let request = TuneRequest::Atsc(AtscTune {
            frequency_hz: 533_000_000,
            ..Default::default()
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Atsc);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Atsc),
                DtvProperty::Frequency(533_000_000),
                DtvProperty::Modulation(Modulation::Vsb8),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn isdbt_tune_properties() {
        let request = TuneRequest::IsdbT(IsdbTTune {
            frequency_hz: 521_142_857,
            ..Default::default()
        });

        assert_eq!(request.delivery_system(), DeliverySystem::Isdbt);
        assert_eq!(
            request.properties(),
            vec![
                DtvProperty::DeliverySystem(DeliverySystem::Isdbt),
                DtvProperty::Frequency(521_142_857),
                DtvProperty::BandwidthHz(6_000_000),
                DtvProperty::Inversion(Inversion::Auto),
                DtvProperty::Tune,
            ]
        );
    }

    #[test]
    fn properties_matches_vec_conversion() {
        let request = TuneRequest::DvbS(DvbSTune {
            frequency_khz: 1_294_000,
            symbolrate: 27_500_000,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
            ..Default::default()
        });

        let cmdseq: Vec<DtvProperty> = (&request).into();
        assert_eq!(cmdseq, request.properties());
    }
}
