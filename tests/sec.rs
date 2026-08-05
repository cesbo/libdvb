use std::time::Duration;

use libdvb::fe::{
    DiseqcSwitchConfig,
    Lnb,
    SecCommand,
    SecConfig,
    SecTimings,
    ToneburstConfig,
    UnicableConfig,
    sec_sequence,
    sys::{
        SecMiniCmd,
        SecTone,
        SecVoltage,
    },
};

const UNIVERSAL: Lnb = Lnb::Universal {
    lof_low_mhz: 9_750,
    lof_high_mhz: 10_600,
    switch_mhz: 11_700,
};

/// Transponder in the low band of [`UNIVERSAL`]: 1232 MHz IF, band tone off.
const LOW_BAND: u32 = 10_982;

/// Transponder in the high band of [`UNIVERSAL`]: the same 1232 MHz IF, band
/// tone on.
const HIGH_BAND: u32 = 11_832;

#[test]
fn lnb_converts_transponder_to_intermediate_frequency() {
    assert_eq!(
        Lnb::Passthrough.intermediate(1_232).unwrap(),
        (1_232, SecTone::Off)
    );
    assert_eq!(
        Lnb::Single { lof_mhz: 10_750 }
            .intermediate(12_000)
            .unwrap(),
        (1_250, SecTone::Off)
    );
    assert_eq!(
        UNIVERSAL.intermediate(LOW_BAND).unwrap(),
        (1_232, SecTone::Off)
    );
    assert_eq!(
        UNIVERSAL.intermediate(HIGH_BAND).unwrap(),
        (1_232, SecTone::On)
    );
    // the switch frequency itself belongs to the high band
    assert_eq!(
        UNIVERSAL.intermediate(11_700).unwrap(),
        (1_100, SecTone::On)
    );
    assert_eq!(
        Lnb::CBand { lof_mhz: 5_150 }.intermediate(3_800).unwrap(),
        (1_350, SecTone::Off)
    );
}

#[test]
fn lnb_rejects_a_transponder_it_cannot_convert() {
    assert!(Lnb::Single { lof_mhz: 10_750 }.intermediate(9_000).is_err());
    assert!(UNIVERSAL.intermediate(9_000).is_err());
    assert!(Lnb::CBand { lof_mhz: 5_150 }.intermediate(6_000).is_err());
}

#[test]
fn sec_dsl_sequence_generates_sec_commands() {
    let tune = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Dsl("t V W200 [E0 10 38 F3] W15 T".to_owned()),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(200)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xF3]),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::On),
        ]
    );
}

#[test]
fn sec_dsl_accepts_compact_and_spaced_hex() {
    let compact = sec_sequence(
        1_232,
        Lnb::Passthrough,
        SecConfig::Dsl("[E01038F0]".to_owned()),
        SecTimings::default(),
    )
    .unwrap();
    let spaced = sec_sequence(
        1_232,
        Lnb::Passthrough,
        SecConfig::Dsl("[E0 10 38 F0]".to_owned()),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(
        compact.sec_sequence,
        vec![SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xF0])]
    );
    assert_eq!(compact.sec_sequence, spaced.sec_sequence);
}

#[test]
fn sec_dsl_rejects_invalid_sequences() {
    for input in [
        "W",
        "[E0 10 38 F]",
        "[E0 10]",
        "[E0 10 38 F0 00 00 00]",
        "[E0 10 38 X0]",
        "[E0 10 38 F0",
        "[E 0 10 38]",
        "x",
    ] {
        assert!(
            sec_sequence(
                1_232,
                Lnb::Passthrough,
                SecConfig::Dsl(input.to_owned()),
                SecTimings::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn lnb_config_generates_voltage_and_band_tone_sequence() {
    let tune = sec_sequence(
        HIGH_BAND,
        UNIVERSAL,
        SecConfig::Lnb {
            voltage: SecVoltage::V18,
        },
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(100)),
            SecCommand::SetTone(SecTone::On),
            SecCommand::Wait(Duration::from_millis(100)),
        ]
    );

    let tune = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Lnb {
            voltage: SecVoltage::V13,
        },
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence[1],
        SecCommand::SetVoltage(SecVoltage::V13)
    );
    assert_eq!(tune.sec_sequence[3], SecCommand::SetTone(SecTone::Off));
}

#[test]
fn shared_lnb_releases_voltage_and_tone() {
    let tune = sec_sequence(
        HIGH_BAND,
        UNIVERSAL,
        SecConfig::Shared,
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::Off),
        ]
    );
}

