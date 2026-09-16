//! DVB-S2/T2 BBFrame decoder: base band frames to transport stream
//!
//! A base band frame (EN 302 307-1 5.1.6, EN 302 755 5.1.7) carries the user packets of one input
//! stream behind a 10-byte BBHEADER (MATYPE, UPL, DFL, SYNC, SYNCD, CRC-8). In normal mode every
//! UP is a 188-byte TS packet whose sync byte the modulator replaced by a CRC-8; in high
//! efficiency mode (signalled by the mode bit xored into the CRC-8 field) the sync byte is
//! removed and the UP is 187 bytes, optionally followed by a null packet deletion counter. The
//! decoder restores the `0x47` sync byte, carries a UP split across frames and returns whole TS
//! packets.
//!
//! [`BbFrameDecoder`] also reassembles the frames DigitalDevices DVB-S2 frontends deliver with the
//! BBFrames bit set in `DTV_STREAM_ID`: each BBFRAME is fragmented into TS packets on
//! [`BBFRAME_PID`] behind a 5-byte prefix (`00 80 00`, fragment length, fragment counter; the
//! counter is `0xB8` for the first fragment of a frame and then 1, 2, 3 ...). A frame is decoded
//! when the first fragment of the next frame arrives, so extraction lags one BBFRAME behind. A
//! frame that arrives whole through another transport goes through
//! [`BbFrameDecoder::push_frame`]; [`crate::T2miDecoder`] shares the extractor.

use std::cmp::Ordering;

use libmpegts::ts::{
    PACKET_SIZE,
    TsPacketRef,
};

/// PID carrying the BBFrame fragments
pub const BBFRAME_PID: u16 = 270;

/// Largest BBFRAME (58192 bits, normal FECFRAME rate 9/10)
const FRAME_MAX: usize = 58192 / 8;
const HEADER_LEN: usize = 10;
/// Fragment prefix: `00 80 00`, length, counter
const PREFIX_LEN: usize = 9;
const FRAGMENT_LEN_MAX: u8 = 0xB4;
const FRAGMENT_FIRST: u8 = 0xB8;
/// Every UP of the longest input decoded in one call (a 13-bit DFL, or a T2-MI section of
/// 8192 bytes in high efficiency mode) plus the completed carried UP
const OUT_MAX: usize = 44 * PACKET_SIZE;

/// CRC-8 x^8+x^7+x^6+x^4+x^2+1, MSB first, init 0
const CRC8_TABLE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0xD5
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub(crate) fn crc8(data: &[u8]) -> u8 {
    data.iter()
        .fold(0, |crc, &b| CRC8_TABLE[usize::from(crc ^ b)])
}

/// Reassembles BBFrames from the fragment PID and extracts the TS of one ISI
pub struct BbFrameDecoder {
    cc: u8,
    /// Last fragment counter; `None` until the next `0xB8` fragment
    fragment: Option<u8>,
    frame: Box<[u8; FRAME_MAX]>,
    frame_len: usize,
    extract: Extractor,
}

impl BbFrameDecoder {
    /// Creates a decoder for the input stream `isi`
    pub fn new(isi: u8) -> Self {
        Self {
            cc: 0,
            fragment: None,
            frame: Box::new([0; FRAME_MAX]),
            frame_len: 0,
            extract: Extractor::new(Some(isi)),
        }
    }

