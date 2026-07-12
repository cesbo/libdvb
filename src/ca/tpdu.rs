//! Transport Protocol Data Unit (TPDU)
//!
//! en50221 7.1
//! The Transport Layer of the Command Interface operates on top of a Link
//! Layer provided by the particular physical implementation used.
//! The transport protocol is a command-response protocol where the host
//! sends a command to the module, using a Command Transport Protocol Data
//! Unit (C_TPDU) and waits for a response from the module with a Response
//! Transport Protocol Data Unit (R_TPDU).
//! The module cannot initiate communication: it must wait for the host to
//! poll it or send it data first.

use std::fmt;

use super::asn1;
use crate::error::{
    Error,
    Result,
};

/// Link frame budget in bytes: the largest frame read from or written to
/// the CA device (matches the legacy MAX_TPDU_SIZE)
pub const MAX_TPDU_SIZE: usize = 2048;

/// Largest data payload that fits one framed TPDU:
/// MAX_TPDU_SIZE - 3 (link header) - 3 (asn.1 worst case) - 1 (t_c_id)
pub const MAX_TPDU_DATA: usize = MAX_TPDU_SIZE - 7;

/// Status byte bit: the module has data pending and the host must send
/// [`TpduTag::RCV`] to fetch it (en50221 A.4.1.3)
pub const DATA_INDICATOR: u8 = 0x80;

/// en50221 A.4.1.13: transport tag (one byte on the wire)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TpduTag(u8);

impl TpduTag {
    pub const SB: Self = Self(0x80);
    pub const RCV: Self = Self(0x81);
    pub const CREATE_TC: Self = Self(0x82);
    pub const CTC_REPLY: Self = Self(0x83);
    pub const DELETE_TC: Self = Self(0x84);
    pub const DTC_REPLY: Self = Self(0x85);
    pub const REQUEST_TC: Self = Self(0x86);
    pub const NEW_TC: Self = Self(0x87);
    pub const TC_ERROR: Self = Self(0x88);
    pub const DATA_LAST: Self = Self(0xA0);
    pub const DATA_MORE: Self = Self(0xA1);

    /// Wraps a raw tag value
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Raw tag value
    pub const fn raw(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for TpduTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::SB => "SB",
            Self::RCV => "RCV",
            Self::CREATE_TC => "CREATE_TC",
            Self::CTC_REPLY => "CTC_REPLY",
            Self::DELETE_TC => "DELETE_TC",
            Self::DTC_REPLY => "DTC_REPLY",
            Self::REQUEST_TC => "REQUEST_TC",
            Self::NEW_TC => "NEW_TC",
            Self::TC_ERROR => "TC_ERROR",
            Self::DATA_LAST => "DATA_LAST",
            Self::DATA_MORE => "DATA_MORE",
            _ => return write!(f, "TpduTag(0x{:02X})", self.0),
        };

        write!(f, "TpduTag({})", name)
    }
}

/// Builds a complete C_TPDU link frame for the given tag
pub fn build(slot_id: u8, tag: TpduTag, data: &[u8]) -> Result<Vec<u8>> {
    let t_c_id = slot_id
        .checked_add(1)
        .ok_or_else(|| Error::InvalidProperty(format!("ca tpdu invalid slot id {}", slot_id)))?;

    match tag {
        TpduTag::RCV
        | TpduTag::CREATE_TC
        | TpduTag::CTC_REPLY
        | TpduTag::DELETE_TC
        | TpduTag::DTC_REPLY
        | TpduTag::REQUEST_TC => Ok(vec![slot_id, t_c_id, tag.raw(), 1, t_c_id]),
        TpduTag::NEW_TC | TpduTag::TC_ERROR => match data.first() {
            Some(&extra) => Ok(vec![slot_id, t_c_id, tag.raw(), 2, t_c_id, extra]),
            None => Err(Error::InvalidProperty(format!(
                "ca tpdu tag {:?} requires one data byte",
                tag
            ))),
        },
        TpduTag::DATA_LAST | TpduTag::DATA_MORE => {
            if data.len() > MAX_TPDU_DATA {
                return Err(Error::InvalidProperty(format!(
                    "ca tpdu data payload is too large: {} bytes",
                    data.len()
                )));
            }

            let mut frame = Vec::with_capacity(7 + data.len());
            frame.push(slot_id);
            frame.push(t_c_id);
            frame.push(tag.raw());
            asn1::encode(data.len() as u16 + 1, &mut frame);
            frame.push(t_c_id);
            frame.extend_from_slice(data);

            Ok(frame)
        }
        _ => Err(Error::InvalidProperty(format!(
            "ca tpdu unsupported tag {:?}",
            tag
        ))),
    }
}

