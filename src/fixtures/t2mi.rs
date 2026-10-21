//! T2-MI packets (TS 102 773) and their TS carrier

use libmpegts::{
    ts::PACKET_SIZE,
    utils::crc32b,
};

use crate::t2mi::TYPE_BASEBAND_FRAME;

/// T2-MI packet of type `kind`: `count`, superframe index 0, payload length in bits, `payload`,
/// CRC-32
pub fn packet(kind: u8, count: u8, payload: &[u8]) -> Vec<u8> {
    let bits = (payload.len() * 8) as u16;
    let mut p = vec![kind, count, 0x00, 0x00];
    p.extend_from_slice(&bits.to_be_bytes());
    p.extend_from_slice(payload);
    let crc = crc32b(&p);
    p.extend_from_slice(&crc.to_be_bytes());
    p
}

/// Baseband Frame packet: frame index 0, `plp`, intl_frame_start set, the BBFRAME `frame`
pub fn bb_packet(count: u8, plp: u8, frame: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x00, plp, 0x80];
    payload.extend_from_slice(frame);
    packet(TYPE_BASEBAND_FRAME, count, &payload)
}

/// One TS packet of `pid` with `payload` at its end behind adaptation field stuffing; `pusi` sets
/// payload_unit_start
pub fn ts_packet(pid: u16, pusi: bool, cc: u8, payload: &[u8]) -> [u8; PACKET_SIZE] {
    let mut p = [0xFFu8; PACKET_SIZE];
    p[0] = 0x47;
    p[1 .. 3].copy_from_slice(&pid.to_be_bytes());
    if pusi {
        p[1] |= 0x40;
    }
    p[3] = 0x10 | (cc & 0x0F);
    let stuffing = PACKET_SIZE - 4 - payload.len();
    if stuffing > 0 {
        p[3] |= 0x20;
        p[4] = stuffing as u8 - 1;
        if stuffing > 1 {
            p[5] = 0;
        }
    }
    p[4 + stuffing ..].copy_from_slice(payload);
    p
}

fn flush_full(pid: u16, pending: &mut Vec<u8>, out: &mut Vec<[u8; PACKET_SIZE]>, cc: &mut u8) {
    while pending.len() >= 183 {
        let n = pending.len().min(184);
        out.push(ts_packet(pid, false, *cc, &pending[.. n]));
        *cc = (*cc + 1) & 0x0F;
        pending.drain(.. n);
    }
}

/// Encapsulates `units` in TS packets of `pid`, continuing the continuity counter `cc`: every
/// unit starts in a packet with payload_unit_start whose pointer counts the bytes of the previous
/// unit in front of it
pub fn pack(pid: u16, units: &[Vec<u8>], cc: &mut u8) -> Vec<[u8; PACKET_SIZE]> {
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    for unit in units {
        flush_full(pid, &mut pending, &mut out, cc);
        let mut payload = vec![pending.len() as u8];
        payload.extend_from_slice(&pending);
        let take = unit.len().min(183 - pending.len());
        payload.extend_from_slice(&unit[.. take]);
        out.push(ts_packet(pid, true, *cc, &payload));
        *cc = (*cc + 1) & 0x0F;
        pending = unit[take ..].to_vec();
    }
    flush_full(pid, &mut pending, &mut out, cc);
    if !pending.is_empty() {
        out.push(ts_packet(pid, false, *cc, &pending));
        *cc = (*cc + 1) & 0x0F;
    }
    out
}
