use std::time::Duration;

use super::sys::{
    SecMiniCmd,
    SecTone,
    SecVoltage,
};
use crate::error::{
    Error,
    Result,
};

/// One SEC/DiSEqC operation in a frontend control sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecCommand {
    /// Turns the continuous 22kHz tone on or off.
    SetTone(SecTone),
    /// Sets LNB voltage.
    SetVoltage(SecVoltage),
    /// Sends a mini-DiSEqC tone/data burst.
    SendBurst(SecMiniCmd),
    /// Sends a DiSEqC master command. Valid Linux DVB length is 3..=6 bytes.
    SendMasterCommand(Vec<u8>),
    /// Waits before executing the next command.
    Wait(Duration),
}

/// Low-noise block downconverter in front of the frontend.
///
/// Converts a transponder frequency into the intermediate frequency the
/// frontend tunes to, together with the 22 kHz tone that selects that
/// band - one decision, so the two cannot contradict each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lnb {
    /// The frequency is already an intermediate frequency; nothing to
    /// convert, and the tone stays off.
    Passthrough,
    /// One local oscillator.
    Single {
        /// Local oscillator frequency in MHz.
        lof_mhz: u32,
    },
    /// Two local oscillators, selected by the 22 kHz tone. A transponder at
    /// or above `switch_mhz` uses the high oscillator and turns the tone on.
    Universal {
        /// Low band local oscillator frequency in MHz.
        lof_low_mhz: u32,
        /// High band local oscillator frequency in MHz.
        lof_high_mhz: u32,
        /// Band switch frequency in MHz.
        switch_mhz: u32,
    },
    /// Inverted conversion, as used in the C band: the intermediate
    /// frequency is `lof_mhz - transponder`.
    CBand {
        /// Local oscillator frequency in MHz.
        lof_mhz: u32,
    },
}

impl Lnb {
    /// Intermediate frequency in MHz for `transponder_mhz`, and the band
    /// tone that selects it.
    pub fn intermediate(&self, transponder_mhz: u32) -> Result<(u32, SecTone)> {
        match *self {
            Lnb::Passthrough => Ok((transponder_mhz, SecTone::Off)),
            Lnb::Single { lof_mhz } => Ok((lnb_offset(transponder_mhz, lof_mhz)?, SecTone::Off)),
            Lnb::Universal {
                lof_low_mhz,
                lof_high_mhz,
                switch_mhz,
            } => {
                if transponder_mhz >= switch_mhz {
                    Ok((lnb_offset(transponder_mhz, lof_high_mhz)?, SecTone::On))
                } else {
                    Ok((lnb_offset(transponder_mhz, lof_low_mhz)?, SecTone::Off))
                }
            }
            Lnb::CBand { lof_mhz } => Ok((lnb_offset(lof_mhz, transponder_mhz)?, SecTone::Off)),
        }
    }
}

fn lnb_offset(from_mhz: u32, subtract_mhz: u32) -> Result<u32> {
    from_mhz.checked_sub(subtract_mhz).ok_or_else(|| {
        Error::InvalidData(format!(
            "LNB conversion needs {} MHz to be at least {} MHz",
            from_mhz, subtract_mhz
        ))
    })
}

/// Common inputs for DiSEqC switch commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiseqcSwitchConfig {
    /// Switch port number. Valid range depends on the DiSEqC level.
    pub port: u8,
    /// Polarization encoded in the switch command.
    pub voltage: SecVoltage,
}

/// Inputs for toneburst A/B selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneburstConfig {
    /// Toneburst satellite selection.
    pub burst: SecMiniCmd,
    /// Polarization to set before sending the burst.
    pub voltage: SecVoltage,
}

/// Common inputs for Unicable channel-change commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnicableConfig {
    /// User band / SCR slot number. Valid range depends on the protocol.
    pub slot: u8,
    /// User band center frequency in MHz.
    pub user_band_frequency_mhz: u32,
    /// Satellite position index.
    pub position: u8,
    /// Polarization encoded in the Unicable command.
    pub voltage: SecVoltage,
    /// Optional EN 50607 PIN. It is ignored for Unicable I / EN 50494.
    pub pin: Option<u8>,
}

