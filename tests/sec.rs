use std::time::Duration;

use libdvb::fe::{
    SecCommand, diseqc_1_0_sequence, diseqc_1_1_sequence, parse_sec_sequence,
    sys::{SecMiniCmd, SecTone, SecVoltage},
    toneburst_sequence,
};

#[test]
fn parse_diseqc_dsl_sequence() {
    let sequence = parse_sec_sequence("t V W200 [E0 10 38 F3] W15 T").unwrap();

    assert_eq!(
        sequence,
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
fn parse_compact_and_spaced_hex() {
    let compact = parse_sec_sequence("[E01038F0]").unwrap();
    let spaced = parse_sec_sequence("[E0 10 38 F0]").unwrap();

    assert_eq!(
        compact,
        vec![SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xF0])]
    );
    assert_eq!(compact, spaced);
}

#[test]
fn parse_rejects_invalid_sequences() {
    assert!(parse_sec_sequence("W").is_err());
    assert!(parse_sec_sequence("[E0 10 38 F]").is_err());
    assert!(parse_sec_sequence("[E0 10]").is_err());
    assert!(parse_sec_sequence("[E0 10 38 F0 00 00 00]").is_err());
    assert!(parse_sec_sequence("[E0 10 38 X0]").is_err());
    assert!(parse_sec_sequence("[E0 10 38 F0").is_err());
    assert!(parse_sec_sequence("[E 0 10 38]").is_err());
    assert!(parse_sec_sequence("x").is_err());
}

#[test]
fn diseqc_1_0_builder_generates_committed_switch_bytes() {
    let sequence = diseqc_1_0_sequence(4, SecVoltage::V18, SecTone::On).unwrap();

    assert_eq!(
        sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(Duration::from_millis(200)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xFF]),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::On),
        ]
    );

    assert!(diseqc_1_0_sequence(0, SecVoltage::V13, SecTone::Off).is_err());
    assert!(diseqc_1_0_sequence(5, SecVoltage::V13, SecTone::Off).is_err());
}

#[test]
fn diseqc_1_1_builder_generates_uncommitted_switch_bytes() {
    let sequence = diseqc_1_1_sequence(16, SecVoltage::V13, SecTone::Off).unwrap();

    assert_eq!(
        sequence,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(SecVoltage::V13),
            SecCommand::Wait(Duration::from_millis(200)),
            SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x39, 0xFF]),
            SecCommand::Wait(Duration::from_millis(15)),
            SecCommand::SetTone(SecTone::Off),
        ]
    );

    assert!(diseqc_1_1_sequence(0, SecVoltage::V13, SecTone::Off).is_err());
    assert!(diseqc_1_1_sequence(17, SecVoltage::V13, SecTone::Off).is_err());
}

#[test]
fn toneburst_builder_generates_mini_burst_sequence() {
    let sequence = toneburst_sequence(SecMiniCmd::B, SecVoltage::V18, SecTone::On);

    assert_eq!(
        sequence,
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
