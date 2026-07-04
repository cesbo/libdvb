//! Session Protocol Data Unit (SPDU)
//!
//! en50221 7.2.4
//! The session layer uses a Session Protocol Data Unit (SPDU) structure
//! to exchange data at session level either from the host to the module
//! or from the module to the host.

use std::fmt;

pub use ca_spdu_status::*;

use super::resource::ResourceId;
use crate::error::{
    Error,
    Result,
};

/// Size of the session_number SPDU header preceding the APDU bytes
pub const SPDU_HEADER_SIZE: usize = 4;

/// en50221 7.2.7: session tag (one byte on the wire)
///
/// A newtype instead of an enum: the module may send any tag value, so
/// unknown tags must stay representable. The associated constants list
/// the tags of the session protocol.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpduTag(u8);

impl SpduTag {
    pub const SESSION_NUMBER: Self = Self(0x90);
    pub const OPEN_SESSION_REQUEST: Self = Self(0x91);
    pub const OPEN_SESSION_RESPONSE: Self = Self(0x92);
    /// host -> module direction only, never parsed nor built by this host
    pub const CREATE_SESSION: Self = Self(0x93);
    pub const CREATE_SESSION_RESPONSE: Self = Self(0x94);
    pub const CLOSE_SESSION_REQUEST: Self = Self(0x95);
    pub const CLOSE_SESSION_RESPONSE: Self = Self(0x96);

    /// Wraps a raw tag value
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Raw tag value
    pub const fn raw(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for SpduTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::SESSION_NUMBER => "SESSION_NUMBER",
            Self::OPEN_SESSION_REQUEST => "OPEN_SESSION_REQUEST",
            Self::OPEN_SESSION_RESPONSE => "OPEN_SESSION_RESPONSE",
            Self::CREATE_SESSION => "CREATE_SESSION",
            Self::CREATE_SESSION_RESPONSE => "CREATE_SESSION_RESPONSE",
            Self::CLOSE_SESSION_REQUEST => "CLOSE_SESSION_REQUEST",
            Self::CLOSE_SESSION_RESPONSE => "CLOSE_SESSION_RESPONSE",
            _ => return write!(f, "SpduTag(0x{:02X})", self.0),
        };

        write!(f, "SpduTag({})", name)
    }
}

/// en50221 Table 7: Open Session Status values
mod ca_spdu_status {
    pub const SS_OK: u8 = 0x00;
    pub const SS_NOT_ALLOCATED: u8 = 0xF0;
}

/// Parsed SPDU received from the module (en50221 7.2)
#[derive(Debug, PartialEq, Eq)]
pub enum Spdu<'a> {
    /// `[0x90, 0x02, sid(2), APDU...]` - the APDU bytes follow the header
    SessionNumber { session_id: u16, apdu: &'a [u8] },
    /// `[0x91, 0x04, RI(4)]` - total size 6
    OpenSessionRequest { resource_id: ResourceId },
    /// `[0x94, 0x07, status, RI(4), sid(2)]` - total size 9
    CreateSessionResponse {
        status: u8,
        resource_id: ResourceId,
        session_id: u16,
    },
    /// `[0x95, 0x02, sid(2)]` - total size 4
    CloseSessionRequest { session_id: u16 },
    /// `[0x96, 0x03, status, sid(2)]` - total size 5
    CloseSessionResponse { status: u8, session_id: u16 },
}

fn read_u16(data: &[u8]) -> u16 {
    (u16::from(data[0]) << 8) | u16::from(data[1])
}

fn read_u32(data: &[u8]) -> u32 {
    (u32::from(data[0]) << 24)
        | (u32::from(data[1]) << 16)
        | (u32::from(data[2]) << 8)
        | u32::from(data[3])
}

/// Parses a complete reassembled SPDU
pub fn parse(data: &[u8]) -> Result<Spdu<'_>> {
    let tag = match data.first() {
        Some(&tag) => SpduTag::new(tag),
        None => {
            return Err(Error::InvalidData("ca spdu is empty".to_owned()));
        }
    };

    let check = |size: usize, exact: bool| -> Result<()> {
        let size_ok = if exact {
            data.len() == size
        } else {
            data.len() >= size
        };
        if size_ok && data[1] == (size - 2) as u8 {
            Ok(())
        } else {
            Err(Error::InvalidData(format!(
                "ca spdu invalid size for tag {:?}",
                tag
            )))
        }
    };

    match tag {
        SpduTag::SESSION_NUMBER => {
            check(SPDU_HEADER_SIZE, false)?;
            Ok(Spdu::SessionNumber {
                session_id: read_u16(&data[2 .. 4]),
                apdu: &data[SPDU_HEADER_SIZE ..],
            })
        }
        SpduTag::OPEN_SESSION_REQUEST => {
            check(6, true)?;
            Ok(Spdu::OpenSessionRequest {
                resource_id: ResourceId::new(read_u32(&data[2 .. 6])),
            })
        }
        SpduTag::CREATE_SESSION_RESPONSE => {
            check(9, true)?;
            Ok(Spdu::CreateSessionResponse {
                status: data[2],
                resource_id: ResourceId::new(read_u32(&data[3 .. 7])),
                session_id: read_u16(&data[7 .. 9]),
            })
        }
        SpduTag::CLOSE_SESSION_REQUEST => {
            check(4, true)?;
            Ok(Spdu::CloseSessionRequest {
                session_id: read_u16(&data[2 .. 4]),
            })
        }
        SpduTag::CLOSE_SESSION_RESPONSE => {
            check(5, true)?;
            Ok(Spdu::CloseSessionResponse {
                status: data[2],
                session_id: read_u16(&data[3 .. 5]),
            })
        }
        tag => Err(Error::InvalidData(format!("ca spdu unknown tag {:?}", tag))),
    }
}

