//! en50221 7.1 transport layer: command-response framing over a CiLink
//!
//! The transport owns the per-slot outgoing queue and busy flag, the
//! outgoing fragmentation of oversize SPDUs and the incoming reassembly
//! of TT_DATA_MORE fragments. The module never initiates: after every
//! successful write the slot is busy until the next read event for that
//! slot; outgoing TPDUs are queued per slot and flushed one at a time on
//! each read event (driven by the slot manager).

use std::collections::VecDeque;

use super::{
    CaDevice,
    apdu,
    apdu::ApduTag,
    spdu,
    tpdu,
    tpdu::{
        MAX_TPDU_DATA,
        MAX_TPDU_SIZE,
        TpduTag,
    },
};
use crate::error::{
    Error,
    Result,
};

/// Transport-level unit delivered by [`CiTransport::recv_apdu`]
#[derive(Debug)]
pub enum TransportRecv {
    /// TT_CTC_REPLY - the transport connection is established for the slot
    TcReply {
        /// slot the reply arrived on
        slot_id: u8,
    },
    /// A complete reassembled SPDU (for a session_number SPDU the tail is
    /// exactly one APDU - hence the method name recv_apdu)
    Spdu {
        /// slot the SPDU arrived on
        slot_id: u8,
        /// reassembled SPDU bytes
        spdu: Vec<u8>,
    },
    /// Status-only R_TPDU or an intermediate TT_DATA_MORE fragment;
    /// nothing to dispatch, but the busy/queue state advanced
    Status {
        /// slot the frame arrived on
        slot_id: u8,
    },
    /// A frame attributable to a slot that failed the strict validation
    /// (corrupt status trailer, length mismatch, unexpected tag,
    /// reassembly overflow); the frame content is dropped but the busy
    /// flag was cleared so the slot keeps going (legacy parity: real CAMs
    /// emit quirky frames and the command-response cycle must survive)
    Malformed {
        /// slot the frame arrived on
        slot_id: u8,
        /// human-readable description of the violation
        context: String,
    },
}

impl TransportRecv {
    /// Slot the received unit belongs to
    pub fn slot_id(&self) -> u8 {
        match self {
            TransportRecv::TcReply { slot_id }
            | TransportRecv::Spdu { slot_id, .. }
            | TransportRecv::Status { slot_id }
            | TransportRecv::Malformed { slot_id, .. } => *slot_id,
        }
    }
}

/// Per-slot transport state
struct TransportSlot {
    /// a write was issued; wait for the next read event for this slot
    busy: bool,
    /// pending framed TPDUs, flushed one per read event
    queue: VecDeque<Vec<u8>>,
    /// TT_DATA_MORE reassembly buffer, capped at MAX_TPDU_SIZE
    rx_buffer: Vec<u8>,
    /// DATA_INDICATOR seen on the last received frame
    data_pending: bool,
}

impl TransportSlot {
    fn new() -> Self {
        TransportSlot {
            busy: false,
            queue: VecDeque::new(),
            rx_buffer: Vec::new(),
            data_pending: false,
        }
    }
}

/// en50221 7.1 transport layer: command-response framing over a [`CiLink`]
///
/// The module never initiates: after every successful write the slot is
/// busy until the next read event for that slot. Outgoing TPDUs are
/// queued per slot and flushed one at a time on each read event.
pub struct CiTransport {
    link: CaDevice,
    slots: Vec<TransportSlot>,
    /// single read scratch buffer, like the legacy ca_buffer
    rx: Box<[u8; MAX_TPDU_SIZE]>,
}

impl CiTransport {
    /// Creates a transport for `slots_num` slots over the given link
    ///
    /// `slots_num` typically comes from `CaDevice::caps().slot_num`.
    pub fn new(link: CaDevice, slots_num: u8) -> Self {
        CiTransport {
            link,
            slots: (0 .. slots_num).map(|_| TransportSlot::new()).collect(),
            rx: Box::new([0; MAX_TPDU_SIZE]),
        }
    }

    /// Returns a reference to the underlying link
    pub fn link(&self) -> &CaDevice {
        &self.link
    }

    /// Returns a mutable reference to the underlying link
    pub fn link_mut(&mut self) -> &mut CaDevice {
        &mut self.link
    }

    /// Number of slots the transport was created with
    pub fn slots_num(&self) -> u8 {
        self.slots.len() as u8
    }

    fn check_slot(&self, slot_id: u8) -> Result<()> {
        if usize::from(slot_id) < self.slots.len() {
            Ok(())
        } else {
            Err(Error::InvalidProperty(format!(
                "ca invalid slot id {}",
                slot_id
            )))
        }
    }

    /// Builds a session_number SPDU + APDU and sends it, fragmenting into
    /// TT_DATA_MORE chunks of `MAX_TPDU_DATA` plus a final TT_DATA_LAST
    pub fn send_apdu(
        &mut self,
        slot_id: u8,
        session_id: u16,
        tag: ApduTag,
        body: &[u8],
    ) -> Result<()> {
        if body.len() > usize::from(u16::MAX) {
            return Err(Error::InvalidProperty(format!(
                "ca apdu body is too large: {} bytes",
                body.len()
            )));
        }

        let mut blob = spdu::build_session_number(session_id);
        apdu::build(&mut blob, tag, body);

        self.send_spdu(slot_id, &blob)
    }

    /// Sends a raw SPDU (session-control responses built by the session
    /// layer); fragments exactly like [`CiTransport::send_apdu`]
    pub fn send_spdu(&mut self, slot_id: u8, spdu: &[u8]) -> Result<()> {
        self.check_slot(slot_id)?;

        let mut offset = 0;
        while spdu.len() - offset > MAX_TPDU_DATA {
            self.send_tpdu(
                slot_id,
                TpduTag::DATA_MORE,
                &spdu[offset .. offset + MAX_TPDU_DATA],
            )?;
            offset += MAX_TPDU_DATA;
        }

        self.send_tpdu(slot_id, TpduTag::DATA_LAST, &spdu[offset ..])
    }