/// Wait times used while building a SEC sequence.
///
/// The defaults reproduce the sequences this module generates when nothing
/// overrides them. [`SecConfig::Dsl`] spells its waits out in the
/// sequence text itself and ignores this struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecTimings {
    /// Settle time after the voltage is set, before a switch command. The
    /// switch is powered by that voltage and has to come up first.
    pub switch_settle: Duration,
    /// Gap between one SEC message and whatever follows it.
    pub message_gap: Duration,
    /// Settle time after the idle voltage, before it is raised to carry a
    /// Unicable command.
    pub unicable_settle: Duration,
    /// Hold time after a Unicable command, before the voltage drops back.
    pub unicable_hold: Duration,
    /// Settle time after the voltage and again after the band tone of a
    /// plain LNB setup.
    pub lnb_settle: Duration,
}

impl Default for SecTimings {
    fn default() -> Self {
        Self {
            switch_settle: Duration::from_millis(200),
            message_gap: Duration::from_millis(15),
            unicable_settle: Duration::from_millis(5),
            unicable_hold: Duration::from_millis(50),
            lnb_settle: Duration::from_millis(100),
        }
    }
}

/// What sits between the LNB and the frontend, and how to address it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecConfig {
    /// Nothing but the LNB: set the polarization voltage and the band tone.
    Lnb {
        /// Polarization selection.
        voltage: SecVoltage,
    },
    /// An LNB powered by another receiver on the same cable. The voltage and
    /// the tone are released instead of driven, so this receiver does not
    /// fight the one that owns them; the band still follows the [`Lnb`],
    /// because the other receiver is expected to be selecting it.
    Shared,
    /// DiSEqC 1.0 committed switch.
    Switch1_0(DiseqcSwitchConfig),
    /// DiSEqC 1.1 uncommitted switch.
    Switch1_1(DiseqcSwitchConfig),
    /// Mini-DiSEqC toneburst.
    Toneburst(ToneburstConfig),
    /// Unicable I / EN 50494.
    Unicable1(UnicableConfig),
    /// Unicable II / EN 50607.
    Unicable2(UnicableConfig),
    /// Custom SEC/DiSEqC sequence in the documented DSL format. It carries
    /// its own tone commands, so the band the [`Lnb`] derives is not applied.
    Dsl(String),
}

/// Generated SEC sequence and the resulting frontend frequency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecSetup {
    /// Frontend frequency after LNB and Unicable translation, in kHz.
    pub frontend_frequency_khz: u32,
    /// SEC commands that perform the setup.
    pub sec_sequence: Vec<SecCommand>,
}

/// Parses an Astra-compatible SEC/DiSEqC DSL sequence.
///
/// Supported commands:
///
/// - `t` - tone off
/// - `T` - tone on
/// - `v` - 13V
/// - `V` - 18V
/// - `A` - mini burst A
/// - `B` - mini burst B
/// - `W<number>` - wait in milliseconds
/// - `[hex bytes]` - DiSEqC master command, 3 to 6 bytes
fn parse_sec_sequence(input: &str) -> Result<Vec<SecCommand>> {
    let mut parser = Parser::new(input);
    parser.parse()
}

