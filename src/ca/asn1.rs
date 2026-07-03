//! asn.1 length field coding
//!
//! en50221 7.3.1

use crate::error::{
    Error,
    Result,
};

const SIZE_INDICATOR: u8 = 0x80;

/// Appends the asn.1 length coding of `value` to `out`
pub fn encode(value: u16, out: &mut Vec<u8>) {
    if value < SIZE_INDICATOR as _ {
        out.push(value as u8);
    } else if value < 0x100 {
        out.push(SIZE_INDICATOR + 1);
        out.push(value as u8);
    } else {
        out.push(SIZE_INDICATOR + 2);
        out.push((value >> 8) as u8);
        out.push(value as u8);
    }
}

/// Decodes an asn.1 length field from the start of `data`
pub fn decode(data: &[u8]) -> Result<(u16, usize)> {
    let first = match data.first() {
        Some(&b) => b,
        None => {
            return Err(Error::InvalidData(
                "ca asn.1 length field is empty".to_owned(),
            ));
        }
    };

    if first < SIZE_INDICATOR {
        return Ok((u16::from(first), 1));
    }

    match first {
        0x81 => match data.get(1) {
            Some(&b) => Ok((u16::from(b), 2)),
            None => Err(Error::InvalidData(
                "ca asn.1 length field is truncated".to_owned(),
            )),
        },
        0x82 => match data.get(1 .. 3) {
            Some(b) => Ok(((u16::from(b[0]) << 8) | u16::from(b[1]), 3)),
            None => Err(Error::InvalidData(
                "ca asn.1 length field is truncated".to_owned(),
            )),
        },
        _ => Err(Error::InvalidData(format!(
            "ca asn.1 invalid size indicator 0x{:02X}",
            first
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: u16, expected: &[u8]) {
        let mut out = Vec::new();
        encode(value, &mut out);
        assert_eq!(out.as_slice(), expected);

        let (decoded, consumed) = decode(&out).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(consumed, out.len());
    }

    #[test]
    fn test_roundtrip_boundaries() {
        roundtrip(0, &[0x00]);
        roundtrip(1, &[0x01]);
        roundtrip(127, &[0x7F]);
        roundtrip(128, &[0x81, 0x80]);
        roundtrip(255, &[0x81, 0xFF]);
        roundtrip(256, &[0x82, 0x01, 0x00]);
        roundtrip(2041, &[0x82, 0x07, 0xF9]);
        roundtrip(65535, &[0x82, 0xFF, 0xFF]);
    }

    #[test]
    fn test_decode_ignores_trailing_bytes() {
        assert_eq!(decode(&[0x05, 0xAA, 0xBB]).unwrap(), (5, 1));
        assert_eq!(decode(&[0x81, 0xFF, 0xAA]).unwrap(), (255, 2));
        assert_eq!(decode(&[0x82, 0x01, 0x00, 0xAA]).unwrap(), (256, 3));
    }

    #[test]
    fn test_decode_empty() {
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn test_decode_truncated() {
        assert!(decode(&[0x81]).is_err());
        assert!(decode(&[0x82]).is_err());
        assert!(decode(&[0x82, 0x01]).is_err());
    }

    #[test]
    fn test_decode_invalid_indicator() {
        assert!(decode(&[0x80]).is_err());
        assert!(decode(&[0x83, 0x00, 0x00, 0x01]).is_err());
        assert!(decode(&[0xFF, 0x00]).is_err());
    }
}
