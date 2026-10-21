//! Base band frames (EN 302 307-1 5.1.6) and their DigitalDevices TS encapsulation

use libmpegts::ts::PACKET_SIZE;

use crate::bbframe::{
    BBFRAME_PID,
    FRAGMENT_FIRST,
    FRAGMENT_LEN_MAX,
    PREFIX_LEN,
    crc8,
};

/// UP of `tag`: a TS packet on PID `0x100 | tag` with a `tag`-derived payload and `0x11 ^ tag` in
/// the sync position, where the modulator writes its CRC-8
pub fn up(tag: u8) -> [u8; PACKET_SIZE] {
    let mut p = [0u8; PACKET_SIZE];
    p[0] = 0x11 ^ tag;
    p[1] = 0x01;
    p[2] = tag;
    p[3] = 0x10;
    for (i, b) in p[4 ..].iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(tag).wrapping_add(7);
    }
    p
}

/// The UP of `tag` as the decoder restores it, with the `0x47` sync byte
pub fn restored(tag: u8) -> [u8; PACKET_SIZE] {
    let mut p = up(tag);
    p[0] = 0x47;
    p
}

/// Normal mode BBHEADER of a TS input stream: MATYPE `0xC0`, `isi`, `upl`, `dfl` and `syncd` in
/// bytes, SYNC `0x47`, CRC-8
pub fn header(isi: u8, upl: u16, dfl: u16, syncd: u16) -> [u8; 10] {
    let mut h = [0u8; 10];
    h[0] = 0xC0;
    h[1] = isi;
    h[2 .. 4].copy_from_slice(&(upl << 3).to_be_bytes());
    h[4 .. 6].copy_from_slice(&(dfl << 3).to_be_bytes());
    h[6] = 0x47;
    h[7 .. 9].copy_from_slice(&(syncd << 3).to_be_bytes());
    h[9] = crc8(&h[.. 9]);
    h
}

/// Normal mode BBFRAME: [`header`] over the data field `df`
pub fn normal_frame(isi: u8, upl: u16, syncd: u16, df: &[u8]) -> Vec<u8> {
    let mut f = header(isi, upl, df.len() as u16, syncd).to_vec();
    f.extend_from_slice(df);
    f
}

/// High efficiency mode BBFRAME: the mode bit xored into the CRC-8 field, `npd` enables null
/// packet deletion, `syncd` is the offset of the first UP start in `df` or `None` when no UP
/// starts there (SYNCD `0xFFFF`)
pub fn hem_frame(isi: u8, npd: bool, syncd: Option<u16>, df: &[u8]) -> Vec<u8> {
    let mut h = [0u8; 10];
    h[0] = 0xC0 | if npd { 0x04 } else { 0 };
    h[1] = isi;
    h[4 .. 6].copy_from_slice(&((df.len() as u16) << 3).to_be_bytes());
    h[7 .. 9].copy_from_slice(&syncd.map_or(0xFFFF, |s| s << 3).to_be_bytes());
    h[9] = crc8(&h[.. 9]) ^ 0x01;
    let mut f = h.to_vec();
    f.extend_from_slice(df);
    f
}

/// One TS packet of [`BBFRAME_PID`] with `data` behind the DigitalDevices fragment prefix:
/// `slen` is the fragment length plus one, `count` is `0xB8` for the first fragment of a frame
pub fn dd_fragment(cc: u8, slen: u8, count: u8, data: &[u8]) -> [u8; PACKET_SIZE] {
    let mut p = [0xFFu8; PACKET_SIZE];
    p[0] = 0x47;
    p[1 .. 3].copy_from_slice(&BBFRAME_PID.to_be_bytes());
    p[3] = 0x10 | (cc & 0x0F);
    p[4 .. 7].copy_from_slice(&[0x00, 0x80, 0x00]);
    p[7] = slen;
    p[8] = count;
    p[PREFIX_LEN .. PREFIX_LEN + data.len()].copy_from_slice(data);
    p
}

/// Fragments `frame` into TS packets of [`BBFRAME_PID`] as a DigitalDevices frontend does it,
/// continuing the continuity counter `cc`
pub fn dd_fragments(frame: &[u8], cc: &mut u8) -> Vec<[u8; PACKET_SIZE]> {
    let mut out = Vec::new();
    for (n, chunk) in frame.chunks(usize::from(FRAGMENT_LEN_MAX) - 1).enumerate() {
        let count = if n == 0 { FRAGMENT_FIRST } else { n as u8 };
        out.push(dd_fragment(*cc, chunk.len() as u8 + 1, count, chunk));
        *cc = (*cc + 1) & 0x0F;
    }
    out
}