    /// Feeds one TS packet and returns the extracted TS packets, valid until the next call.
    ///
    /// Packets shorter than 188 bytes, without sync byte or on another PID are ignored.
    pub fn push(&mut self, ts: &[u8]) -> &[u8] {
        self.extract.clear();

        let Ok(ts) = <&[u8; PACKET_SIZE]>::try_from(&ts[.. ts.len().min(PACKET_SIZE)]) else {
            return &[];
        };
        let pkt = TsPacketRef::from(ts);
        if !pkt.is_sync() || pkt.pid() != BBFRAME_PID {
            return &[];
        }

        let cc = pkt.cc();
        if self.frame_len > 0
            && (self.cc + 1) & 0x0F != cc
            && pkt
                .adaptation_field()
                .is_some_and(|af| !af.is_empty() && !af.discontinuity_indicator())
        {
            self.drop_frame();
        }
        self.cc = cc;

        if ts[4 .. 7] != [0x00, 0x80, 0x00] {
            return &[];
        }
        let slen = ts[7];
        if slen == 0 || slen > FRAGMENT_LEN_MAX {
            return &[];
        }
        let fragment = &ts[PREFIX_LEN .. PREFIX_LEN + usize::from(slen) - 1];

        let count = ts[8];
        if count == FRAGMENT_FIRST {
            if self.frame_len > 0 {
                self.extract.decode(&self.frame[.. self.frame_len]);
            }
            self.fragment = Some(0);
            self.frame_len = 0;
        } else if self.fragment.and_then(|n| n.checked_add(1)) == Some(count) {
            self.fragment = Some(count);
        } else {
            self.drop_frame();
            return &[];
        }

        let end = self.frame_len + fragment.len();
        if end > FRAME_MAX {
            self.drop_frame();
            return &[];
        }
        self.frame[self.frame_len .. end].copy_from_slice(fragment);
        self.frame_len = end;

        self.extract.out()
    }

    /// Decodes one complete BBFRAME (BBHEADER and data field) and returns its TS packets, valid
    /// until the next call
    pub fn push_frame(&mut self, frame: &[u8]) -> &[u8] {
        self.extract.clear();
        self.extract.decode(frame);
        self.extract.out()
    }

    /// Clears all reassembly state and the foreign ISI memory
    pub fn reset(&mut self) {
        self.cc = 0;
        self.drop_frame();
        self.extract.reset();
    }

    /// Returns an ISI seen in the stream other than the configured one, each once
    pub fn take_foreign_isi(&mut self) -> Option<u8> {
        self.extract.take_foreign_isi()
    }

    fn drop_frame(&mut self) {
        self.fragment = None;
        self.frame_len = 0;
        self.extract.carry_len = 0;
    }
}

/// Data field to user packets; the UP tail is carried between frames
pub(crate) struct Extractor {
    /// Frames of other input streams are skipped; `None` takes every frame
    isi: Option<u8>,
    /// Carried UP in packet form (byte 0 is the sync position): first 188 bytes stored,
    /// `carry_len` counts the UP bytes received so far as transmitted
    carry: [u8; PACKET_SIZE],
    carry_len: usize,
    out: Box<[u8; OUT_MAX]>,
    out_len: usize,
    foreign_isi: Option<u8>,
    /// ISIs already reported by `take_foreign_isi`, one bit each
    seen_isi: [u32; 8],
}

impl Extractor {
    pub(crate) fn new(isi: Option<u8>) -> Self {
        Self {
            isi,
            carry: [0; PACKET_SIZE],
            carry_len: 0,
            out: Box::new([0; OUT_MAX]),
            out_len: 0,
            foreign_isi: None,
            seen_isi: [0; 8],
        }
    }

    /// TS packets extracted since `clear`
    pub(crate) fn out(&self) -> &[u8] {
        &self.out[.. self.out_len]
    }

    pub(crate) fn clear(&mut self) {
        self.out_len = 0;
    }

    pub(crate) fn reset(&mut self) {
        self.carry_len = 0;
        self.out_len = 0;
        self.foreign_isi = None;
        self.seen_isi = [0; 8];
    }

    pub(crate) fn take_foreign_isi(&mut self) -> Option<u8> {
        let isi = self.foreign_isi.take()?;
        self.seen_isi[usize::from(isi >> 5)] |= 1 << (isi & 0x1F);
        Some(isi)
    }

