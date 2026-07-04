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
    pub const ENTER_MENU: Self = Self(0x9F8022);
    pub const TUNE: Self = Self(0x9F8400);
    pub const REPLACE: Self = Self(0x9F8401);
    pub const CLEAR_REPLACE: Self = Self(0x9F8402);
    pub const ASK_RELEASE: Self = Self(0x9F8403);
    pub const DATE_TIME_ENQ: Self = Self(0x9F8440);
    pub const DATE_TIME: Self = Self(0x9F8441);
    pub const CLOSE_MMI: Self = Self(0x9F8800);
    pub const DISPLAY_CONTROL: Self = Self(0x9F8801);
    pub const DISPLAY_REPLY: Self = Self(0x9F8802);
    pub const TEXT_LAST: Self = Self(0x9F8803);
    pub const TEXT_MORE: Self = Self(0x9F8804);
    pub const ENQ: Self = Self(0x9F8807);
    pub const ANSW: Self = Self(0x9F8808);
    pub const MENU_LAST: Self = Self(0x9F8809);
    pub const MENU_MORE: Self = Self(0x9F880A);
    pub const MENU_ANSW: Self = Self(0x9F880B);
    pub const LIST_LAST: Self = Self(0x9F880C);
    pub const LIST_MORE: Self = Self(0x9F880D);

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
            Self::ENTER_MENU => "ENTER_MENU",
            Self::TUNE => "TUNE",
            Self::REPLACE => "REPLACE",
            Self::CLEAR_REPLACE => "CLEAR_REPLACE",
            Self::ASK_RELEASE => "ASK_RELEASE",
            Self::DATE_TIME_ENQ => "DATE_TIME_ENQ",
            Self::DATE_TIME => "DATE_TIME",
            Self::CLOSE_MMI => "CLOSE_MMI",
            Self::DISPLAY_CONTROL => "DISPLAY_CONTROL",
            Self::DISPLAY_REPLY => "DISPLAY_REPLY",
            Self::TEXT_LAST => "TEXT_LAST",
            Self::TEXT_MORE => "TEXT_MORE",
            Self::ENQ => "ENQ",
            Self::ANSW => "ANSW",
            Self::MENU_LAST => "MENU_LAST",
            Self::MENU_MORE => "MENU_MORE",
            Self::MENU_ANSW => "MENU_ANSW",
            Self::LIST_LAST => "LIST_LAST",
            Self::LIST_MORE => "LIST_MORE",
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

/// Parses one APDU at the start of `data` and returns it with its full
/// encoded size (tag + asn.1 length field + body)
pub fn parse_at(data: &[u8]) -> Result<(Apdu<'_>, usize)> {
    let tag_bytes = data
        .get(.. APDU_TAG_SIZE)
        .ok_or_else(|| Error::InvalidData("ca apdu is too short".to_owned()))?;
    let tag = ApduTag::new(
        (u32::from(tag_bytes[0]) << 16) | (u32::from(tag_bytes[1]) << 8) | u32::from(tag_bytes[2]),
    );

    let (length, consumed) = asn1::decode(&data[APDU_TAG_SIZE ..])?;
    let body_start = APDU_TAG_SIZE + consumed;
    let body_end = body_start + usize::from(length);
    let body = data
        .get(body_start .. body_end)
        .ok_or_else(|| Error::InvalidData(format!("ca apdu {:?} body is truncated", tag)))?;

    Ok((Apdu { tag, body }, body_end))
}

/// Iterator over consecutive APDUs sharing one SPDU payload,
/// created with [`iter`]
pub struct ApduIter<'a> {
    data: &'a [u8],
}

/// Iterates over consecutive APDUs: a module may pack several APDUs into
/// a single session_number SPDU. A parse error is yielded once and stops
/// the iteration.
pub fn iter(data: &[u8]) -> ApduIter<'_> {
    ApduIter { data }
}

impl<'a> Iterator for ApduIter<'a> {
    type Item = Result<Apdu<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() {
            return None;
        }

        match parse_at(self.data) {
            Ok((apdu, size)) => {
                self.data = &self.data[size ..];
                Some(Ok(apdu))
            }
            Err(e) => {
                self.data = &[];
                Some(Err(e))
            }
        }
    }
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

        let (apdu, size) = parse_at(&data).unwrap();
        assert_eq!(apdu.tag, ApduTag::APPLICATION_INFO);
        assert_eq!(apdu.body, &[0x01, 0x06, 0x00, 0x12, 0x34]);
        assert_eq!(size, data.len());
    }

    #[test]
    fn test_parse_ignores_trailing_bytes() {
        let (apdu, size) = parse_at(&[0x9F, 0x80, 0x10, 0x01, 0xAA, 0xBB, 0xCC]).unwrap();
        assert_eq!(apdu.tag, ApduTag::PROFILE_ENQ);
        assert_eq!(apdu.body, &[0xAA]);
        assert_eq!(size, 5);
    }

    #[test]
    fn test_parse_errors() {
        // too short for the tag
        assert!(parse_at(&[0x9F, 0x80]).is_err());
        // missing length field
        assert!(parse_at(&[0x9F, 0x80, 0x10]).is_err());
        // declared length exceeds the remaining bytes
        assert!(parse_at(&[0x9F, 0x80, 0x10, 0x02, 0xAA]).is_err());
        // truncated multi-byte length field
        assert!(parse_at(&[0x9F, 0x80, 0x10, 0x82, 0x01]).is_err());
    }

    #[test]
    fn test_iter() {
        let mut data = Vec::new();
        build(&mut data, ApduTag::PROFILE_ENQ, &[]);
        build(&mut data, ApduTag::MENU_ANSW, &[0x01]);
        build(&mut data, ApduTag::PROFILE, &[0x42; 128]);

        let mut iter = iter(&data);
        assert_eq!(
            iter.next().unwrap().unwrap(),
            Apdu {
                tag: ApduTag::PROFILE_ENQ,
                body: &[],
            }
        );
        assert_eq!(
            iter.next().unwrap().unwrap(),
            Apdu {
                tag: ApduTag::MENU_ANSW,
                body: &[0x01],
            }
        );
        assert_eq!(
            iter.next().unwrap().unwrap(),
            Apdu {
                tag: ApduTag::PROFILE,
                body: &[0x42; 128],
            }
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_iter_empty() {
        assert!(iter(&[]).next().is_none());
    }

    #[test]
    fn test_iter_stops_on_error() {
        let mut data = Vec::new();
        build(&mut data, ApduTag::PROFILE_ENQ, &[]);
        // truncated second APDU
        data.extend_from_slice(&[0x9F, 0x80]);

        let mut iter = iter(&data);
        assert!(iter.next().unwrap().is_ok());
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none());
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