/// Parsed R_TPDU link frame
#[derive(Debug)]
pub struct RTpdu<'a> {
    pub slot_id: u8,
    pub tag: TpduTag,
    pub body: &'a [u8],
    pub data_indicator: bool,
}

pub fn frame_slot_id(frame: &[u8], slots_num: u8) -> Option<u8> {
    let slot_id = frame.first().copied()?;
    (slot_id < slots_num).then_some(slot_id)
}

/// Parses one R_TPDU read from the link
pub fn parse(frame: &[u8], slots_num: u8) -> Result<RTpdu<'_>> {
    if frame.len() < 5 {
        return Err(Error::InvalidData(format!(
            "ca tpdu frame is too short: {} bytes",
            frame.len()
        )));
    }

    let t_c_id = frame[1];
    if t_c_id == 0 || t_c_id > slots_num {
        return Err(Error::InvalidData(format!(
            "ca tpdu invalid t_c_id {}",
            t_c_id
        )));
    }
    let slot_id = t_c_id - 1;
    if frame[0] != slot_id {
        return Err(Error::InvalidData(format!(
            "ca slot id {} does not match t_c_id {}",
            frame[0], t_c_id
        )));
    }
    let tag = TpduTag::new(frame[2]);

    // the last 4 bytes must be the status object [SB, 2, t_c_id, status]
    let trailer = &frame[frame.len() - 4 ..];
    if trailer[0] != TpduTag::SB.raw() || trailer[1] != 2 || trailer[2] != t_c_id {
        return Err(Error::InvalidData(format!(
            "ca slot {}: invalid tpdu status trailer",
            slot_id
        )));
    }
    let data_indicator = (trailer[3] & DATA_INDICATOR) != 0;

    let body: &[u8] = match tag {
        TpduTag::DATA_LAST | TpduTag::DATA_MORE => {
            let (declared_len, asn1_size) = asn1::decode(&frame[3 ..])?;
            let declared_len = usize::from(declared_len);
            if declared_len < 1 || 3 + asn1_size + declared_len + 4 != frame.len() {
                return Err(Error::InvalidData(format!(
                    "ca slot {}: tpdu length field mismatch",
                    slot_id
                )));
            }
            if frame[3 + asn1_size] != t_c_id {
                return Err(Error::InvalidData(format!(
                    "ca slot {}: tpdu t_c_id byte mismatch",
                    slot_id
                )));
            }

            let body_start = 3 + asn1_size + 1;
            &frame[body_start .. body_start + declared_len - 1]
        }
        TpduTag::SB => {
            // the status object is the whole frame body
            if frame.len() != 6 {
                return Err(Error::InvalidData(format!(
                    "ca slot {}: invalid tpdu status frame size",
                    slot_id
                )));
            }
            &[]
        }
        TpduTag::CTC_REPLY | TpduTag::DTC_REPLY | TpduTag::REQUEST_TC => {
            if frame.len() != 9 || frame[3] != 1 || frame[4] != t_c_id {
                return Err(Error::InvalidData(format!(
                    "ca slot {}: invalid tpdu frame for tag {:?}",
                    slot_id, tag
                )));
            }
            &[]
        }
        TpduTag::NEW_TC | TpduTag::TC_ERROR => {
            if frame.len() != 10 || frame[3] != 2 || frame[4] != t_c_id {
                return Err(Error::InvalidData(format!(
                    "ca slot {}: invalid tpdu frame for tag {:?}",
                    slot_id, tag
                )));
            }
            &[]
        }
        _ => {
            return Err(Error::InvalidData(format!(
                "ca slot {}: unknown tpdu tag {:?}",
                slot_id, tag
            )));
        }
    };

    Ok(RTpdu {
        slot_id,
        tag,
        body,
        data_indicator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_simple_tags() {
        assert_eq!(
            build(0, TpduTag::CREATE_TC, &[]).unwrap(),
            vec![0x00, 0x01, 0x82, 0x01, 0x01]
        );
        assert_eq!(
            build(0, TpduTag::RCV, &[]).unwrap(),
            vec![0x00, 0x01, 0x81, 0x01, 0x01]
        );
        assert_eq!(
            build(0, TpduTag::DTC_REPLY, &[]).unwrap(),
            vec![0x00, 0x01, 0x85, 0x01, 0x01]
        );
        // second slot: t_c_id = slot_id + 1
        assert_eq!(
            build(1, TpduTag::CREATE_TC, &[]).unwrap(),
            vec![0x01, 0x02, 0x82, 0x01, 0x02]
        );
    }

    #[test]
    fn test_build_new_tc() {
        assert_eq!(
            build(0, TpduTag::NEW_TC, &[0x42]).unwrap(),
            vec![0x00, 0x01, 0x87, 0x02, 0x01, 0x42]
        );
        assert!(build(0, TpduTag::NEW_TC, &[]).is_err());
    }

    #[test]
    fn test_build_data_frames() {
        // empty poll frame
        assert_eq!(
            build(0, TpduTag::DATA_LAST, &[]).unwrap(),
            vec![0x00, 0x01, 0xA0, 0x01, 0x01]
        );

        // small payload: single-byte asn.1 length
        assert_eq!(
            build(0, TpduTag::DATA_LAST, &[0xAA, 0xBB, 0xCC, 0xDD]).unwrap(),
            vec![0x00, 0x01, 0xA0, 0x05, 0x01, 0xAA, 0xBB, 0xCC, 0xDD]
        );

        // asn.1 boundary: 126-byte payload still fits one length byte
        let frame = build(0, TpduTag::DATA_MORE, &[0x55; 126]).unwrap();
        assert_eq!(&frame[.. 5], &[0x00, 0x01, 0xA1, 0x7F, 0x01]);
        assert_eq!(frame.len(), 5 + 126);

        // 127-byte payload: length 128 needs the 0x81 indicator
        let frame = build(0, TpduTag::DATA_MORE, &[0x55; 127]).unwrap();
        assert_eq!(&frame[.. 6], &[0x00, 0x01, 0xA1, 0x81, 0x80, 0x01]);

        // largest payload: frame is exactly MAX_TPDU_SIZE
        let frame = build(0, TpduTag::DATA_MORE, &[0x55; MAX_TPDU_DATA]).unwrap();
        assert_eq!(&frame[.. 7], &[0x00, 0x01, 0xA1, 0x82, 0x07, 0xFA, 0x01]);
        assert_eq!(frame.len(), MAX_TPDU_SIZE);
    }

    #[test]
    fn test_build_rejects_oversize_and_unknown() {
        assert!(build(0, TpduTag::DATA_LAST, &[0u8; MAX_TPDU_DATA + 1]).is_err());
        assert!(build(0, TpduTag::new(0x99), &[]).is_err());
        assert!(build(255, TpduTag::CREATE_TC, &[]).is_err());
    }

    #[test]
    fn test_parse_status_frame() {
        let parsed = parse(&[0x00, 0x01, 0x80, 0x02, 0x01, 0x00], 1).unwrap();
        assert_eq!(parsed.slot_id, 0);
        assert_eq!(parsed.tag, TpduTag::SB);
        assert!(parsed.body.is_empty());
        assert!(!parsed.data_indicator);

        let parsed = parse(&[0x00, 0x01, 0x80, 0x02, 0x01, 0x80], 1).unwrap();
        assert!(parsed.data_indicator);
    }

    #[test]
    fn test_parse_ctc_reply() {
        let frame = [0x00, 0x01, 0x83, 0x01, 0x01, 0x80, 0x02, 0x01, 0x00];
        let parsed = parse(&frame, 1).unwrap();
        assert_eq!(parsed.slot_id, 0);
        assert_eq!(parsed.tag, TpduTag::CTC_REPLY);
        assert!(parsed.body.is_empty());

        // wrong fixed size
        let frame = [0x00, 0x01, 0x83, 0x01, 0x01, 0xFF, 0x80, 0x02, 0x01, 0x00];
        assert!(parse(&frame, 1).is_err());
    }

    #[test]
    fn test_parse_data_frame() {
        // body is delimited by the declared length, the trailer stays out
        let frame = [
            0x00, 0x01, 0xA0, 0x05, 0x01, 0xAA, 0xBB, 0xCC, 0xDD, 0x80, 0x02, 0x01, 0x00,
        ];
        let parsed = parse(&frame, 1).unwrap();
        assert_eq!(parsed.tag, TpduTag::DATA_LAST);
        assert_eq!(parsed.body, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert!(!parsed.data_indicator);

        // empty data frame (poll response shape)
        let frame = [0x00, 0x01, 0xA0, 0x01, 0x01, 0x80, 0x02, 0x01, 0x80];
        let parsed = parse(&frame, 1).unwrap();
        assert!(parsed.body.is_empty());
        assert!(parsed.data_indicator);
    }

    #[test]
    fn test_parse_rejects_short_and_bad_slot() {
        assert!(parse(&[], 1).is_err());
        assert!(parse(&[0x00, 0x01, 0x80, 0x02], 1).is_err());
        // t_c_id 0
        assert!(parse(&[0x00, 0x00, 0x80, 0x02, 0x00, 0x00], 1).is_err());
        // t_c_id above slots_num
        assert!(parse(&[0x01, 0x02, 0x80, 0x02, 0x02, 0x00], 1).is_err());
        // link-layer slot id must agree with t_c_id - 1
        assert!(parse(&[0x01, 0x01, 0x80, 0x02, 0x01, 0x00], 2).is_err());
        // malformed frames are attributed by their physical slot byte, not
        // by a mismatched transport connection id
        assert_eq!(
            frame_slot_id(&[0x00, 0x02, 0x80, 0x02, 0x02, 0x00], 2),
            Some(0)
        );
        assert_eq!(frame_slot_id(&[0x02], 2), None);
    }

    #[test]
    fn test_parse_rejects_corrupt_trailer() {
        // wrong SB byte
        assert!(parse(&[0x00, 0x01, 0xA0, 0x01, 0x01, 0x81, 0x02, 0x01, 0x00], 1).is_err());
        // wrong size byte
        assert!(parse(&[0x00, 0x01, 0xA0, 0x01, 0x01, 0x80, 0x03, 0x01, 0x00], 1).is_err());
        // mismatched t_c_id in the trailer
        assert!(parse(&[0x00, 0x01, 0xA0, 0x01, 0x01, 0x80, 0x02, 0x02, 0x00], 1).is_err());
    }

    #[test]
    fn test_parse_rejects_length_mismatch() {
        // declared length is larger than the frame
        let frame = [0x00, 0x01, 0xA0, 0x09, 0x01, 0xAA, 0x80, 0x02, 0x01, 0x00];
        assert!(parse(&frame, 1).is_err());
        // declared length is smaller than the frame
        let frame = [
            0x00, 0x01, 0xA0, 0x02, 0x01, 0xAA, 0xBB, 0xCC, 0x80, 0x02, 0x01, 0x00,
        ];
        assert!(parse(&frame, 1).is_err());
        // declared length 0 does not even cover the t_c_id byte
        let frame = [0x00, 0x01, 0xA0, 0x00, 0x01, 0x80, 0x02, 0x01, 0x00];
        assert!(parse(&frame, 1).is_err());
        // t_c_id byte inside the data frame does not match
        let frame = [0x00, 0x01, 0xA0, 0x02, 0x02, 0xAA, 0x80, 0x02, 0x01, 0x00];
        assert!(parse(&frame, 1).is_err());
    }

    #[test]
    fn test_parse_rejects_unknown_tag() {
        assert!(parse(&[0x00, 0x01, 0x99, 0x01, 0x01, 0x80, 0x02, 0x01, 0x00], 1).is_err());
    }

    #[test]
    fn test_tag_debug() {
        assert_eq!(format!("{:?}", TpduTag::DATA_LAST), "TpduTag(DATA_LAST)");
        assert_eq!(format!("{:?}", TpduTag::new(0x99)), "TpduTag(0x99)");
    }
}