/// `[0x90, 0x02, sid(2)]` - the header prepended to outgoing APDUs
pub fn build_session_number(session_id: u16) -> Vec<u8> {
    vec![
        SpduTag::SESSION_NUMBER.raw(),
        2,
        (session_id >> 8) as u8,
        session_id as u8,
    ]
}

/// `[0x92, 0x07, status, RI(4), sid(2)]` - total size 9
pub fn build_open_session_response(
    status: u8,
    resource_id: ResourceId,
    session_id: u16,
) -> Vec<u8> {
    let resource_id = resource_id.raw();
    vec![
        SpduTag::OPEN_SESSION_RESPONSE.raw(),
        7,
        status,
        (resource_id >> 24) as u8,
        (resource_id >> 16) as u8,
        (resource_id >> 8) as u8,
        resource_id as u8,
        (session_id >> 8) as u8,
        session_id as u8,
    ]
}

/// `[0x95, 0x02, sid(2)]` - total size 4
pub fn build_close_session_request(session_id: u16) -> Vec<u8> {
    vec![
        SpduTag::CLOSE_SESSION_REQUEST.raw(),
        2,
        (session_id >> 8) as u8,
        session_id as u8,
    ]
}

/// `[0x96, 0x03, status, sid(2)]` - total size 5
pub fn build_close_session_response(status: u8, session_id: u16) -> Vec<u8> {
    vec![
        SpduTag::CLOSE_SESSION_RESPONSE.raw(),
        3,
        status,
        (session_id >> 8) as u8,
        session_id as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_session_number() {
        assert_eq!(build_session_number(1), vec![0x90, 0x02, 0x00, 0x01]);
        assert_eq!(build_session_number(0x1234), vec![0x90, 0x02, 0x12, 0x34]);
    }

    #[test]
    fn test_build_open_session_response() {
        assert_eq!(
            build_open_session_response(SS_OK, ResourceId::RESOURCE_MANAGER, 1),
            vec![0x92, 0x07, 0x00, 0x00, 0x01, 0x00, 0x41, 0x00, 0x01]
        );
        assert_eq!(
            build_open_session_response(SS_NOT_ALLOCATED, ResourceId::new(0x00FF_0041), 0),
            vec![0x92, 0x07, 0xF0, 0x00, 0xFF, 0x00, 0x41, 0x00, 0x00]
        );
    }

    #[test]
    fn test_build_close_session() {
        assert_eq!(build_close_session_request(3), vec![0x95, 0x02, 0x00, 0x03]);
        assert_eq!(
            build_close_session_response(SS_OK, 3),
            vec![0x96, 0x03, 0x00, 0x00, 0x03]
        );
    }

    #[test]
    fn test_parse_session_number() {
        let data = [0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x10, 0x00];
        assert_eq!(
            parse(&data).unwrap(),
            Spdu::SessionNumber {
                session_id: 1,
                apdu: &[0x9F, 0x80, 0x10, 0x00],
            }
        );

        // empty APDU tail is a parse-level pass; dispatch decides
        let data = [0x90, 0x02, 0x00, 0x01];
        assert!(parse(&data).is_ok());

        // wrong length field
        assert!(parse(&[0x90, 0x03, 0x00, 0x01]).is_err());
    }

    #[test]
    fn test_parse_open_session_request() {
        let data = [0x91, 0x04, 0x00, 0x01, 0x00, 0x41];
        assert_eq!(
            parse(&data).unwrap(),
            Spdu::OpenSessionRequest {
                resource_id: ResourceId::RESOURCE_MANAGER,
            }
        );

        // wrong total size and wrong length field
        assert!(parse(&[0x91, 0x04, 0x00, 0x01, 0x00]).is_err());
        assert!(parse(&[0x91, 0x04, 0x00, 0x01, 0x00, 0x41, 0x00]).is_err());
        assert!(parse(&[0x91, 0x05, 0x00, 0x01, 0x00, 0x41]).is_err());
    }

    #[test]
    fn test_parse_session_responses() {
        let data = [0x94, 0x07, 0x00, 0x00, 0x40, 0x00, 0x41, 0x00, 0x02];
        assert_eq!(
            parse(&data).unwrap(),
            Spdu::CreateSessionResponse {
                status: SS_OK,
                resource_id: ResourceId::MMI,
                session_id: 2,
            }
        );

        assert_eq!(
            parse(&[0x95, 0x02, 0x00, 0x03]).unwrap(),
            Spdu::CloseSessionRequest { session_id: 3 }
        );
        assert_eq!(
            parse(&[0x96, 0x03, 0x00, 0x00, 0x03]).unwrap(),
            Spdu::CloseSessionResponse {
                status: SS_OK,
                session_id: 3,
            }
        );

        assert!(parse(&[0x94, 0x07, 0x00]).is_err());
        assert!(parse(&[0x95, 0x02, 0x00]).is_err());
        assert!(parse(&[0x96, 0x03, 0x00, 0x00]).is_err());
    }

    #[test]
    fn test_parse_rejects_unknown_and_empty() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[0x93, 0x06, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(parse(&[0xFF, 0x00]).is_err());
    }

    #[test]
    fn test_tag_debug() {
        assert_eq!(
            format!("{:?}", SpduTag::SESSION_NUMBER),
            "SpduTag(SESSION_NUMBER)"
        );
        assert_eq!(format!("{:?}", SpduTag::new(0xFF)), "SpduTag(0xFF)");
    }
}