    /// Queues one TPDU (TT_CREATE_TC, TT_RCV, the empty TT_DATA_LAST
    /// poll, ...) and flushes immediately when the slot is idle
    pub fn send_tpdu(&mut self, slot_id: u8, tag: TpduTag, data: &[u8]) -> Result<()> {
        self.check_slot(slot_id)?;

        let frame = tpdu::build(slot_id, tag, data)?;
        self.slots[usize::from(slot_id)].queue.push_back(frame);

        self.flush(slot_id)
    }

    /// Writes the next queued TPDU if the slot is idle (one frame only)
    ///
    /// A link write error propagates as `Err` with the frame dropped and
    /// the slot left idle (legacy parity); the slot manager resets the
    /// slot in that case.
    pub fn flush(&mut self, slot_id: u8) -> Result<()> {
        self.check_slot(slot_id)?;

        let slot = &mut self.slots[usize::from(slot_id)];
        if slot.busy {
            return Ok(());
        }
        let frame = match slot.queue.pop_front() {
            Some(frame) => frame,
            None => return Ok(()),
        };

        self.link.send_msg(&frame)?;
        self.slots[usize::from(slot_id)].busy = true;

        Ok(())
    }

    /// Pulls one frame from the link and advances the transport state
    ///
    /// Returns `Ok(None)` when the link has no data. Clears the busy flag
    /// for the slot, accumulates TT_DATA_MORE fragments (bounded at
    /// `MAX_TPDU_SIZE` (2048), overflow drops the buffer) and records the
    /// DATA_INDICATOR status bit.
    pub fn recv_apdu(&mut self) -> Result<Option<TransportRecv>> {
        let len = match self.link.recv_msg(&mut self.rx[..])? {
            Some(len) => len,
            None => return Ok(None),
        };
        if len == 0 {
            // a zero-length read is end-of-stream
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "ca link closed (zero-length read)",
            )));
        }

        let slots_num = self.slots.len() as u8;

        let frame_slot = tpdu::frame_slot_id(&self.rx[.. len], slots_num);
        if let Some(slot_id) = frame_slot {
            self.slots[usize::from(slot_id)].busy = false;
        }

        let parsed = match tpdu::parse(&self.rx[.. len], slots_num) {
            Ok(parsed) => parsed,
            Err(Error::InvalidData(context)) => {
                return match frame_slot {
                    // the frame content is dropped, the slot keeps going
                    Some(slot_id) => Ok(Some(TransportRecv::Malformed { slot_id, context })),
                    None => Err(Error::InvalidData(context)),
                };
            }
            Err(e) => return Err(e),
        };
        let slot_id = parsed.slot_id;
        let tag = parsed.tag;
        let data_indicator = parsed.data_indicator;

        let slot = self
            .slots
            .get_mut(usize::from(slot_id))
            .expect("tpdu::parse bounds the slot id");

        slot.data_pending = data_indicator;

        match tag {
            TpduTag::CTC_REPLY => Ok(Some(TransportRecv::TcReply { slot_id })),
            TpduTag::DATA_MORE | TpduTag::DATA_LAST => {
                if slot.rx_buffer.len() + parsed.body.len() > MAX_TPDU_SIZE {
                    slot.rx_buffer.clear();
                    return Ok(Some(TransportRecv::Malformed {
                        slot_id,
                        context: format!("ca slot {}: tpdu reassembly buffer overflow", slot_id),
                    }));
                }
                slot.rx_buffer.extend_from_slice(parsed.body);

                if tag == TpduTag::DATA_MORE || slot.rx_buffer.is_empty() {
                    // intermediate fragment, or an empty poll response
                    Ok(Some(TransportRecv::Status { slot_id }))
                } else {
                    let spdu = std::mem::take(&mut slot.rx_buffer);
                    Ok(Some(TransportRecv::Spdu { slot_id, spdu }))
                }
            }
            TpduTag::SB => Ok(Some(TransportRecv::Status { slot_id })),
            // DTC_REPLY, REQUEST_TC, NEW_TC, TC_ERROR: parsed but not
            // expected by this host - dropped, slot keeps going
            tag => Ok(Some(TransportRecv::Malformed {
                slot_id,
                context: format!("ca slot {}: unexpected tpdu tag {:?}", slot_id, tag),
            })),
        }
    }

    /// Returns true while the slot waits for a read event after a write
    pub fn is_busy(&self, slot_id: u8) -> bool {
        self.slots
            .get(usize::from(slot_id))
            .is_some_and(|slot| slot.busy)
    }

    /// Number of queued (not yet written) TPDUs for the slot
    pub fn queue_len(&self, slot_id: u8) -> usize {
        self.slots
            .get(usize::from(slot_id))
            .map_or(0, |slot| slot.queue.len())
    }

    /// Takes and clears the DATA_INDICATOR flag for the slot
    pub fn take_data_pending(&mut self, slot_id: u8) -> bool {
        match self.slots.get_mut(usize::from(slot_id)) {
            Some(slot) => std::mem::take(&mut slot.data_pending),
            None => false,
        }
    }

    /// Clears the queue, busy flag and reassembly buffer for the slot
    pub fn clear_slot(&mut self, slot_id: u8) {
        if let Some(slot) = self.slots.get_mut(usize::from(slot_id)) {
            slot.busy = false;
            slot.queue.clear();
            slot.rx_buffer.clear();
            slot.data_pending = false;
        }
    }
}