#[test]
fn diseqc_1_0_builder_generates_committed_switch_bytes() {
    let tune = sec_sequence(
        HIGH_BAND,
        UNIVERSAL,
        SecConfig::Switch1_0(DiseqcSwitchConfig {
            port: 4,
            voltage: SecVoltage::V18,
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(200)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xFF]),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::On),
        ]
    );

    // the low band clears both the band bit of the command and the tone
    let tune = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Switch1_0(DiseqcSwitchConfig {
            port: 4,
            voltage: SecVoltage::V18,
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(
        tune.sec_sequence[3],
        SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xFE])
    );
    assert_eq!(tune.sec_sequence[5], SecCommand::SetTone(SecTone::Off));

    for port in [0, 5] {
        assert!(
            sec_sequence(
                LOW_BAND,
                UNIVERSAL,
                SecConfig::Switch1_0(DiseqcSwitchConfig {
                    port,
                    voltage: SecVoltage::V13,
                }),
                SecTimings::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn diseqc_1_1_builder_generates_uncommitted_switch_bytes() {
    let tune = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Switch1_1(DiseqcSwitchConfig {
            port: 16,
            voltage: SecVoltage::V13,
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V13),
            SecCommand::Wait(Duration::from_millis(200)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x39, 0xFF]),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::Off),
        ]
    );

    for port in [0, 17] {
        assert!(
            sec_sequence(
                LOW_BAND,
                UNIVERSAL,
                SecConfig::Switch1_1(DiseqcSwitchConfig {
                    port,
                    voltage: SecVoltage::V13,
                }),
                SecTimings::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn toneburst_builder_generates_mini_burst_sequence() {
    let tune = sec_sequence(
        HIGH_BAND,
        UNIVERSAL,
        SecConfig::Toneburst(ToneburstConfig {
            burst: SecMiniCmd::B,
            voltage: SecVoltage::V18,
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_232_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SendBurst(SecMiniCmd::B),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::On),
        ]
    );
}

#[test]
fn unicable_1_builder_generates_en50494_bytes() {
    let tune = sec_sequence(
        HIGH_BAND,
        UNIVERSAL,
        SecConfig::Unicable1(UnicableConfig {
            slot: 3,
            user_band_frequency_mhz: 1210,
            position: 1,
            voltage: SecVoltage::V18,
            pin: None,
        }),
        SecTimings::default(),
    )
    .unwrap();

    // the frontend follows the user band, not the transponder
    assert_eq!(tune.frontend_frequency_khz, 1_210_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetVoltage(SecVoltage::V13),
            SecCommand::SetTone(SecTone::Off),
            SecCommand::Wait(Duration::from_millis(5)),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x5A, 0x5D, 0x05]),
            SecCommand::Wait(Duration::from_millis(50)),
            SecCommand::SetVoltage(SecVoltage::V13),
        ]
    );

    assert!(
        sec_sequence(
            LOW_BAND,
            UNIVERSAL,
            SecConfig::Unicable1(UnicableConfig {
                slot: 0,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                pin: None,
            }),
            SecTimings::default(),
        )
        .is_err()
    );
    assert!(
        sec_sequence(
            LOW_BAND,
            UNIVERSAL,
            SecConfig::Unicable1(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 2,
                voltage: SecVoltage::V13,
                pin: None,
            }),
            SecTimings::default(),
        )
        .is_err()
    );
}

