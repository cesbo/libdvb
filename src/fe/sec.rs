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
pub fn parse_sec_sequence(input: &str) -> Result<Vec<SecCommand>> {
    let mut parser = Parser::new(input);
    parser.parse()
}

/// Builds a DiSEqC 1.0 committed-switch sequence for ports 1..=4.
pub fn diseqc_1_0_sequence(
    port: u8,
    voltage: SecVoltage,
    tone: SecTone,
) -> Result<Vec<SecCommand>> {
    if !(1 ..= 4).contains(&port) {
        return Err(Error::InvalidData(format!(
            "DiSEqC 1.0 port must be in range 1..=4, got {}",
            port
        )));
    }

    let port = port - 1;
    let data = 0xF0
        | (port << 2)
        | (if voltage == SecVoltage::V18 {
            0x02
        } else {
            0x00
        })
        | (if tone == SecTone::On { 0x01 } else { 0x00 });

    Ok(controlled_master_sequence(
        voltage,
        tone,
        [0xE0, 0x10, 0x38, data],
    ))
}

/// Builds a DiSEqC 1.1 uncommitted-switch sequence for ports 1..=16.
pub fn diseqc_1_1_sequence(
    port: u8,
    voltage: SecVoltage,
    tone: SecTone,
) -> Result<Vec<SecCommand>> {
    if !(1 ..= 16).contains(&port) {
        return Err(Error::InvalidData(format!(
            "DiSEqC 1.1 port must be in range 1..=16, got {}",
            port
        )));
    }

    let data = 0xF0 | (port - 1);

    Ok(controlled_master_sequence(
        voltage,
        tone,
        [0xE0, 0x10, 0x39, data],
    ))
}

/// Builds a toneburst A/B sequence.
pub fn toneburst_sequence(
    burst: SecMiniCmd,
    voltage: SecVoltage,
    tone: SecTone,
) -> Vec<SecCommand> {
    vec![
        SecCommand::SetTone(SecTone::Off),
        SecCommand::SetVoltage(voltage),
        SecCommand::Wait(Duration::from_millis(15)),
        SecCommand::SendBurst(burst),
        SecCommand::Wait(Duration::from_millis(15)),
        SecCommand::SetTone(tone),
    ]
}

fn controlled_master_sequence<const N: usize>(
    voltage: SecVoltage,
    tone: SecTone,
    msg: [u8; N],
) -> Vec<SecCommand> {
    vec![
        SecCommand::SetTone(SecTone::Off),
        SecCommand::SetVoltage(voltage),
        SecCommand::Wait(Duration::from_millis(200)),
        SecCommand::SendMasterCommand(msg.to_vec()),
        SecCommand::Wait(Duration::from_millis(15)),
        SecCommand::SetTone(tone),
    ]
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

        let len = msg.len();
        if len < 3 || len > 6 {
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
