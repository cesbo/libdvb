//! T2-MI decoder (TS 102 773): the modulator interface stream of one PID back to the transport
//! stream of one PLP
//!
//! T2-MI packets (type, count, superframe index, payload length in bits, payload, CRC-32) travel
//! in TS packets section-like: a payload_unit_start packet begins with a pointer to where the
//! T2-MI packet in progress ends. Baseband Frame packets (type `0x00`: frame index, PLP id, flags,
//! BBFRAME) of the selected PLP go through the [`crate::bbframe`] extractor; timestamps, L1
//! signalling and the other packet types are skipped.

use libmpegts::{
    ts::{
        PACKET_SIZE,
        TsPacketRef,
    },
    utils::crc32b,
};

use crate::bbframe::Extractor;

/// Longest run of T2-MI packet bytes between two pointer fields
const SECTION_MAX: usize = 0x2000;
const HEADER_LEN: usize = 6;
const CRC_LEN: usize = 4;
const TYPE_BASEBAND_FRAME: u8 = 0x00;
/// Baseband Frame payload prefix: frame_idx, plp_id, intl_frame_start
const BBFRAME_PREFIX: usize = 3;

/// Reassembles the T2-MI packets of one PID and extracts the TS of one PLP
pub struct T2miDecoder {
    plp: u8,
    cc: u8,
    section: Box<[u8; SECTION_MAX]>,
    section_len: usize,
    extract: Extractor,
    foreign_plp: Option<u8>,
    /// PLPs already reported by `take_foreign_plp`, one bit each
    seen_plp: [u32; 8],
}

impl T2miDecoder {
    /// Creates a decoder for the physical layer pipe `plp`
    pub fn new(plp: u8) -> Self {
        Self {
            plp,
            cc: 0,
            section: Box::new([0; SECTION_MAX]),
            section_len: 0,
            extract: Extractor::new(None),
            foreign_plp: None,
            seen_plp: [0; 8],
        }
    }

    /// Feeds one TS packet of the T2-MI PID and returns the extracted TS packets, valid until the
    /// next call.
    ///
    /// Packets shorter than 188 bytes, without sync byte or without payload are ignored; the
    /// caller selects the PID.
    pub fn push(&mut self, ts: &[u8]) -> &[u8] {
        self.extract.clear();

        let Ok(ts) = <&[u8; PACKET_SIZE]>::try_from(&ts[.. ts.len().min(PACKET_SIZE)]) else {
            return &[];
        };
        let pkt = TsPacketRef::from(ts);
        if !pkt.is_sync() {
            return &[];
        }
        let Some(payload) = pkt.payload() else {
            return &[];
        };
        let cc = pkt.cc();

        if pkt.is_payload_start() {
            let Some((&pointer, rest)) = payload.split_first() else {
                self.drop_section();
                return &[];
            };
            let Some((tail, head)) = rest.split_at_checked(usize::from(pointer)) else {
                self.drop_section();
                self.cc = cc;
                return &[];
            };
            if self.section_len > 0 && self.append(tail) {
                self.decode_section();
            }
            self.section_len = 0;
            self.append(head);
        } else if self.section_len > 0 {
            let gap = (self.cc + 1) & 0x0F != cc;
            if gap
                && pkt
                    .adaptation_field()
                    .is_some_and(|af| !af.is_empty() && !af.discontinuity_indicator())
            {
                self.drop_section();
            } else {
                self.append(payload);
            }
        }
        self.cc = cc;

        self.extract.out()
    }

    /// Clears all reassembly state and the foreign PLP memory
    pub fn reset(&mut self) {
        self.cc = 0;
        self.section_len = 0;
        self.extract.reset();
        self.foreign_plp = None;
        self.seen_plp = [0; 8];
    }

    /// Returns a PLP seen in the stream other than the configured one, each once
    pub fn take_foreign_plp(&mut self) -> Option<u8> {
        let plp = self.foreign_plp.take()?;
        self.seen_plp[usize::from(plp >> 5)] |= 1 << (plp & 0x1F);
        Some(plp)
    }