#[test]
fn unicable_2_builder_generates_en50607_bytes() {
    let tune = sec_sequence(
        11_834,
        UNIVERSAL,
        SecConfig::Unicable2(UnicableConfig {
            slot: 32,
            user_band_frequency_mhz: 1210,
            position: 15,
            voltage: SecVoltage::V18,
            pin: Some(0x44),
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(tune.frontend_frequency_khz, 1_210_000);
    assert_eq!(
        tune.sec_sequence,
        vec![
            SecCommand::SetVoltage(SecVoltage::V13),
            SecCommand::SetTone(SecTone::Off),
            SecCommand::Wait(Duration::from_millis(5)),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SendMasterCommand(vec![0x71, 0xFC, 0x6E, 0x3F, 0x44]),
            SecCommand::Wait(Duration::from_millis(50)),
            SecCommand::SetVoltage(SecVoltage::V13),
        ]
    );

    let tune = sec_sequence(
        10_700,
        UNIVERSAL,
        SecConfig::Unicable2(UnicableConfig {
            slot: 1,
            user_band_frequency_mhz: 980,
            position: 0,
            voltage: SecVoltage::V13,
            pin: None,
        }),
        SecTimings::default(),
    )
    .unwrap();

    assert_eq!(
        tune.sec_sequence[5],
        SecCommand::SendMasterCommand(vec![0x70, 0x03, 0x52, 0x00])
    );
}

#[test]
fn unicable_2_builder_rejects_invalid_values() {
    assert!(
        sec_sequence(
            1_234,
            Lnb::Passthrough,
            SecConfig::Unicable2(UnicableConfig {
                slot: 33,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                pin: None,
            }),
            SecTimings::default(),
        )
        .is_err()
    );
    assert!(
        sec_sequence(
            1_234,
            Lnb::Passthrough,
            SecConfig::Unicable2(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 64,
                voltage: SecVoltage::V13,
                pin: None,
            }),
            SecTimings::default(),
        )
        .is_err()
    );
    assert!(
        sec_sequence(
            2_200,
            Lnb::Passthrough,
            SecConfig::Unicable2(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                pin: None,
            }),
            SecTimings::default(),
        )
        .is_err()
    );
}

#[test]
fn timings_replace_the_generated_waits() {
    let timings = SecTimings {
        switch_settle: Duration::from_millis(250),
        message_gap: Duration::from_millis(20),
        unicable_settle: Duration::from_millis(8),
        unicable_hold: Duration::from_millis(60),
        lnb_settle: Duration::from_millis(30),
    };

    let switch = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Switch1_0(DiseqcSwitchConfig {
            port: 1,
            voltage: SecVoltage::V13,
        }),
        timings,
    )
    .unwrap();
    assert_eq!(
        waits(&switch.sec_sequence),
        vec![Duration::from_millis(250), Duration::from_millis(20)]
    );

    let unicable = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Unicable1(UnicableConfig {
            slot: 1,
            user_band_frequency_mhz: 1210,
            position: 0,
            voltage: SecVoltage::V13,
            pin: None,
        }),
        timings,
    )
    .unwrap();
    assert_eq!(
        waits(&unicable.sec_sequence),
        vec![
            Duration::from_millis(8),
            Duration::from_millis(20),
            Duration::from_millis(60),
        ]
    );

    let lnb = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Lnb {
            voltage: SecVoltage::V13,
        },
        timings,
    )
    .unwrap();
    assert_eq!(
        waits(&lnb.sec_sequence),
        vec![Duration::from_millis(30), Duration::from_millis(30)]
    );

    // the DSL spells its own waits out and ignores the timings
    let dsl = sec_sequence(
        LOW_BAND,
        UNIVERSAL,
        SecConfig::Dsl("t v W200 [E0 10 38 F0] W15 t".to_owned()),
        timings,
    )
    .unwrap();
    assert_eq!(
        waits(&dsl.sec_sequence),
        vec![Duration::from_millis(200), Duration::from_millis(15)]
    );
}

fn waits(sequence: &[SecCommand]) -> Vec<Duration> {
    sequence
        .iter()
        .filter_map(|command| match command {
            SecCommand::Wait(duration) => Some(*duration),
            _ => None,
        })
        .collect()
}