/// Builds the SEC sequence that puts the requested transponder in front of
/// the frontend, and reports the frequency to tune to afterwards.
///
/// `transponder_mhz` is the broadcast frequency; `lnb` converts it to the
/// intermediate frequency and derives the band tone. `timings` are the wait
/// times to place between the commands, and are ignored by
/// [`SecConfig::Dsl`], which carries its own.
pub fn sec_sequence(
    transponder_mhz: u32,
    lnb: Lnb,
    config: SecConfig,
    timings: SecTimings,
) -> Result<SecSetup> {
    let (frequency_mhz, band) = lnb.intermediate(transponder_mhz)?;

    match config {
        SecConfig::Lnb { voltage } => Ok(lnb_sequence(frequency_mhz, voltage, band, timings)),
        SecConfig::Shared => Ok(sec_setup(
            frequency_mhz,
            vec![
                SecCommand::SetTone(SecTone::Off),
                SecCommand::SetVoltage(SecVoltage::Off),
            ],
        )),
        SecConfig::Switch1_0(config) => diseqc_1_0_sequence(frequency_mhz, config, band, timings),
        SecConfig::Switch1_1(config) => diseqc_1_1_sequence(frequency_mhz, config, band, timings),
        SecConfig::Toneburst(config) => {
            Ok(toneburst_sequence(frequency_mhz, config, band, timings))
        }
        SecConfig::Unicable1(config) => unicable_1_sequence(frequency_mhz, config, band, timings),
        SecConfig::Unicable2(config) => unicable_2_sequence(frequency_mhz, config, band, timings),
        SecConfig::Dsl(input) => Ok(sec_setup(frequency_mhz, parse_sec_sequence(&input)?)),
    }
}

/// Builds the voltage/tone sequence of a plain LNB.
fn lnb_sequence(
    frequency_mhz: u32,
    voltage: SecVoltage,
    band: SecTone,
    timings: SecTimings,
) -> SecSetup {
    sec_setup(
        frequency_mhz,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(voltage),
            SecCommand::Wait(timings.lnb_settle),
            SecCommand::SetTone(band),
            SecCommand::Wait(timings.lnb_settle),
        ],
    )
}

/// Builds a DiSEqC 1.0 committed-switch sequence.
fn diseqc_1_0_sequence(
    frequency_mhz: u32,
    config: DiseqcSwitchConfig,
    band: SecTone,
    timings: SecTimings,
) -> Result<SecSetup> {
    if !(1 ..= 4).contains(&config.port) {
        return Err(Error::InvalidData(format!(
            "DiSEqC 1.0 port must be in range 1..=4, got {}",
            config.port
        )));
    }

    let port = config.port - 1;
    let data = 0xF0
        | (port << 2)
        | (if config.voltage == SecVoltage::V18 {
            0x02
        } else {
            0x00
        })
        | (if band == SecTone::On { 0x01 } else { 0x00 });

    Ok(sec_setup(
        frequency_mhz,
        controlled_master_sequence(config.voltage, band, [0xE0, 0x10, 0x38, data], timings),
    ))
}

/// Builds a DiSEqC 1.1 uncommitted-switch sequence.
fn diseqc_1_1_sequence(
    frequency_mhz: u32,
    config: DiseqcSwitchConfig,
    band: SecTone,
    timings: SecTimings,
) -> Result<SecSetup> {
    if !(1 ..= 16).contains(&config.port) {
        return Err(Error::InvalidData(format!(
            "DiSEqC 1.1 port must be in range 1..=16, got {}",
            config.port
        )));
    }

    let port = config.port - 1;
    let data = 0xF0 | port;

    Ok(sec_setup(
        frequency_mhz,
        controlled_master_sequence(config.voltage, band, [0xE0, 0x10, 0x39, data], timings),
    ))
}

/// Builds a toneburst A/B sequence.
fn toneburst_sequence(
    frequency_mhz: u32,
    config: ToneburstConfig,
    band: SecTone,
    timings: SecTimings,
) -> SecSetup {
    sec_setup(
        frequency_mhz,
        vec![
            SecCommand::SetTone(SecTone::Off),
            SecCommand::SetVoltage(config.voltage),
            SecCommand::Wait(timings.message_gap),
            SecCommand::SendBurst(config.burst),
            SecCommand::Wait(timings.message_gap),
            SecCommand::SetTone(band),
        ],
    )
}