    /// Decodes one BBFRAME and appends its TS packets to the output; a malformed frame drops the
    /// carried UP as well
    pub(crate) fn decode(&mut self, frame: &[u8]) {
        let Some((h, df)) = frame.split_at_checked(HEADER_LEN) else {
            self.carry_len = 0;
            return;
        };
        // TS streams only; the CRC-8 field xor the header CRC is the mode
        let mode = crc8(&h[.. 9]) ^ h[9];
        if h[0] >> 6 != 0b11 || mode > 1 {
            self.carry_len = 0;
            return;
        }
        if self.isi.is_some_and(|isi| isi != h[1]) {
            let isi = h[1];
            if self.seen_isi[usize::from(isi >> 5)] & (1 << (isi & 0x1F)) == 0 {
                self.foreign_isi = Some(isi);
            }
            return;
        }

        // UP as transmitted and the part of it inside the packet: the whole packet behind a
        // CRC-8 in normal mode, 187 bytes without the sync byte plus the null packet deletion
        // counter in high efficiency mode
        let (up_len, body) = if mode == 0 {
            (
                usize::from(u16::from_be_bytes([h[2], h[3]]) >> 3),
                PACKET_SIZE,
            )
        } else {
            (
                PACKET_SIZE - 1 + usize::from(h[0] >> 2 & 1),
                PACKET_SIZE - 1,
            )
        };
        let dfl = usize::from(u16::from_be_bytes([h[4], h[5]]) >> 3);
        // SYNCD 0xFFFF: no UP starts in this data field
        let syncd = match u16::from_be_bytes([h[7], h[8]]) {
            0xFFFF => None,
            bits => Some(usize::from(bits >> 3)),
        };
        if up_len < body || dfl > df.len() || syncd.is_some_and(|s| s > dfl) {
            self.carry_len = 0;
            return;
        }
        let df = &df[.. dfl];

        // the bytes before the first UP start continue the carried UP, which completes only
        // when they land exactly at its end
        let head = syncd.unwrap_or(dfl);
        match (self.carry_len + head).cmp(&up_len) {
            Ordering::Equal => {
                self.append_carry(body, &df[.. head]);
                emit(&mut *self.out, &mut self.out_len, &self.carry);
            }
            Ordering::Less if syncd.is_none() && self.carry_len > 0 => {
                self.append_carry(body, df);
                return;
            }
            _ => {}
        }

        let mut i = head;
        while dfl - i >= up_len {
            emit(&mut *self.out, &mut self.out_len, &df[i .. i + body]);
            i += up_len;
        }

        self.carry_len = 0;
        self.append_carry(body, &df[i ..]);
    }

    /// Appends UP bytes to the carried UP; in high efficiency mode the stored copy starts after
    /// the sync position
    fn append_carry(&mut self, body: usize, data: &[u8]) {
        let pos = PACKET_SIZE - body + self.carry_len;
        if pos < PACKET_SIZE {
            let n = data.len().min(PACKET_SIZE - pos);
            self.carry[pos .. pos + n].copy_from_slice(&data[.. n]);
        }
        self.carry_len += data.len();
    }
}

