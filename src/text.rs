//! Text in DVB charset coding

use std::fmt;

use libmpegts::utils::textcode::TextcodeRef;

/// Text in DVB charset coding (EN 300 468 annex A)
///
/// [`Display`](fmt::Display) decodes the text; an undecodable text
/// displays as empty. The raw bytes stay reachable through
/// [`DvbText::as_bytes`].
#[derive(Clone, PartialEq, Eq, Default)]
pub struct DvbText(Vec<u8>);

impl DvbText {
    /// Raw DVB-coded bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Raw DVB-coded bytes, consuming the text
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Whether the text has no bytes at all
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for DvbText {
    fn from(data: Vec<u8>) -> Self {
        DvbText(data)
    }
}

impl From<&[u8]> for DvbText {
    fn from(data: &[u8]) -> Self {
        DvbText(data.to_vec())
    }
}

impl fmt::Display for DvbText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match TextcodeRef::try_from(self.0.as_slice()) {
            Ok(text) => text.fmt(f),
            Err(_) => Ok(()),
        }
    }
}

impl fmt::Debug for DvbText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match TextcodeRef::try_from(self.0.as_slice()) {
            Ok(text) => write!(f, "{:?}", text.to_string()),
            Err(_) => write!(f, "DvbText({:02X?})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(DvbText::from(&b"Menu"[..]).to_string(), "Menu");
        assert_eq!(DvbText::default().to_string(), "");
        // a truncated charset selector is undecodable and displays as empty
        assert_eq!(DvbText::from(&[0x10][..]).to_string(), "");
    }

    #[test]
    fn test_debug() {
        assert_eq!(format!("{:?}", DvbText::from(&b"Menu"[..])), "\"Menu\"");
    }
}
