//! Synthetic T2-MI and DigitalDevices BBFrame streams for tests
//!
//! Deterministic builders of base band frames, of the T2-MI packets carrying them and of the TS
//! packets both travel in, feeding [`crate::BbFrameDecoder`] and [`crate::T2miDecoder`] in the
//! unit tests of this crate and of its users. Available with the `fixtures` feature; not meant
//! for production use.

use libmpegts::ts::PACKET_SIZE;

pub mod bbframe;
pub mod t2mi;

/// Concatenates TS packets into one byte vector
pub fn concat(ups: &[[u8; PACKET_SIZE]]) -> Vec<u8> {
    ups.iter().flat_map(|p| p.iter().copied()).collect()
}
