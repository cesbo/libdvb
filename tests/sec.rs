use std::time::Duration;

use libdvb::fe::{
    DiseqcConfig,
    DiseqcSwitchConfig,
    SecCommand,
    ToneburstConfig,
    UnicableConfig,
    diseqc_sequence,
    sys::{
        SecMiniCmd,
        SecTone,
        SecVoltage,
    },
};

#[test]
fn diseqc_dsl_sequence_generates_sec_commands() {
    let tune = diseqc_sequence(
        1_232,
        DiseqcConfig::Dsl("t V W200 [E0 10 38 F3] W15 T".to_owned()),
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
fn diseqc_dsl_accepts_compact_and_spaced_hex() {
    let compact = diseqc_sequence(1_232, DiseqcConfig::Dsl("[E01038F0]".to_owned())).unwrap();
    let spaced = diseqc_sequence(1_232, DiseqcConfig::Dsl("[E0 10 38 F0]".to_owned())).unwrap();

    assert_eq!(
        compact.sec_sequence,
        vec![SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xF0])]
    );
    assert_eq!(compact.sec_sequence, spaced.sec_sequence);
}

#[test]
fn diseqc_dsl_rejects_invalid_sequences() {
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
        assert!(diseqc_sequence(1_232, DiseqcConfig::Dsl(input.to_owned())).is_err());
    }
}

#[test]
fn diseqc_1_0_builder_generates_committed_switch_bytes() {
    let tune = diseqc_sequence(
        1_232,
        DiseqcConfig::Switch1_0(DiseqcSwitchConfig {
            port: 4,
            voltage: SecVoltage::V18,
            tone: SecTone::On,
        }),
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

    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Switch1_0(DiseqcSwitchConfig {
                port: 0,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
            })
        )
        .is_err()
    );
    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Switch1_0(DiseqcSwitchConfig {
                port: 5,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
            })
        )
        .is_err()
    );
}

#[test]
fn diseqc_1_1_builder_generates_uncommitted_switch_bytes() {
    let tune = diseqc_sequence(
        1_232,
        DiseqcConfig::Switch1_1(DiseqcSwitchConfig {
            port: 16,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
        }),
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

    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Switch1_1(DiseqcSwitchConfig {
                port: 0,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
            })
        )
        .is_err()
    );
    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Switch1_1(DiseqcSwitchConfig {
                port: 17,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
            })
        )
        .is_err()
    );
}

#[test]
fn toneburst_builder_generates_mini_burst_sequence() {
    let tune = diseqc_sequence(
        1_232,
        DiseqcConfig::Toneburst(ToneburstConfig {
            burst: SecMiniCmd::B,
            voltage: SecVoltage::V18,
            tone: SecTone::On,
        }),
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
    let tune = diseqc_sequence(
        1_232,
        DiseqcConfig::Unicable1(UnicableConfig {
            slot: 3,
            user_band_frequency_mhz: 1210,
            position: 1,
            voltage: SecVoltage::V18,
            tone: SecTone::On,
            pin: None,
        }),
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
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x5A, 0x5D, 0x05]),
            SecCommand::Wait(Duration::from_millis(50)),
            SecCommand::SetVoltage(SecVoltage::V13),
        ]
    );

    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Unicable1(UnicableConfig {
                slot: 0,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
                pin: None,
            })
        )
        .is_err()
    );
    assert!(
        diseqc_sequence(
            1_232,
            DiseqcConfig::Unicable1(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 2,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
                pin: None,
            })
        )
        .is_err()
    );
}

#[test]
fn unicable_2_builder_generates_en50607_bytes() {
    let tune = diseqc_sequence(
        1_234,
        DiseqcConfig::Unicable2(UnicableConfig {
            slot: 32,
            user_band_frequency_mhz: 1210,
            position: 15,
            voltage: SecVoltage::V18,
            tone: SecTone::On,
            pin: Some(0x44),
        }),
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

    let tune = diseqc_sequence(
        950,
        DiseqcConfig::Unicable2(UnicableConfig {
            slot: 1,
            user_band_frequency_mhz: 980,
            position: 0,
            voltage: SecVoltage::V13,
            tone: SecTone::Off,
            pin: None,
        }),
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
        diseqc_sequence(
            1_234,
            DiseqcConfig::Unicable2(UnicableConfig {
                slot: 33,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
                pin: None,
            })
        )
        .is_err()
    );
    assert!(
        diseqc_sequence(
            1_234,
            DiseqcConfig::Unicable2(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 64,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
                pin: None,
            })
        )
        .is_err()
    );
    assert!(
        diseqc_sequence(
            2_200,
            DiseqcConfig::Unicable2(UnicableConfig {
                slot: 1,
                user_band_frequency_mhz: 1210,
                position: 0,
                voltage: SecVoltage::V13,
                tone: SecTone::Off,
                pin: None,
            })
        )
        .is_err()
    );
}