/// Builds a Unicable I / EN 50494 channel-change sequence.
fn unicable_1_sequence(
    frequency_mhz: u32,
    config: UnicableConfig,
    band: SecTone,
    timings: SecTimings,
) -> Result<SecSetup> {
    if !(1 ..= 8).contains(&config.slot) {
        return Err(Error::InvalidData(format!(
            "Unicable I slot must be in range 1..=8, got {}",
            config.slot
        )));
    }
    if config.position > 1 {
        return Err(Error::InvalidData(format!(
            "Unicable I position must be in range 0..=1, got {}",
            config.position
        )));
    }

    let x = frequency_mhz
        .checked_add(config.user_band_frequency_mhz)
        .and_then(|v| v.checked_add(2))
        .map(|v| v / 4)
        .and_then(|v| v.checked_sub(350))
        .ok_or_else(|| Error::InvalidData("Unicable I frequency is out of range".to_owned()))?;

    if x > 0x03FF {
        return Err(Error::InvalidData(format!(
            "Unicable I encoded frequency must fit 10 bits, got {}",
            x
        )));
    }

    let b1 = ((config.slot - 1) << 5)
        | (config.position << 4)
        | sec_voltage_bit(config.voltage, 0x08)
        | sec_tone_bit(band, 0x04)
        | ((x >> 8) as u8 & 0x03);
    let b2 = x as u8;

    Ok(unicable_tune(
        config.user_band_frequency_mhz,
        vec![0xE0, 0x10, 0x5A, b1, b2],
        timings,
    ))
}

/// Builds a Unicable II / EN 50607 channel-change sequence.
fn unicable_2_sequence(
    frequency_mhz: u32,
    config: UnicableConfig,
    band: SecTone,
    timings: SecTimings,
) -> Result<SecSetup> {
    if !(1 ..= 32).contains(&config.slot) {
        return Err(Error::InvalidData(format!(
            "Unicable II slot must be in range 1..=32, got {}",
            config.slot
        )));
    }
    if config.position > 63 {
        return Err(Error::InvalidData(format!(
            "Unicable II position must be in range 0..=63, got {}",
            config.position
        )));
    }

    let x = frequency_mhz
        .checked_sub(100)
        .ok_or_else(|| Error::InvalidData("Unicable II frequency is out of range".to_owned()))?;

    if x > 0x07FF {
        return Err(Error::InvalidData(format!(
            "Unicable II encoded frequency must fit 11 bits, got {}",
            x
        )));
    }

    let b1 = ((config.slot - 1) << 3) | ((x >> 8) as u8 & 0x07);
    let b2 = x as u8;
    let b3 =
        (config.position << 2) | sec_voltage_bit(config.voltage, 0x02) | sec_tone_bit(band, 0x01);

    let mut msg = Vec::with_capacity(if config.pin.is_some() { 5 } else { 4 });
    msg.push(if config.pin.is_some() { 0x71 } else { 0x70 });
    msg.extend([b1, b2, b3]);
    if let Some(pin) = config.pin {
        msg.push(pin);
    }

    Ok(unicable_tune(config.user_band_frequency_mhz, msg, timings))
}

fn controlled_master_sequence<const N: usize>(
    voltage: SecVoltage,
    tone: SecTone,
    msg: [u8; N],
    timings: SecTimings,
) -> Vec<SecCommand> {
    vec![
        SecCommand::SetTone(SecTone::Off),
        SecCommand::SetVoltage(voltage),
        SecCommand::Wait(timings.switch_settle),
        SecCommand::SendMasterCommand(msg.to_vec()),
        SecCommand::Wait(timings.message_gap),
        SecCommand::SetTone(tone),
    ]
}

fn sec_setup(frequency_mhz: u32, sec_sequence: Vec<SecCommand>) -> SecSetup {
    SecSetup {
        frontend_frequency_khz: frequency_mhz * 1000,
        sec_sequence,
    }
}

fn unicable_tune(frequency_mhz: u32, msg: Vec<u8>, timings: SecTimings) -> SecSetup {
    SecSetup {
        frontend_frequency_khz: frequency_mhz * 1000,
        sec_sequence: vec![
            SecCommand::SetVoltage(SecVoltage::V13),
            SecCommand::SetTone(SecTone::Off),
            SecCommand::Wait(timings.unicable_settle),
            SecCommand::SetVoltage(SecVoltage::V18),
            SecCommand::Wait(timings.message_gap),
            SecCommand::SendMasterCommand(msg),
            SecCommand::Wait(timings.unicable_hold),
            SecCommand::SetVoltage(SecVoltage::V13),
        ],
    }
}