/// Appends one UP as a TS packet with the sync byte restored: 188 bytes with the CRC-8 in the
/// sync position, or the 187 bytes behind it
fn emit(out: &mut [u8], out_len: &mut usize, up: &[u8]) {
    let Some(dst) = out.get_mut(*out_len .. *out_len + PACKET_SIZE) else {
        return;
    };
    dst[PACKET_SIZE - up.len() ..].copy_from_slice(up);
    dst[0] = 0x47;
    *out_len += PACKET_SIZE;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISI: u8 = 1;

    /// UP: a TS packet whose sync byte was replaced by the modulator
    fn up(tag: u8) -> [u8; PACKET_SIZE] {
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

    fn header(isi: u8, upl: u16, dfl: u16, syncd: u16) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        h[0] = 0xC0;
        h[1] = isi;
        h[2 .. 4].copy_from_slice(&(upl << 3).to_be_bytes());
        h[4 .. 6].copy_from_slice(&(dfl << 3).to_be_bytes());
        h[6] = 0x47;
        h[7 .. 9].copy_from_slice(&(syncd << 3).to_be_bytes());
        h[9] = crc8(&h[.. 9]);
        h
    }

    fn frame(isi: u8, upl: u16, syncd: u16, df: &[u8]) -> Vec<u8> {
        let mut f = header(isi, upl, df.len() as u16, syncd).to_vec();
        f.extend_from_slice(df);
        f
    }

    fn fragment(cc: u8, slen: u8, count: u8, data: &[u8]) -> [u8; PACKET_SIZE] {
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

    fn fragments(frame: &[u8], cc: &mut u8) -> Vec<[u8; PACKET_SIZE]> {
        let mut out = Vec::new();
        for (n, chunk) in frame.chunks(usize::from(FRAGMENT_LEN_MAX) - 1).enumerate() {
            let count = if n == 0 { FRAGMENT_FIRST } else { n as u8 };
            out.push(fragment(*cc, chunk.len() as u8 + 1, count, chunk));
            *cc = (*cc + 1) & 0x0F;
        }
        out
    }

    fn feed(dec: &mut BbFrameDecoder, pkts: &[[u8; PACKET_SIZE]]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in pkts {
            out.extend_from_slice(dec.push(p));
        }
        out
    }

    fn restored(tag: u8) -> [u8; PACKET_SIZE] {
        let mut p = up(tag);
        p[0] = 0x47;
        p
    }

    fn concat(ups: &[[u8; PACKET_SIZE]]) -> Vec<u8> {
        ups.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// One aligned frame with three UPs followed by the first fragment of the next frame
    fn aligned_stream(cc: &mut u8) -> (Vec<[u8; PACKET_SIZE]>, Vec<u8>) {
        let df = concat(&[up(1), up(2), up(3)]);
        let mut pkts = fragments(&frame(ISI, 188, 0, &df), cc);
        pkts.push(fragments(&frame(ISI, 188, 0, &up(4)), cc)[0]);
        (pkts, concat(&[restored(1), restored(2), restored(3)]))
    }

    #[test]
    fn table() {
        assert_eq!(CRC8_TABLE[0x00], 0x00);
        assert_eq!(CRC8_TABLE[0x01], 0xd5);
        assert_eq!(CRC8_TABLE[0x02], 0x7f);
        assert_eq!(CRC8_TABLE[0x03], 0xaa);
        assert_eq!(CRC8_TABLE[0xff], 0xf9);
    }

    #[test]
    fn header_crc() {
        assert_eq!(crc8(&header(3, 188, 1880, 0)), 0);
        let mut h = header(3, 188, 1880, 0);
        h[1] ^= 1;
        assert_ne!(crc8(&h), 0);
    }

    #[test]
    fn aligned() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let (pkts, expect) = aligned_stream(&mut cc);
        let (frame, next) = pkts.split_at(pkts.len() - 1);
        assert!(feed(&mut dec, frame).is_empty());
        let out = feed(&mut dec, next);
        assert_eq!(out, expect);
        assert!(out.chunks(PACKET_SIZE).all(|p| p[0] == 0x47));
    }

    #[test]
    fn whole_frame() {
        let mut dec = BbFrameDecoder::new(ISI);
        let split = up(3);
        let mut df1 = concat(&[up(1), up(2)]);
        df1.extend_from_slice(&split[.. 100]);
        let mut df2 = split[100 ..].to_vec();
        df2.extend_from_slice(&up(4));
        assert_eq!(
            dec.push_frame(&frame(ISI, 188, 0, &df1)),
            concat(&[restored(1), restored(2)])
        );
        assert_eq!(
            dec.push_frame(&frame(ISI, 188, 88, &df2)),
            concat(&[restored(3), restored(4)])
        );
        assert!(dec.push_frame(&frame(2, 188, 0, &up(5))).is_empty());
        assert_eq!(dec.take_foreign_isi(), Some(2));
        assert!(dec.push_frame(&[0; 5]).is_empty());

        let big = concat(&(1 ..= 43).map(up).collect::<Vec<_>>());
        assert_eq!(
            dec.push_frame(&frame(ISI, 188, 0, &big)).len(),
            43 * PACKET_SIZE
        );
    }

    #[test]
    fn carry() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let split = up(3);
        let mut df1 = concat(&[up(1), up(2)]);
        df1.extend_from_slice(&split[.. 100]);
        let mut df2 = split[100 ..].to_vec();
        df2.extend_from_slice(&up(4));
        let mut pkts = fragments(&frame(ISI, 188, 0, &df1), &mut cc);
        pkts.extend(fragments(&frame(ISI, 188, 88, &df2), &mut cc));
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(5)), &mut cc));
        assert_eq!(
            feed(&mut dec, &pkts),
            concat(&[restored(1), restored(2), restored(3), restored(4)])
        );
    }

    #[test]
    fn unaligned() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let mut df1 = up(1).to_vec();
        df1.extend_from_slice(&up(2)[.. 100]);
        let mut pkts = fragments(&frame(ISI, 188, 0, &df1), &mut cc);
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(4)), &mut cc));
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(5)), &mut cc));
        assert_eq!(feed(&mut dec, &pkts), concat(&[restored(1), restored(4)]));
    }

    #[test]
    fn foreign_isi() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let mut pkts = fragments(&frame(5, 188, 0, &up(1)), &mut cc);
        pkts.extend(fragments(&frame(5, 188, 0, &up(2)), &mut cc));
        assert!(feed(&mut dec, &pkts).is_empty());
        assert_eq!(dec.take_foreign_isi(), Some(5));
        assert_eq!(dec.take_foreign_isi(), None);
        let pkts = fragments(&frame(5, 188, 0, &up(3)), &mut cc);
        assert!(feed(&mut dec, &pkts).is_empty());
        assert_eq!(dec.take_foreign_isi(), None);
    }

    #[test]
    fn bad_crc() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let split = up(2);
        let mut df1 = up(1).to_vec();
        df1.extend_from_slice(&split[.. 100]);
        let mut bad = frame(ISI, 188, 0, &up(9));
        bad[9] ^= 0xFF;
        let mut df3 = split[100 ..].to_vec();
        df3.extend_from_slice(&up(3));
        let mut pkts = fragments(&frame(ISI, 188, 0, &df1), &mut cc);
        pkts.extend(fragments(&bad, &mut cc));
        pkts.extend(fragments(&frame(ISI, 188, 88, &df3), &mut cc));
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(4)), &mut cc));
        assert_eq!(feed(&mut dec, &pkts), concat(&[restored(1), restored(3)]));
    }

    #[test]
    fn sequence_gap() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let (mut pkts, _) = aligned_stream(&mut cc);
        pkts.remove(1);
        assert!(feed(&mut dec, &pkts).is_empty());
    }

    #[test]
    fn sequence_gap_carry() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let mut pkts = fragments(&frame(ISI, 188, 0, &up(1)[.. 100]), &mut cc);

        let mut df2 = up(1)[100 ..].to_vec();
        df2.extend_from_slice(&up(2));
        df2.extend_from_slice(&up(3)[.. 100]);
        let mut lost = fragments(&frame(ISI, 188, 88, &df2), &mut cc);
        lost.remove(1);
        pkts.extend(lost);

        // SYNCD fits the stale carry, but the tail belongs to a different UP.
        let mut df3 = up(3)[100 ..].to_vec();
        df3.extend_from_slice(&up(4));
        pkts.extend(fragments(&frame(ISI, 188, 88, &df3), &mut cc));
        pkts.push(fragments(&frame(ISI, 188, 0, &up(5)), &mut cc)[0]);

        assert_eq!(feed(&mut dec, &pkts), restored(4));
    }

    #[test]
    fn slen_rejected() {
        for slen in [0x00, 0xB5] {
            let mut dec = BbFrameDecoder::new(ISI);
            let mut cc = 0;
            let (mut pkts, _) = aligned_stream(&mut cc);
            pkts[1][7] = slen;
            assert!(feed(&mut dec, &pkts).is_empty());
        }
    }

    #[test]
    fn cc_gap_resets() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let (mut pkts, _) = aligned_stream(&mut cc);
        let mut gap = [0xFFu8; PACKET_SIZE];
        gap[0] = 0x47;
        gap[1 .. 3].copy_from_slice(&BBFRAME_PID.to_be_bytes());
        gap[3] = 0x30 | ((pkts[0][3] + 5) & 0x0F);
        gap[4] = 1;
        gap[5] = 0x00;
        pkts.insert(1, gap);
        assert!(feed(&mut dec, &pkts).is_empty());
    }

    /// A cc gap with an adaptation field drops accumulated bytes only, not an empty first fragment
    #[test]
    fn cc_gap_empty_first() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let (mut pkts, expect) = aligned_stream(&mut cc);
        let next = pkts.pop().unwrap();
        for (n, p) in pkts.iter_mut().enumerate() {
            p[8] = n as u8 + 1;
        }
        let mut gap = [0xFFu8; PACKET_SIZE];
        gap[0] = 0x47;
        gap[1 .. 3].copy_from_slice(&BBFRAME_PID.to_be_bytes());
        gap[3] = 0x35;
        gap[4] = 1;
        gap[5] = 0x00;
        pkts.insert(0, gap);
        pkts.insert(0, fragment(14, 1, FRAGMENT_FIRST, &[]));
        pkts.push(next);
        assert_eq!(feed(&mut dec, &pkts), expect);
    }

    #[test]
    fn upl_shrink() {
        for (syncd, tail) in [(0, Some(3)), (10, None)] {
            let mut dec = BbFrameDecoder::new(ISI);
            let mut cc = 0;
            let mut df1 = Vec::new();
            for tag in 1 .. 3 {
                df1.extend_from_slice(&up(tag));
                df1.extend_from_slice(&[0xAA, 0xBB]);
            }
            df1.extend_from_slice(&up(3));
            let mut df2 = vec![0xEE; syncd];
            df2.extend_from_slice(&concat(&[up(4), up(5)]));
            let mut pkts = fragments(&frame(ISI, 190, 0, &df1), &mut cc);
            pkts.extend(fragments(&frame(ISI, 188, syncd as u16, &df2), &mut cc));
            pkts.extend(fragments(&frame(ISI, 188, 0, &up(6)), &mut cc));
            let mut expect = concat(&[restored(1), restored(2)]);
            expect.extend(tail.map(restored).iter().flatten());
            expect.extend_from_slice(&concat(&[restored(4), restored(5)]));
            assert_eq!(feed(&mut dec, &pkts), expect);
        }
    }

    #[test]
    fn upl_190() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let mut df = Vec::new();
        for tag in 1 .. 4 {
            df.extend_from_slice(&up(tag));
            df.extend_from_slice(&[0xAA, 0xBB]);
        }
        let mut pkts = fragments(&frame(ISI, 190, 0, &df), &mut cc);
        pkts.extend(fragments(&frame(ISI, 190, 0, &up(4)), &mut cc));
        assert_eq!(
            feed(&mut dec, &pkts),
            concat(&[restored(1), restored(2), restored(3)])
        );
    }

    #[test]
    fn upl_rejected() {
        for upl in [0, 100] {
            let mut dec = BbFrameDecoder::new(ISI);
            let mut cc = 0;
            let df = concat(&[up(1), up(2)]);
            let mut pkts = fragments(&frame(ISI, upl, 0, &df), &mut cc);
            pkts.extend(fragments(&frame(ISI, 188, 0, &up(3)), &mut cc));
            assert!(feed(&mut dec, &pkts).is_empty());
        }
    }

    #[test]
    fn overflow() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let big = vec![0x55u8; FRAME_MAX + 3 * 179];
        let mut pkts = fragments(&frame(ISI, 188, 0, &big[HEADER_LEN ..]), &mut cc);
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(3)), &mut cc));
        assert!(feed(&mut dec, &pkts).is_empty());
    }

    #[test]
    fn dfl_rejected() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let df = concat(&[up(1), up(2)]);
        let mut f = header(ISI, 188, df.len() as u16 + 500, 0).to_vec();
        f.extend_from_slice(&df);
        let mut pkts = fragments(&f, &mut cc);
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(3)), &mut cc));
        assert!(feed(&mut dec, &pkts).is_empty());
    }

    #[test]
    fn reset() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut cc = 0;
        let (pkts, _) = aligned_stream(&mut cc);
        feed(&mut dec, &pkts[.. 2]);
        dec.reset();
        assert!(feed(&mut dec, &pkts[2 ..]).is_empty());

        let mut pkts = fragments(&frame(5, 188, 0, &up(1)), &mut cc);
        pkts.extend(fragments(&frame(ISI, 188, 0, &up(2)), &mut cc));
        feed(&mut dec, &pkts);
        assert_eq!(dec.take_foreign_isi(), Some(5));
        dec.reset();
        feed(&mut dec, &pkts);
        assert_eq!(dec.take_foreign_isi(), Some(5));
    }

    /// High efficiency mode header: the mode bit is xored into the CRC-8 field
    fn header_hem(npd: bool, dfl: u16, syncd: u16) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        h[0] = 0xC0 | if npd { 0x04 } else { 0 };
        h[1] = ISI;
        h[4 .. 6].copy_from_slice(&(dfl << 3).to_be_bytes());
        h[7 .. 9].copy_from_slice(&syncd.to_be_bytes());
        h[9] = crc8(&h[.. 9]) ^ 0x01;
        h
    }

    fn frame_hem(npd: bool, syncd: Option<u16>, df: &[u8]) -> Vec<u8> {
        let mut f = header_hem(npd, df.len() as u16, syncd.map_or(0xFFFF, |s| s << 3)).to_vec();
        f.extend_from_slice(df);
        f
    }

    #[test]
    fn hem() {
        let mut dec = BbFrameDecoder::new(ISI);
        // UPs without the sync byte, a UP split over two frames, then null packet deletion
        let mut df1 = up(1)[1 ..].to_vec();
        df1.extend_from_slice(&up(2)[1 .. 60]);
        let mut df2 = up(2)[60 ..].to_vec();
        df2.extend_from_slice(&up(3)[1 ..]);
        assert_eq!(
            dec.push_frame(&frame_hem(false, Some(0), &df1)),
            restored(1)
        );
        assert_eq!(
            dec.push_frame(&frame_hem(false, Some(128), &df2)),
            concat(&[restored(2), restored(3)])
        );

        let mut df3 = up(4)[1 ..].to_vec();
        df3.push(0x05);
        df3.extend_from_slice(&up(5)[1 ..]);
        df3.push(0x00);
        assert_eq!(
            dec.push_frame(&frame_hem(true, Some(0), &df3)),
            concat(&[restored(4), restored(5)])
        );
    }

    #[test]
    fn syncd_none() {
        let mut dec = BbFrameDecoder::new(ISI);
        let split = up(1);
        // a UP spread over three frames: start, a data field without any UP start, end
        assert!(
            dec.push_frame(&frame(ISI, 188, 0, &split[.. 50]))
                .is_empty()
        );
        assert!(
            dec.push_frame(&frame_hem_normal(&split[50 .. 100]))
                .is_empty()
        );
        let mut df3 = split[100 ..].to_vec();
        df3.extend_from_slice(&up(2));
        assert_eq!(
            dec.push_frame(&frame(ISI, 188, 88, &df3)),
            concat(&[restored(1), restored(2)])
        );

        // nothing carried: a data field without UP start is unusable
        assert!(dec.push_frame(&frame_hem_normal(&[0x55; 30])).is_empty());
        assert_eq!(dec.push_frame(&frame(ISI, 188, 0, &up(3))), restored(3));
    }

    /// Normal mode frame with SYNCD 0xFFFF
    fn frame_hem_normal(df: &[u8]) -> Vec<u8> {
        let mut h = header(ISI, 188, df.len() as u16, 0);
        h[7 .. 9].copy_from_slice(&[0xFF, 0xFF]);
        h[9] = crc8(&h[.. 9]);
        let mut f = h.to_vec();
        f.extend_from_slice(df);
        f
    }

    #[test]
    fn matype() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut h = header(ISI, 188, 188, 0);
        h[0] = 0x80; // GSE
        h[9] = crc8(&h[.. 9]);
        let mut f = h.to_vec();
        f.extend_from_slice(&up(1));
        assert!(dec.push_frame(&f).is_empty());
        // mode field above 1: bad CRC
        let mut f = frame(ISI, 188, 0, &up(1));
        f[9] ^= 0x02;
        assert!(dec.push_frame(&f).is_empty());
    }

    #[test]
    fn garbage() {
        let mut dec = BbFrameDecoder::new(ISI);
        let mut x: u32 = 0x9E37_79B9;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0 .. 5000 {
            let mut p = [0u8; PACKET_SIZE];
            for b in &mut p {
                *b = next() as u8;
            }
            let r = next();
            if r & 1 == 0 {
                p[0] = 0x47;
                p[1 .. 3].copy_from_slice(&BBFRAME_PID.to_be_bytes());
                p[1] |= (r >> 8) as u8 & 0xE0;
            }
            if r & 2 == 0 {
                p[4 .. 7].copy_from_slice(&[0x00, 0x80, 0x00]);
            }
            if r & 4 == 0 {
                p[8] = [FRAGMENT_FIRST, 1, 2, 3][(r >> 16) as usize & 3];
            }
            if r & 8 == 0 {
                p[7] = (r >> 20) as u8 % 0xB6;
            }
            let len = if r & 16 == 0 {
                PACKET_SIZE
            } else {
                (r >> 24) as usize % 200
            };
            let out = dec.push(&p[.. len.min(PACKET_SIZE)]);
            assert_eq!(out.len() % PACKET_SIZE, 0);
            dec.take_foreign_isi();
        }
    }
}
