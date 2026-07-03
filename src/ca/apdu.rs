//! Application Protocol Data Unit (APDU)
//!
//! en50221 8.3
//! All protocols in the Application Layer use a common Application
//! Protocol Data Unit (APDU) structure to send application data between
//! module and host or between modules.

use std::fmt;

use super::asn1;
use crate::error::{
    Error,
    Result,
};

/// Size of the application object tag preceding the asn.1 length field
pub const APDU_TAG_SIZE: usize = 3;

/// en50221 Table 58: application object tag.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApduTag(u32);

impl ApduTag {
    pub const PROFILE_ENQ: Self = Self(0x9F8010);
    pub const PROFILE: Self = Self(0x9F8011);
    pub const PROFILE_CHANGE: Self = Self(0x9F8012);
    pub const APPLICATION_INFO_ENQ: Self = Self(0x9F8020);
    pub const APPLICATION_INFO: Self = Self(0x9F8021);
    pub const CLOSE_MMI: Self = Self(0x9F8800);
    pub const DISPLAY_CONTROL: Self = Self(0x9F8801);
    pub const DISPLAY_REPLY: Self = Self(0x9F8802);
    pub const TEXT_LAST: Self = Self(0x9F8803);
    pub const ENQ: Self = Self(0x9F8807);
    pub const ANSW: Self = Self(0x9F8808);
    pub const MENU_LAST: Self = Self(0x9F8809);
    pub const MENU_ANSW: Self = Self(0x9F880B);
    pub const LIST_LAST: Self = Self(0x9F880C);

    /// Wraps a raw tag value
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Raw tag value
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ApduTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::PROFILE_ENQ => "PROFILE_ENQ",
            Self::PROFILE => "PROFILE",
            Self::PROFILE_CHANGE => "PROFILE_CHANGE",
            Self::APPLICATION_INFO_ENQ => "APPLICATION_INFO_ENQ",
            Self::APPLICATION_INFO => "APPLICATION_INFO",
            Self::CLOSE_MMI => "CLOSE_MMI",
            Self::DISPLAY_CONTROL => "DISPLAY_CONTROL",
            Self::DISPLAY_REPLY => "DISPLAY_REPLY",
            Self::TEXT_LAST => "TEXT_LAST",
            Self::ENQ => "ENQ",
            Self::ANSW => "ANSW",
            Self::MENU_LAST => "MENU_LAST",
            Self::MENU_ANSW => "MENU_ANSW",
            Self::LIST_LAST => "LIST_LAST",
            _ => return write!(f, "ApduTag(0x{:06X})", self.0),
        };

        write!(f, "ApduTag({})", name)
    }
}

/// Parsed APDU: 3-byte big-endian tag + asn.1 length + body
#[derive(Debug, PartialEq, Eq)]
pub struct Apdu<'a> {
    /// application object tag
    pub tag: ApduTag,
    /// payload, delimited by the declared asn.1 length
    pub body: &'a [u8],
}

/// Parses one APDU
pub fn parse(data: &[u8]) -> Result<Apdu<'_>> {
    let tag_bytes = data
        .get(.. APDU_TAG_SIZE)
        .ok_or_else(|| Error::InvalidData("ca apdu is too short".to_owned()))?;
    let tag = ApduTag::new(
        (u32::from(tag_bytes[0]) << 16) | (u32::from(tag_bytes[1]) << 8) | u32::from(tag_bytes[2]),
    );

    let (length, consumed) = asn1::decode(&data[APDU_TAG_SIZE ..])?;
    let body_start = APDU_TAG_SIZE + consumed;
    let body = data
        .get(body_start .. body_start + usize::from(length))
        .ok_or_else(|| Error::InvalidData(format!("ca apdu {:?} body is truncated", tag)))?;

    Ok(Apdu { tag, body })
}

/// Appends `tag + asn1(body.len()) + body` to `out`
///
/// The body length must fit the asn.1 u16 length coding.
pub fn build(out: &mut Vec<u8>, tag: ApduTag, body: &[u8]) {
    let tag = tag.raw();
    out.push((tag >> 16) as u8);
    out.push((tag >> 8) as u8);
    out.push(tag as u8);
    asn1::encode(body.len() as u16, out);
    out.extend_from_slice(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build() {
        let mut out = Vec::new();
        build(&mut out, ApduTag::PROFILE_ENQ, &[]);
        assert_eq!(out, vec![0x9F, 0x80, 0x10, 0x00]);

        let mut out = Vec::new();
        build(&mut out, ApduTag::MENU_ANSW, &[0x01]);
        assert_eq!(out, vec![0x9F, 0x88, 0x0B, 0x01, 0x01]);

        // asn.1 boundary: 128-byte body uses the 0x81 indicator
        let mut out = Vec::new();
        build(&mut out, ApduTag::PROFILE, &[0x42; 128]);
        assert_eq!(&out[.. 5], &[0x9F, 0x80, 0x11, 0x81, 0x80]);
        assert_eq!(out.len(), 5 + 128);
    }

    #[test]
    fn test_parse_roundtrip() {
        let mut data = Vec::new();
        build(
            &mut data,
            ApduTag::APPLICATION_INFO,
            &[0x01, 0x06, 0x00, 0x12, 0x34],
        );

        let apdu = parse(&data).unwrap();
        assert_eq!(apdu.tag, ApduTag::APPLICATION_INFO);
        assert_eq!(apdu.body, &[0x01, 0x06, 0x00, 0x12, 0x34]);
    }

    #[test]
    fn test_parse_ignores_trailing_bytes() {
        let apdu = parse(&[0x9F, 0x80, 0x10, 0x01, 0xAA, 0xBB, 0xCC]).unwrap();
        assert_eq!(apdu.tag, ApduTag::PROFILE_ENQ);
        assert_eq!(apdu.body, &[0xAA]);
    }

    #[test]
    fn test_parse_errors() {
        // too short for the tag
        assert!(parse(&[0x9F, 0x80]).is_err());
        // missing length field
        assert!(parse(&[0x9F, 0x80, 0x10]).is_err());
        // declared length exceeds the remaining bytes
        assert!(parse(&[0x9F, 0x80, 0x10, 0x02, 0xAA]).is_err());
        // truncated multi-byte length field
        assert!(parse(&[0x9F, 0x80, 0x10, 0x82, 0x01]).is_err());
    }

    #[test]
    fn test_tag_debug() {
        assert_eq!(
            format!("{:?}", ApduTag::PROFILE_ENQ),
            "ApduTag(PROFILE_ENQ)"
        );
        assert_eq!(format!("{:?}", ApduTag::new(0x9F8888)), "ApduTag(0x9F8888)");
    }
}