fn sec_voltage_bit(voltage: SecVoltage, bit: u8) -> u8 {
    if voltage == SecVoltage::V18 { bit } else { 0 }
}

fn sec_tone_bit(tone: SecTone, bit: u8) -> u8 {
    if tone == SecTone::On { bit } else { 0 }
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn parse(&mut self) -> Result<Vec<SecCommand>> {
        let mut commands = Vec::new();

        while self.skip_whitespace() {
            let command = match self.current() {
                b't' => {
                    self.pos += 1;
                    SecCommand::SetTone(SecTone::Off)
                }
                b'T' => {
                    self.pos += 1;
                    SecCommand::SetTone(SecTone::On)
                }
                b'v' => {
                    self.pos += 1;
                    SecCommand::SetVoltage(SecVoltage::V13)
                }
                b'V' => {
                    self.pos += 1;
                    SecCommand::SetVoltage(SecVoltage::V18)
                }
                b'A' => {
                    self.pos += 1;
                    SecCommand::SendBurst(SecMiniCmd::A)
                }
                b'B' => {
                    self.pos += 1;
                    SecCommand::SendBurst(SecMiniCmd::B)
                }
                b'W' => {
                    self.pos += 1;
                    SecCommand::Wait(Duration::from_millis(self.parse_wait()?))
                }
                b'[' => self.parse_master_command()?,
                _ => {
                    return Err(self.error(format!(
                        "unexpected SEC DSL command '{}'",
                        self.current() as char
                    )));
                }
            };

            commands.push(command);
        }

        Ok(commands)
    }

    fn parse_wait(&mut self) -> Result<u64> {
        let start = self.pos;
        let mut value = 0u64;

        while self.pos < self.bytes.len() && self.current().is_ascii_digit() {
            value = value
                .checked_mul(10)
                .and_then(|v| v.checked_add((self.current() - b'0') as u64))
                .ok_or_else(|| self.error("wait duration is too large"))?;
            self.pos += 1;
        }

        if self.pos == start {
            return Err(self.error("wait command requires a millisecond value"));
        }

        Ok(value)
    }

    fn parse_master_command(&mut self) -> Result<SecCommand> {
        self.pos += 1; // consume '['

        let mut msg = Vec::new();

        loop {
            while self.pos < self.bytes.len() && self.current().is_ascii_whitespace() {
                self.pos += 1;
            }

            if self.pos >= self.bytes.len() {
                return Err(self.error("unterminated DiSEqC master command"));
            }

            if self.current() == b']' {
                self.pos += 1;
                break;
            }

            let high = self.parse_hex_nibble()?;
            let low = self.parse_hex_nibble()?;
            msg.push((high << 4) | low);
        }

        if !(3 ..= 6).contains(&msg.len()) {
            return Err(self.error(format!(
                "DiSEqC master command length must be 3..=6 bytes, got {}",
                msg.len()
            )));
        }

        Ok(SecCommand::SendMasterCommand(msg))
    }

    fn parse_hex_nibble(&mut self) -> Result<u8> {
        if self.pos >= self.bytes.len() {
            return Err(self.error("unterminated DiSEqC master command"));
        }

        let c = self.current();
        let nibble = match c {
            b'0' ..= b'9' => c - b'0',
            b'a' ..= b'f' => c - b'a' + 10,
            b'A' ..= b'F' => c - b'A' + 10,
            _ => {
                return Err(self.error(format!(
                    "invalid hex character '{}' in DiSEqC master command",
                    self.current() as char
                )));
            }
        };

        self.pos += 1;

        Ok(nibble)
    }

    fn skip_whitespace(&mut self) -> bool {
        while self.pos < self.bytes.len() && self.current().is_ascii_whitespace() {
            self.pos += 1;
        }

        self.pos < self.bytes.len()
    }

    fn current(&self) -> u8 {
        self.bytes[self.pos]
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::InvalidData(format!(
            "{} at byte {} in {:?}",
            message.into(),
            self.pos,
            self.input
        ))
    }
}