    fn drop_section(&mut self) {
        self.section_len = 0;
        self.extract.drop_carry();
    }

    /// Appends to the section; an overflow drops it and returns false
    fn append(&mut self, data: &[u8]) -> bool {
        let end = self.section_len + data.len();
        if end > SECTION_MAX {
            self.drop_section();
            return false;
        }
        self.section[self.section_len .. end].copy_from_slice(data);
        self.section_len = end;
        true
    }

    /// Decodes a section; malformed data drops the remaining section and the carried UP
    fn decode_section(&mut self) {
        let mut rest = &self.section[.. self.section_len];
        while let Some((header, body)) = rest.split_at_checked(HEADER_LEN) {
            let payload_len = (usize::from(u16::from_be_bytes([header[4], header[5]])) + 7) / 8;
            let Some((payload, tail)) = body.split_at_checked(payload_len) else {
                break;
            };
            let Some((crc, _)) = tail.split_at_checked(CRC_LEN) else {
                break;
            };
            let total = HEADER_LEN + payload_len + CRC_LEN;
            if crc32b(&rest[.. total - CRC_LEN])
                != u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]])
            {
                break;
            }

            if header[0] == TYPE_BASEBAND_FRAME {
                let Some((prefix, frame)) = payload.split_at_checked(BBFRAME_PREFIX) else {
                    break;
                };
                let plp = prefix[1];
                if plp == self.plp {
                    self.extract.decode(frame);
                } else if self.seen_plp[usize::from(plp >> 5)] & (1 << (plp & 0x1F)) == 0 {
                    self.foreign_plp = Some(plp);
                }
            }

            rest = &rest[total ..];
        }
        if !rest.is_empty() {
            self.extract.drop_carry();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLP: u8 = 1;
    const PID: u16 = 0x1000;

    /// TS packet whose sync byte the modulator removed: the 187 bytes behind it
    fn up(tag: u8) -> [u8; PACKET_SIZE] {
        let mut p = [0u8; PACKET_SIZE];
        p[0] = 0x47;
        p[1] = 0x01;
        p[2] = tag;
        p[3] = 0x10;
        for (i, b) in p[4 ..].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(tag).wrapping_add(3);
        }
        p
    }

    fn body(tag: u8) -> Vec<u8> {
        up(tag)[1 ..].to_vec()
    }

    fn concat(ups: &[[u8; PACKET_SIZE]]) -> Vec<u8> {
        ups.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// BBFRAME in high efficiency mode
    fn bbframe(npd: bool, syncd: Option<u16>, df: &[u8]) -> Vec<u8> {
        let mut h = [0u8; 10];
        h[0] = 0xC0 | if npd { 0x04 } else { 0 };
        h[4 .. 6].copy_from_slice(&((df.len() as u16) << 3).to_be_bytes());
        h[7 .. 9].copy_from_slice(&syncd.map_or(0xFFFF, |s| s << 3).to_be_bytes());
        h[9] = crate::bbframe::crc8(&h[.. 9]) ^ 0x01;
        let mut f = h.to_vec();
        f.extend_from_slice(df);
        f
    }

    /// BBFRAME in normal mode: 188-byte UPs with a CRC-8 in the sync position
    fn bbframe_normal(df: &[u8]) -> Vec<u8> {
        let mut h = [0u8; 10];
        h[0] = 0xC0;
        h[2 .. 4].copy_from_slice(&(188u16 << 3).to_be_bytes());
        h[4 .. 6].copy_from_slice(&((df.len() as u16) << 3).to_be_bytes());
        h[6] = 0x47;
        h[9] = crate::bbframe::crc8(&h[.. 9]);
        let mut f = h.to_vec();
        f.extend_from_slice(df);
        f
    }

    fn t2mi_packet(kind: u8, count: u8, payload: &[u8]) -> Vec<u8> {
        let bits = (payload.len() * 8) as u16;
        let mut p = vec![kind, count, 0x00, 0x00];
        p.extend_from_slice(&bits.to_be_bytes());
        p.extend_from_slice(payload);
        let crc = crc32b(&p);
        p.extend_from_slice(&crc.to_be_bytes());
        p
    }

    fn bb_packet(count: u8, plp: u8, frame: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x00, plp, 0x80];
        payload.extend_from_slice(frame);
        t2mi_packet(TYPE_BASEBAND_FRAME, count, &payload)
    }

    fn ts_packet(pusi: bool, cc: u8, payload: &[u8]) -> [u8; PACKET_SIZE] {
        let mut p = [0xFFu8; PACKET_SIZE];
        p[0] = 0x47;
        p[1 .. 3].copy_from_slice(&PID.to_be_bytes());
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

    fn flush_full(pending: &mut Vec<u8>, out: &mut Vec<[u8; PACKET_SIZE]>, cc: &mut u8) {
        while pending.len() > 183 {
            out.push(ts_packet(false, *cc, &pending[.. 184]));
            *cc = (*cc + 1) & 0x0F;
            pending.drain(.. 184);
        }
    }

    /// Section-like TS encapsulation: every unit starts with a payload_unit_start packet whose
    /// pointer skips the tail of the previous one; a final flush packet closes the last unit
    fn pack(units: &[Vec<u8>], cc: &mut u8) -> Vec<[u8; PACKET_SIZE]> {
        let mut out = Vec::new();
        let mut pending: Vec<u8> = Vec::new();
        for unit in units {
            flush_full(&mut pending, &mut out, cc);
            let mut payload = vec![pending.len() as u8];
            payload.extend_from_slice(&pending);
            let take = unit.len().min(183 - pending.len());
            payload.extend_from_slice(&unit[.. take]);
            out.push(ts_packet(true, *cc, &payload));
            *cc = (*cc + 1) & 0x0F;
            pending = unit[take ..].to_vec();
        }
        flush_full(&mut pending, &mut out, cc);
        let mut payload = vec![pending.len() as u8];
        payload.extend_from_slice(&pending);
        out.push(ts_packet(true, *cc, &payload));
        *cc = (*cc + 1) & 0x0F;
        out
    }

    fn feed(dec: &mut T2miDecoder, pkts: &[[u8; PACKET_SIZE]]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in pkts {
            out.extend_from_slice(dec.push(p));
        }
        out
    }

    #[test]
    fn hem() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let df = [body(1), body(2), body(3)].concat();
        let pkts = pack(&[bb_packet(0, PLP, &bbframe(false, Some(0), &df))], &mut cc);
        assert!(pkts.len() > 3);
        assert_eq!(feed(&mut dec, &pkts), concat(&[up(1), up(2), up(3)]));
    }

    #[test]
    fn carry() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let split = body(2);
        let df1 = [body(1), split[.. 60].to_vec()].concat();
        let df2 = [split[60 ..].to_vec(), body(3)].concat();
        let units = [
            bb_packet(0, PLP, &bbframe(false, Some(0), &df1)),
            bb_packet(1, PLP, &bbframe(false, Some(127), &df2)),
        ];
        assert_eq!(
            feed(&mut dec, &pack(&units, &mut cc)),
            concat(&[up(1), up(2), up(3)])
        );
    }

    #[test]
    fn span() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let split = body(1);
        let units = [
            bb_packet(0, PLP, &bbframe(false, Some(0), &split[.. 40])),
            bb_packet(1, PLP, &bbframe(false, None, &split[40 .. 100])),
            bb_packet(
                2,
                PLP,
                &bbframe(false, Some(87), &[split[100 ..].to_vec(), body(2)].concat()),
            ),
        ];
        assert_eq!(
            feed(&mut dec, &pack(&units, &mut cc)),
            concat(&[up(1), up(2)])
        );
    }

    #[test]
    fn npd() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let df = [body(1), vec![0x02], body(2), vec![0x00]].concat();
        let pkts = pack(&[bb_packet(0, PLP, &bbframe(true, Some(0), &df))], &mut cc);
        assert_eq!(feed(&mut dec, &pkts), concat(&[up(1), up(2)]));
    }

    #[test]
    fn normal_mode() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let mut df = concat(&[up(1), up(2)]);
        df[0] = 0x33;
        df[188] = 0x44;
        let pkts = pack(&[bb_packet(0, PLP, &bbframe_normal(&df))], &mut cc);
        assert_eq!(feed(&mut dec, &pkts), concat(&[up(1), up(2)]));
    }

    #[test]
    fn plp() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let units = [
            bb_packet(0, 2, &bbframe(false, Some(0), &body(9))),
            bb_packet(1, PLP, &bbframe(false, Some(0), &body(1))),
            bb_packet(2, 2, &bbframe(false, Some(0), &body(9))),
        ];
        assert_eq!(feed(&mut dec, &pack(&units, &mut cc)), up(1));
        assert_eq!(dec.take_foreign_plp(), Some(2));
        assert_eq!(dec.take_foreign_plp(), None);
    }

    #[test]
    fn other_types() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let units = [
            t2mi_packet(0x20, 0, &[0x11; 12]),
            bb_packet(1, PLP, &bbframe(false, Some(0), &body(1))),
            t2mi_packet(0x10, 2, &[0x22; 40]),
        ];
        assert_eq!(feed(&mut dec, &pack(&units, &mut cc)), up(1));
    }

    #[test]
    fn bad_crc() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let mut broken = bb_packet(0, PLP, &bbframe(false, Some(0), &body(1)));
        let last = broken.len() - 1;
        broken[last] ^= 0xFF;
        // one section holding the broken packet and a good one behind it, then a clean section
        let mut section = broken;
        section.extend_from_slice(&bb_packet(1, PLP, &bbframe(false, Some(0), &body(2))));
        let units = [
            section,
            bb_packet(2, PLP, &bbframe(false, Some(0), &body(3))),
        ];
        assert_eq!(feed(&mut dec, &pack(&units, &mut cc)), up(3));
    }

    #[test]
    fn malformed_drops_carry() {
        let df1 = [body(1), body(2)[.. 100].to_vec()].concat();
        let df2 = [body(2)[100 ..].to_vec(), body(3), body(4)[.. 100].to_vec()].concat();
        let df3 = [body(4)[100 ..].to_vec(), body(5)].concat();
        let middle = bb_packet(1, PLP, &bbframe(false, Some(87), &df2));
        let mut bad_crc = middle.clone();
        let last = bad_crc.len() - 1;
        bad_crc[last] ^= 0xFF;

        for broken in [
            bad_crc,
            middle[.. HEADER_LEN - 1].to_vec(),
            middle[.. HEADER_LEN + BBFRAME_PREFIX].to_vec(),
            middle[.. middle.len() - 1].to_vec(),
            t2mi_packet(TYPE_BASEBAND_FRAME, 1, &[0, PLP]),
        ] {
            let mut dec = T2miDecoder::new(PLP);
            let mut cc = 0;
            // Preserve packets emitted before the error in the same section.
            let mut section = bb_packet(0, PLP, &bbframe(false, Some(0), &df1));
            section.extend_from_slice(&broken);
            let units = [section, bb_packet(2, PLP, &bbframe(false, Some(87), &df3))];
            assert_eq!(
                feed(&mut dec, &pack(&units, &mut cc)),
                concat(&[up(1), up(5)])
            );
        }
    }

    #[test]
    fn discard_drops_carry() {
        for cause in ["pointer", "overflow", "cc"] {
            let mut dec = T2miDecoder::new(PLP);
            let mut cc = 0;
            let start = bb_packet(0, PLP, &bbframe(false, Some(0), &body(1)[.. 100]));
            assert!(feed(&mut dec, &pack(&[start], &mut cc)).is_empty());

            assert!(dec.push(&ts_packet(true, cc, &[0; 184])).is_empty());
            cc = (cc + 1) & 0x0F;
            match cause {
                "pointer" => {
                    assert!(dec.push(&ts_packet(true, cc, &[0xFF])).is_empty());
                    cc = (cc + 1) & 0x0F;
                }
                "overflow" => {
                    for _ in 0 .. SECTION_MAX / 184 {
                        assert!(dec.push(&ts_packet(false, cc, &[0; 184])).is_empty());
                        cc = (cc + 1) & 0x0F;
                    }
                }
                _ => {
                    cc = (cc + 5) & 0x0F;
                    assert!(dec.push(&ts_packet(false, cc, &[0; 100])).is_empty());
                    cc = (cc + 1) & 0x0F;
                }
            }

            let df = [body(2)[100 ..].to_vec(), body(3)].concat();
            let next = bb_packet(2, PLP, &bbframe(false, Some(87), &df));
            assert_eq!(feed(&mut dec, &pack(&[next], &mut cc)), up(3), "{cause}");
        }
    }

    #[test]
    fn cc_gap() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let unit = bb_packet(
            0,
            PLP,
            &bbframe(false, Some(0), &[body(1), body(2)].concat()),
        );
        let mut pkts = pack(&[unit.clone()], &mut cc);
        // a continuation packet with a cc gap and an adaptation field without discontinuity
        // drops the section
        let mut p = pkts[1];
        p[3] = 0x30 | ((p[3] + 1) & 0x0F);
        p[4] = 0x01;
        p[5] = 0x00;
        pkts[1] = p;
        assert!(feed(&mut dec, &pkts).is_empty());

        // the same gap without an adaptation field keeps the data
        let mut pkts = pack(&[unit], &mut cc);
        pkts[1][3] = 0x10 | ((pkts[1][3] + 1) & 0x0F);
        assert_eq!(feed(&mut dec, &pkts), concat(&[up(1), up(2)]));
    }

    #[test]
    fn pointer() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let mut pkts = pack(
            &[bb_packet(0, PLP, &bbframe(false, Some(0), &body(1)))],
            &mut cc,
        );
        let last = pkts.len() - 1;
        let pointer = PACKET_SIZE - TsPacketRef::from(&pkts[last]).payload().unwrap().len();
        pkts[last][pointer] = 0xFF;
        assert!(feed(&mut dec, &pkts).is_empty());
    }

    #[test]
    fn reset() {
        let mut dec = T2miDecoder::new(PLP);
        let mut cc = 0;
        let pkts = pack(
            &[bb_packet(0, 2, &bbframe(false, Some(0), &body(1)))],
            &mut cc,
        );
        feed(&mut dec, &pkts[.. pkts.len() - 1]);
        dec.reset();
        assert_eq!(dec.section_len, 0);
        assert!(dec.push(&pkts[pkts.len() - 1]).is_empty());
        assert_eq!(dec.take_foreign_plp(), None);
    }

    #[test]
    fn garbage() {
        let mut dec = T2miDecoder::new(PLP);
        let mut x: u32 = 0x1234_5678;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0 .. 5000 {
            let mut p = [0u8; PACKET_SIZE];
            for b in p.iter_mut() {
                *b = next() as u8;
            }
            p[0] = 0x47;
            if next() % 4 == 0 {
                p[3] = 0x10 | (p[3] & 0x0F);
            }
            let len = if next() % 8 == 0 {
                usize::try_from(next() % 200).unwrap_or(PACKET_SIZE)
            } else {
                PACKET_SIZE
            };
            let out = dec.push(&p[.. len.min(PACKET_SIZE)]);
            assert_eq!(out.len() % PACKET_SIZE, 0);
            dec.take_foreign_plp();
        }
    }
}
