//! en50221 7.2: session layer
//!
//! The session layer multiplexes sessions between module applications
//! and host resources on top of the transport layer. The module opens
//! sessions to host resources; the host allocates session numbers,
//! dispatches incoming APDUs to the resources and reports the activity
//! as [`CaEvent`].

use std::{
    collections::VecDeque,
    os::{
        fd::{
            AsFd,
            BorrowedFd,
        },
        unix::io::{
            AsRawFd,
            RawFd,
        },
    },
};

use super::{
    apdu,
    apdu::ApduTag,
    resource::{
        ApplicationInfo,
        MmiMenu,
        ResourceContext,
        ResourceId,
        ResourceRegistry,
        mmi,
    },
    spdu,
    spdu::Spdu,
    transport::{
        CiTransport,
        TransportRecv,
    },
};
use crate::error::{
    Error,
    Result,
};

/// Highest number of concurrent sessions per slot
const MAX_SLOT_SESSIONS: usize = 16;

/// Session layer activity delivered by [`CiSession::next_event`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaEvent {
    /// TT_CTC_REPLY: the transport connection is established and the
    /// module is about to open sessions
    TransportReady { slot_id: u8 },
    /// A frame or an object attributable to a slot failed the
    /// validation; the data is dropped but the slot keeps going
    Malformed { slot_id: u8, context: String },
    /// The module opened a session to a host resource
    SessionOpened {
        slot_id: u8,
        session_id: u16,
        resource_id: ResourceId,
    },
    /// The module requested a session the host refused; `status` is the
    /// en50221 Table 7 value: 0xF0 - the resource does not exist,
    /// 0xF2 - only a lower version is available, 0xF3 - no free
    /// session numbers
    SessionRefused {
        slot_id: u8,
        resource_id: ResourceId,
        status: u8,
    },
    /// The session is gone: module close request, host close completion
    /// or slot drop
    SessionClosed {
        slot_id: u8,
        session_id: u16,
        resource_id: ResourceId,
    },
    /// application_info: the module identified itself
    ApplicationInfo { slot_id: u8, info: ApplicationInfo },
    /// close_mmi: the module asks to close the dialogue; `delay` is the
    /// close delay in seconds when the module asked for a deferred close
    MmiClose { slot_id: u8, delay: Option<u8> },
    /// text_last: a standalone text object to display
    MmiText { slot_id: u8, text: Vec<u8> },
    /// menu_last: a menu to display; the selection is answered with
    /// [`CiSession::mmi_menu_answer`]
    MmiMenu { slot_id: u8, menu: MmiMenu },
    /// list_last: a list to display; unlike a menu it needs no answer
    MmiList { slot_id: u8, menu: MmiMenu },
    /// enq: the module asks the user for a text answer (a PIN code
    /// usually), answered with [`CiSession::mmi_answer`]
    MmiEnq {
        slot_id: u8,
        /// mask the user input
        blind: bool,
        /// expected answer length
        answer_len: u8,
        /// prompt in DVB charset coding (EN 300 468 annex A)
        text: Vec<u8>,
    },
    /// tune: the module asks the host to tune to the service
    Tune {
        slot_id: u8,
        network_id: u16,
        original_network_id: u16,
        transport_stream_id: u16,
        service_id: u16,
    },
    /// replace: the module asks to substitute a PID in the stream
    /// passed through it
    Replace {
        slot_id: u8,
        replace_ref: u8,
        replaced_pid: u16,
        replacement_pid: u16,
    },
    /// clear_replace: the module withdraws a replace request
    ClearReplace { slot_id: u8, replace_ref: u8 },
}

/// State of one open session
enum SessionState {
    Active,
    /// the host sent close_session_request and waits for the response
    Closing,
}

struct Session {
    slot_id: u8,
    resource_id: ResourceId,
    state: SessionState,
}

/// en50221 7.2 session layer on top of a [`CiTransport`]
///
/// The layer is driven from the outside: [`CiSession::recv`] consumes
/// one link frame, [`CiSession::tick`] runs the time-based work of the
/// resources (periodic date_time updates), [`CiSession::next_event`]
/// drains the queued activity.
///
/// ```no_run
/// use libdvb::ca::{CaDevice, CiSession, CiTransport};
///
/// fn main() -> libdvb::error::Result<()> {
///     let device = CaDevice::open(0, 0)?;
///     let slots_num = device.caps()?.slot_num as u8;
///     let mut session = CiSession::new(CiTransport::new(device, slots_num));
///
///     // on every read event of the device descriptor:
///     while session.recv()? {}
///     while let Some(event) = session.next_event() {
///         println!("{:?}", event);
///     }
///     // and periodically:
///     session.tick()?;
///
///     Ok(())
/// }
/// ```
pub struct CiSession {
    transport: CiTransport,
    resources: ResourceRegistry,
    /// session state indexed by session number - 1
    sessions: Vec<Option<Session>>,
    events: VecDeque<CaEvent>,
}

impl AsRawFd for CiSession {
    fn as_raw_fd(&self) -> RawFd {
        self.transport.as_raw_fd()
    }
}

impl AsFd for CiSession {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.transport.as_fd()
    }
}

impl CiSession {
    /// Creates the session layer over the given transport
    pub fn new(transport: CiTransport) -> Self {
        CiSession {
            transport,
            resources: ResourceRegistry::new(),
            sessions: Vec::new(),
            events: VecDeque::new(),
        }
    }

    /// Returns a reference to the underlying transport
    pub fn transport(&self) -> &CiTransport {
        &self.transport
    }

    /// Returns a mutable reference to the underlying transport
    pub fn transport_mut(&mut self) -> &mut CiTransport {
        &mut self.transport
    }

    /// Takes the next queued event
    pub fn next_event(&mut self) -> Option<CaEvent> {
        self.events.pop_front()
    }

    /// Pulls one frame from the transport and advances the session
    /// state. Returns `Ok(false)` when the link has no data. Call
    /// [`CiSession::next_event`] to drain the produced events.
    pub fn recv(&mut self) -> Result<bool> {
        let recv = match self.transport.recv_apdu()? {
            Some(recv) => recv,
            None => return Ok(false),
        };
        let slot_id = recv.slot_id();

        match recv {
            TransportRecv::TcReply { slot_id } => {
                self.events.push_back(CaEvent::TransportReady { slot_id });
            }
            TransportRecv::Spdu { slot_id, spdu } => self.dispatch_spdu(slot_id, &spdu)?,
            TransportRecv::Status { .. } => {}
            TransportRecv::Malformed { slot_id, context } => {
                self.events
                    .push_back(CaEvent::Malformed { slot_id, context });
            }
        }

        // the read event allows the next queued frame out
        self.transport.flush(slot_id)?;

        Ok(true)
    }

    /// Runs the time-based work of the resources: periodic date_time
    /// updates. Call once a second or so.
    pub fn tick(&mut self) -> Result<()> {
        self.resources.tick(&mut self.transport)
    }

    /// Closes all sessions of the slot and clears its transport state;
    /// for the slot manager to call when the module is gone or the
    /// transport connection is reset
    pub fn drop_slot(&mut self, slot_id: u8) {
        self.transport.clear_slot(slot_id);

        for index in 0 .. self.sessions.len() {
            let matches = self.sessions[index]
                .as_ref()
                .is_some_and(|session| session.slot_id == slot_id);
            if matches {
                self.free_session((index + 1) as u16);
            }
        }
    }

    /// Last application info received from the module in the slot
    pub fn app_info(&self, slot_id: u8) -> Option<&ApplicationInfo> {
        self.resources.application_info.info(slot_id)
    }

    /// Asks the module to show its menu (enter_menu on the application
    /// information session)
    pub fn enter_menu(&mut self, slot_id: u8) -> Result<()> {
        let session_id = self.find_session(slot_id, ResourceId::APPLICATION_INFORMATION)?;
        self.transport
            .send_apdu(slot_id, session_id, ApduTag::ENTER_MENU, &[])
    }

    /// Answers a [`CaEvent::MmiMenu`] selection with the 1-based item
    /// number; 0 cancels the menu
    pub fn mmi_menu_answer(&mut self, slot_id: u8, choice: u8) -> Result<()> {
        let session_id = self.find_session(slot_id, ResourceId::MMI)?;
        self.transport
            .send_apdu(slot_id, session_id, ApduTag::MENU_ANSW, &[choice])
    }

    /// Answers a [`CaEvent::MmiEnq`] enquiry; `None` cancels the
    /// enquiry
    pub fn mmi_answer(&mut self, slot_id: u8, answer: Option<&[u8]>) -> Result<()> {
        let session_id = self.find_session(slot_id, ResourceId::MMI)?;
        self.transport
            .send_apdu(slot_id, session_id, ApduTag::ANSW, &mmi::build_answ(answer))
    }

    /// Asks the module to close the MMI dialogue; the module closes the
    /// session in response
    pub fn mmi_close(&mut self, slot_id: u8) -> Result<()> {
        let session_id = self.find_session(slot_id, ResourceId::MMI)?;
        self.transport
            .send_apdu(slot_id, session_id, ApduTag::CLOSE_MMI, &mmi::build_close())
    }

    /// Asks the module to release the host control resource
    pub fn ask_release(&mut self, slot_id: u8) -> Result<()> {
        let session_id = self.find_session(slot_id, ResourceId::HOST_CONTROL)?;
        self.transport
            .send_apdu(slot_id, session_id, ApduTag::ASK_RELEASE, &[])
    }

    fn session(&self, session_id: u16) -> Option<&Session> {
        let index = usize::from(session_id.checked_sub(1)?);
        self.sessions.get(index)?.as_ref()
    }

    /// First active session of the slot connected to the resource
    fn find_session(&self, slot_id: u8, resource_id: ResourceId) -> Result<u16> {
        for (index, entry) in self.sessions.iter().enumerate() {
            if let Some(session) = entry
                && session.slot_id == slot_id
                && session.resource_id.base() == resource_id.base()
                && matches!(session.state, SessionState::Active)
            {
                return Ok((index + 1) as u16);
            }
        }

        Err(Error::InvalidProperty(format!(
            "ca slot {}: no open {:?} session",
            slot_id, resource_id
        )))
    }

    fn alloc_session(&mut self, slot_id: u8, resource_id: ResourceId) -> Option<u16> {
        // the pool is per slot: one module cannot starve the others
        let in_use = self
            .sessions
            .iter()
            .flatten()
            .filter(|session| session.slot_id == slot_id)
            .count();
        if in_use >= MAX_SLOT_SESSIONS {
            return None;
        }

        let index = match self.sessions.iter().position(Option::is_none) {
            Some(index) => index,
            None => {
                self.sessions.push(None);
                self.sessions.len() - 1
            }
        };

        self.sessions[index] = Some(Session {
            slot_id,
            resource_id,
            state: SessionState::Active,
        });

        Some((index + 1) as u16)
    }

    /// Frees the session and runs the resource close callback
    fn free_session(&mut self, session_id: u16) {
        let Some(index) = session_id.checked_sub(1) else {
            return;
        };
        let Some(session) = self
            .sessions
            .get_mut(usize::from(index))
            .and_then(Option::take)
        else {
            return;
        };

        if let Some(resource) = self.resources.lookup(session.resource_id) {
            resource.on_close(session.slot_id, session_id);
        }

        self.events.push_back(CaEvent::SessionClosed {
            slot_id: session.slot_id,
            session_id,
            resource_id: session.resource_id,
        });
    }

    fn dispatch_spdu(&mut self, slot_id: u8, data: &[u8]) -> Result<()> {
        let spdu = match spdu::parse(data) {
            Ok(spdu) => spdu,
            Err(Error::InvalidData(context)) => {
                self.events
                    .push_back(CaEvent::Malformed { slot_id, context });
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        match spdu {
            Spdu::SessionNumber { session_id, apdu } => {
                self.dispatch_apdu(slot_id, session_id, apdu)
            }
            Spdu::OpenSessionRequest { resource_id } => self.open_session(slot_id, resource_id),
            Spdu::CloseSessionRequest { session_id } => self.close_session(slot_id, session_id),
            Spdu::CloseSessionResponse { status, session_id } => {
                self.close_session_complete(slot_id, session_id, status);
                Ok(())
            }
            Spdu::CreateSessionResponse { .. } => {
                // the host never sends create_session
                self.events.push_back(CaEvent::Malformed {
                    slot_id,
                    context: format!("ca slot {}: unexpected create_session_response", slot_id),
                });
                Ok(())
            }
        }
    }

    /// Handles open_session_request: allocates a session, replies and
    /// runs the resource open callback
    fn open_session(&mut self, slot_id: u8, resource_id: ResourceId) -> Result<()> {
        let mut session_id = 0;
        let status = match self.resources.lookup(resource_id) {
            None => spdu::SS_NOT_ALLOCATED,
            Some(resource) if resource_id.version() > resource.resource_id().version() => {
                spdu::SS_LOWER_VERSION
            }
            Some(_) => match self.alloc_session(slot_id, resource_id) {
                Some(allocated) => {
                    session_id = allocated;
                    spdu::SS_OK
                }
                None => spdu::SS_BUSY,
            },
        };

        let response = spdu::build_open_session_response(status, resource_id, session_id);
        self.transport.send_spdu(slot_id, &response)?;

        if status != spdu::SS_OK {
            self.events.push_back(CaEvent::SessionRefused {
                slot_id,
                resource_id,
                status,
            });
            return Ok(());
        }

        self.events.push_back(CaEvent::SessionOpened {
            slot_id,
            session_id,
            resource_id,
        });

        let resource = self
            .resources
            .lookup(resource_id)
            .expect("the resource is present: the session was allocated");
        let mut ctx = ResourceContext {
            transport: &mut self.transport,
            events: &mut self.events,
            slot_id,
            session_id,
            close_session: false,
        };

        let result = resource.on_open(&mut ctx);
        Self::resource_result(&mut self.events, slot_id, result)
    }

    /// Dispatches the APDUs of a session_number SPDU to the resource
    /// bound to the session
    fn dispatch_apdu(&mut self, slot_id: u8, session_id: u16, data: &[u8]) -> Result<()> {
        let resource_id = match self.session(session_id) {
            Some(session) if session.slot_id == slot_id => session.resource_id,
            _ => {
                self.events.push_back(CaEvent::Malformed {
                    slot_id,
                    context: format!(
                        "ca slot {}: apdu on unknown session {}",
                        slot_id, session_id
                    ),
                });
                return Ok(());
            }
        };

        let resource = self
            .resources
            .lookup(resource_id)
            .expect("the resource is present: the session was allocated");

        let mut close_session = false;
        for item in apdu::iter(data) {
            match item {
                Ok(item) => {
                    let mut ctx = ResourceContext {
                        transport: &mut self.transport,
                        events: &mut self.events,
                        slot_id,
                        session_id,
                        close_session: false,
                    };

                    let result = resource.on_apdu(&mut ctx, item.tag, item.body);
                    close_session |= ctx.close_session;
                    Self::resource_result(&mut self.events, slot_id, result)?;
                }
                Err(Error::InvalidData(context)) => {
                    self.events
                        .push_back(CaEvent::Malformed { slot_id, context });
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        if close_session {
            self.request_close(slot_id, session_id)?;
        }

        Ok(())
    }

    /// Turns a resource-level data error into a Malformed event; the
    /// slot keeps going
    fn resource_result(
        events: &mut VecDeque<CaEvent>,
        slot_id: u8,
        result: Result<()>,
    ) -> Result<()> {
        match result {
            Err(Error::InvalidData(context)) => {
                events.push_back(CaEvent::Malformed { slot_id, context });
                Ok(())
            }
            result => result,
        }
    }

    /// Starts a host-initiated session close; a second request for a
    /// session already closing is not sent
    fn request_close(&mut self, slot_id: u8, session_id: u16) -> Result<()> {
        let Some(index) = session_id.checked_sub(1).map(usize::from) else {
            return Ok(());
        };
        match self.sessions.get_mut(index) {
            Some(Some(session))
                if session.slot_id == slot_id && matches!(session.state, SessionState::Active) =>
            {
                session.state = SessionState::Closing;
            }
            _ => return Ok(()),
        }

        let request = spdu::build_close_session_request(session_id);
        self.transport.send_spdu(slot_id, &request)
    }

    /// Handles close_session_request from the module
    fn close_session(&mut self, slot_id: u8, session_id: u16) -> Result<()> {
        let known = matches!(
            self.session(session_id),
            Some(session) if session.slot_id == slot_id
        );
        let status = if known {
            spdu::SS_OK
        } else {
            spdu::SS_NOT_ALLOCATED
        };

        let response = spdu::build_close_session_response(status, session_id);
        self.transport.send_spdu(slot_id, &response)?;

        if known {
            self.free_session(session_id);
        } else {
            self.events.push_back(CaEvent::Malformed {
                slot_id,
                context: format!(
                    "ca slot {}: close request for unknown session {}",
                    slot_id, session_id
                ),
            });
        }

        Ok(())
    }

    /// Handles close_session_response completing a host-initiated close
    fn close_session_complete(&mut self, slot_id: u8, session_id: u16, status: u8) {
        let closing = matches!(
            self.session(session_id),
            Some(session) if session.slot_id == slot_id
                && matches!(session.state, SessionState::Closing)
        );

        if !closing {
            self.events.push_back(CaEvent::Malformed {
                slot_id,
                context: format!(
                    "ca slot {}: unexpected close response for session {}",
                    slot_id, session_id
                ),
            });
            return;
        }

        match status {
            // SS_NOT_ALLOCATED also means that the peer has no session to
            // communicate on, so the local half must be released.
            spdu::SS_OK | spdu::SS_NOT_ALLOCATED => self.free_session(session_id),
            status => {
                if let Some(Some(session)) = self.sessions.get_mut(usize::from(session_id - 1)) {
                    session.state = SessionState::Active;
                }
                self.events.push_back(CaEvent::Malformed {
                    slot_id,
                    context: format!(
                        "ca slot {}: invalid close response status 0x{:02X} for session {}",
                        slot_id, status, session_id
                    ),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{
            ErrorKind,
            Read,
            Write,
        },
        os::{
            fd::OwnedFd,
            unix::net::UnixDatagram,
        },
    };

    use super::{
        super::{
            CaDevice,
            asn1,
            tpdu::TpduTag,
        },
        *,
    };

    /// The module side of a socketpair link: SOCK_DGRAM is available on
    /// Linux and macOS and keeps message boundaries like the kernel CA
    /// device does
    struct TestCam {
        file: File,
    }

    fn pair_slots(slots_num: u8) -> (CiSession, TestCam) {
        let (host, cam) = UnixDatagram::pair().unwrap();
        host.set_nonblocking(true).unwrap();
        cam.set_nonblocking(true).unwrap();

        let host = File::from(OwnedFd::from(host));
        let cam = File::from(OwnedFd::from(cam));

        let session = CiSession::new(CiTransport::new(CaDevice::from_file(host), slots_num));

        (session, TestCam { file: cam })
    }

    fn pair() -> (CiSession, TestCam) {
        pair_slots(1)
    }

    impl TestCam {
        /// Wraps the payload into an R_TPDU with a clean status trailer
        /// and writes it to the link
        fn send_slot(&mut self, slot_id: u8, tag: TpduTag, payload: &[u8]) {
            let t_c_id = slot_id + 1;
            let mut frame = vec![slot_id, t_c_id];

            match tag {
                TpduTag::SB => {}
                TpduTag::CTC_REPLY => frame.extend_from_slice(&[tag.raw(), 0x01, t_c_id]),
                TpduTag::DATA_LAST | TpduTag::DATA_MORE => {
                    frame.push(tag.raw());
                    asn1::encode(payload.len() as u16 + 1, &mut frame);
                    frame.push(t_c_id);
                    frame.extend_from_slice(payload);
                }
                tag => panic!("unsupported test tag {:?}", tag),
            }

            frame.extend_from_slice(&[TpduTag::SB.raw(), 0x02, t_c_id, 0x00]);
            self.file.write_all(&frame).unwrap();
        }

        fn send(&mut self, tag: TpduTag, payload: &[u8]) {
            self.send_slot(0, tag, payload);
        }

        /// Sends an SPDU wrapped into a data R_TPDU on slot 0
        fn send_spdu(&mut self, spdu: &[u8]) {
            self.send(TpduTag::DATA_LAST, spdu);
        }

        /// Sends an APDU on the session wrapped into a data R_TPDU on
        /// slot 0
        fn send_apdu(&mut self, session_id: u16, tag: ApduTag, body: &[u8]) {
            let mut payload = spdu::build_session_number(session_id);
            apdu::build(&mut payload, tag, body);
            self.send_spdu(&payload);
        }

        /// Reads one C_TPDU frame from the link
        fn recv(&mut self) -> Option<Vec<u8>> {
            let mut buf = [0; 2048];
            match self.file.read(&mut buf) {
                Ok(len) => Some(buf[.. len].to_vec()),
                Err(e) if e.kind() == ErrorKind::WouldBlock => None,
                Err(e) => panic!("cam link read: {}", e),
            }
        }

        /// Extracts the SPDU payload from a host data C_TPDU
        fn unwrap_data_slot(frame: &[u8], slot_id: u8) -> Vec<u8> {
            let t_c_id = slot_id + 1;
            assert_eq!(&frame[.. 2], &[slot_id, t_c_id], "slot and t_c_id");
            assert_eq!(frame[2], TpduTag::DATA_LAST.raw(), "data frame tag");
            let (length, consumed) = asn1::decode(&frame[3 ..]).unwrap();
            assert_eq!(frame[3 + consumed], t_c_id, "t_c_id byte");
            let start = 3 + consumed + 1;
            assert_eq!(frame.len(), start + usize::from(length) - 1);

            frame[start ..].to_vec()
        }
    }

    /// Runs the host receive loop against the module side. Every frame
    /// the module reads is acknowledged with a status R_TPDU so the
    /// host flushes the next queued frame; returns the SPDU payloads
    /// the host sent. All frames must arrive on the given slot.
    fn pump_slot(session: &mut CiSession, cam: &mut TestCam, slot_id: u8) -> Vec<Vec<u8>> {
        let mut spdus = Vec::new();

        loop {
            while session.recv().unwrap() {}
            match cam.recv() {
                Some(frame) => {
                    spdus.push(TestCam::unwrap_data_slot(&frame, slot_id));
                    // en50221 7.1: the host must not send the next frame
                    // before the module responds to the previous one
                    assert!(
                        cam.recv().is_none(),
                        "two outstanding frames on slot {}",
                        slot_id
                    );
                    cam.send_slot(slot_id, TpduTag::SB, &[]);
                }
                None => break,
            }
        }

        spdus
    }

    fn pump(session: &mut CiSession, cam: &mut TestCam) -> Vec<Vec<u8>> {
        pump_slot(session, cam, 0)
    }

    fn events(session: &mut CiSession) -> Vec<CaEvent> {
        let mut events = Vec::new();
        while let Some(event) = session.next_event() {
            events.push(event);
        }
        events
    }

    fn open_session_request(resource_id: ResourceId) -> Vec<u8> {
        let raw = resource_id.raw();
        vec![
            0x91,
            0x04,
            (raw >> 24) as u8,
            (raw >> 16) as u8,
            (raw >> 8) as u8,
            raw as u8,
        ]
    }

    /// Opens a session to the resource consuming the handshake frames;
    /// returns the allocated session id
    fn open_session(session: &mut CiSession, cam: &mut TestCam, resource_id: ResourceId) -> u16 {
        cam.send_spdu(&open_session_request(resource_id));
        pump(session, cam);

        match events(session).first() {
            Some(&CaEvent::SessionOpened { session_id, .. }) => session_id,
            event => panic!("expected SessionOpened, got {:?}", event),
        }
    }

    #[test]
    fn test_transport_ready() {
        let (mut session, mut cam) = pair();

        cam.send(TpduTag::CTC_REPLY, &[]);
        assert!(session.recv().unwrap());
        assert_eq!(
            session.next_event(),
            Some(CaEvent::TransportReady { slot_id: 0 })
        );
    }

    #[test]
    fn test_rm_handshake() {
        let (mut session, mut cam) = pair();

        // the module opens a session to the resource manager
        cam.send_spdu(&open_session_request(ResourceId::RESOURCE_MANAGER));
        let spdus = pump(&mut session, &mut cam);

        // open_session_response (ok, session 1), then profile_enq
        assert_eq!(spdus.len(), 2);
        assert_eq!(
            spdus[0],
            vec![0x92, 0x07, 0x00, 0x00, 0x01, 0x00, 0x41, 0x00, 0x01]
        );
        assert_eq!(
            spdus[1],
            vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x10, 0x00]
        );
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionOpened {
                slot_id: 0,
                session_id: 1,
                resource_id: ResourceId::RESOURCE_MANAGER,
            }]
        );

        // empty module profile: the host replies profile_change
        cam.send_apdu(1, ApduTag::PROFILE, &[]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x12, 0x00]]
        );

        // profile_enq: the host replies profile with its resource list
        cam.send_apdu(1, ApduTag::PROFILE_ENQ, &[]);
        let spdus = pump(&mut session, &mut cam);
        let mut expected = vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x11, 20];
        expected.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x41, // Resource Manager
            0x00, 0x02, 0x00, 0x41, // Application Information
            0x00, 0x20, 0x00, 0x41, // Host Control
            0x00, 0x24, 0x00, 0x41, // Date-Time
            0x00, 0x40, 0x00, 0x41, // MMI
        ]);
        assert_eq!(spdus, vec![expected]);
        assert!(events(&mut session).is_empty());
    }

    #[test]
    fn test_open_session_refused() {
        let (mut session, mut cam) = pair();

        // the CA support resource is not implemented yet
        cam.send_spdu(&open_session_request(
            ResourceId::CONDITIONAL_ACCESS_SUPPORT,
        ));
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x92, 0x07, 0xF0, 0x00, 0x03, 0x00, 0x41, 0x00, 0x00]]
        );
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionRefused {
                slot_id: 0,
                resource_id: ResourceId::CONDITIONAL_ACCESS_SUPPORT,
                status: 0xF0,
            }]
        );

        // a higher resource version than the host provides
        cam.send_spdu(&open_session_request(ResourceId::new(0x0001_0042)));
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x92, 0x07, 0xF2, 0x00, 0x01, 0x00, 0x42, 0x00, 0x00]]
        );
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionRefused {
                slot_id: 0,
                resource_id: ResourceId::new(0x0001_0042),
                status: 0xF2,
            }]
        );
    }

    #[test]
    fn test_session_exhaustion() {
        let (mut session, mut cam) = pair();

        for expected in 1 ..= 16 {
            assert_eq!(
                open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL),
                expected
            );
        }

        // the 17th session on the slot is refused as busy
        cam.send_spdu(&open_session_request(ResourceId::HOST_CONTROL));
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x92, 0x07, 0xF3, 0x00, 0x20, 0x00, 0x41, 0x00, 0x00]]
        );
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionRefused {
                slot_id: 0,
                resource_id: ResourceId::HOST_CONTROL,
                status: 0xF3,
            }]
        );

        // closing one session makes room again
        cam.send_spdu(&[0x95, 0x02, 0x00, 0x05]);
        pump(&mut session, &mut cam);
        events(&mut session);
        assert_eq!(
            open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL),
            5
        );
    }

    #[test]
    fn test_application_info() {
        let (mut session, mut cam) = pair();

        cam.send_spdu(&open_session_request(ResourceId::APPLICATION_INFORMATION));
        let spdus = pump(&mut session, &mut cam);
        // open_session_response, then application_info_enq
        assert_eq!(spdus.len(), 2);
        assert_eq!(
            spdus[1],
            vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x20, 0x00]
        );
        events(&mut session);

        let info_body = [
            0x01, 0x12, 0x34, 0x56, 0x78, 0x08, b'T', b'e', b's', b't', b' ', b'C', b'A', b'M',
        ];
        cam.send_apdu(1, ApduTag::APPLICATION_INFO, &info_body);
        pump(&mut session, &mut cam);

        let info = ApplicationInfo {
            application_type: 0x01,
            application_manufacturer: 0x1234,
            manufacturer_code: 0x5678,
            menu_string: b"Test CAM".to_vec(),
        };
        assert_eq!(
            events(&mut session),
            vec![CaEvent::ApplicationInfo {
                slot_id: 0,
                info: info.clone(),
            }]
        );
        assert_eq!(session.app_info(0), Some(&info));
        assert_eq!(session.app_info(1), None);

        // enter_menu goes out on the application information session
        session.enter_menu(0).unwrap();
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x22, 0x00]]
        );
    }

    #[test]
    fn test_mmi_dialogue() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);
        assert_eq!(session_id, 1);

        // display_control set_mmi_mode high level -> display_reply ack
        cam.send_apdu(session_id, ApduTag::DISPLAY_CONTROL, &[0x01, 0x01]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![
                0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x02, 0x02, 0x01, 0x01
            ]]
        );

        // menu with two items
        let mut menu_body = vec![0x02];
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Menu");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Info");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Exit");
        cam.send_apdu(session_id, ApduTag::MENU_LAST, &menu_body);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiMenu {
                slot_id: 0,
                menu: MmiMenu {
                    title: b"Menu".to_vec(),
                    sub_title: Vec::new(),
                    bottom: Vec::new(),
                    items: vec![b"Info".to_vec(), b"Exit".to_vec()],
                },
            }]
        );

        // the user picks the second item
        session.mmi_menu_answer(0, 2).unwrap();
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x0B, 0x01, 0x02]]
        );

        // a blind enquiry (PIN code)
        let mut enq_body = vec![0x01, 0x04];
        enq_body.extend_from_slice(b"PIN:");
        cam.send_apdu(session_id, ApduTag::ENQ, &enq_body);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiEnq {
                slot_id: 0,
                blind: true,
                answer_len: 4,
                text: b"PIN:".to_vec(),
            }]
        );

        session.mmi_answer(0, Some(b"1234")).unwrap();
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![
                0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x08, 0x05, 0x01, b'1', b'2', b'3', b'4',
            ]]
        );

        // the module closes the dialogue: the host requests the session
        // close and completes it on the module response
        cam.send_apdu(session_id, ApduTag::CLOSE_MMI, &[0x00]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x95, 0x02, 0x00, 0x01]]);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiClose {
                slot_id: 0,
                delay: None,
            }]
        );

        cam.send_spdu(&[0x96, 0x03, 0x00, 0x00, 0x01]);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionClosed {
                slot_id: 0,
                session_id: 1,
                resource_id: ResourceId::MMI,
            }]
        );

        // the session is gone
        assert!(session.mmi_menu_answer(0, 1).is_err());
    }

    #[test]
    fn test_fragmented_spdu() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::APPLICATION_INFORMATION);

        // an application_info too large for one link frame arrives as
        // TT_DATA_MORE + TT_DATA_LAST and is reassembled by the transport
        let mut body = vec![0x01, 0x12, 0x34, 0x56, 0x78, 200];
        body.extend_from_slice(&[b'x'; 200]);
        let mut payload = spdu::build_session_number(session_id);
        apdu::build(&mut payload, ApduTag::APPLICATION_INFO, &body);

        let (head, tail) = payload.split_at(100);
        cam.send(TpduTag::DATA_MORE, head);
        cam.send(TpduTag::DATA_LAST, tail);
        pump(&mut session, &mut cam);

        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::ApplicationInfo { slot_id: 0, .. }]
        ));
        assert_eq!(session.app_info(0).unwrap().menu_string, vec![b'x'; 200]);
    }

    #[test]
    fn test_mmi_chained_objects() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);

        // a menu split into menu_more + menu_last fragments
        let mut menu_body = vec![0x02];
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Menu");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Info");
        apdu::build(&mut menu_body, ApduTag::TEXT_LAST, b"Exit");
        let (head, tail) = menu_body.split_at(7);

        cam.send_apdu(session_id, ApduTag::MENU_MORE, head);
        cam.send_apdu(session_id, ApduTag::MENU_LAST, tail);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiMenu {
                slot_id: 0,
                menu: MmiMenu {
                    title: b"Menu".to_vec(),
                    sub_title: Vec::new(),
                    bottom: Vec::new(),
                    items: vec![b"Info".to_vec(), b"Exit".to_vec()],
                },
            }]
        );

        // a standalone text split into text_more + text_last
        cam.send_apdu(session_id, ApduTag::TEXT_MORE, b"Hello, ");
        cam.send_apdu(session_id, ApduTag::TEXT_LAST, b"world");
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiText {
                slot_id: 0,
                text: b"Hello, world".to_vec(),
            }]
        );

        // a list split into list_more + list_last
        let (head, tail) = menu_body.split_at(10);
        cam.send_apdu(session_id, ApduTag::LIST_MORE, head);
        cam.send_apdu(session_id, ApduTag::LIST_LAST, tail);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiList {
                slot_id: 0,
                menu: MmiMenu {
                    title: b"Menu".to_vec(),
                    sub_title: Vec::new(),
                    bottom: Vec::new(),
                    items: vec![b"Info".to_vec(), b"Exit".to_vec()],
                },
            }]
        );
    }

    #[test]
    fn test_mmi_display_control_errors() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);

        // an unknown display_control command
        cam.send_apdu(session_id, ApduTag::DISPLAY_CONTROL, &[0x02]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x02, 0x01, 0xF0]]
        );

        // set_mmi_mode with an unsupported mode
        cam.send_apdu(session_id, ApduTag::DISPLAY_CONTROL, &[0x01, 0x02]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x02, 0x01, 0xF1]]
        );
    }

    #[test]
    fn test_mmi_close_delay() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);

        // a deferred close carries the delay byte
        cam.send_apdu(session_id, ApduTag::CLOSE_MMI, &[0x01, 0x05]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x95, 0x02, 0x00, 0x01]]);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiClose {
                slot_id: 0,
                delay: Some(5),
            }]
        );
    }

    #[test]
    fn test_closing_session_races() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);

        // close_mmi puts the session into the closing state
        cam.send_apdu(session_id, ApduTag::CLOSE_MMI, &[0x00]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x95, 0x02, 0x00, 0x01]]);
        events(&mut session);

        // data in flight is still delivered while closing
        cam.send_apdu(session_id, ApduTag::TEXT_LAST, b"bye");
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::MmiText {
                slot_id: 0,
                text: b"bye".to_vec(),
            }]
        );

        // a duplicate close_mmi does not send a second close request
        cam.send_apdu(session_id, ApduTag::CLOSE_MMI, &[0x00]);
        let spdus = pump(&mut session, &mut cam);
        assert!(spdus.is_empty());
        events(&mut session);

        // the module close request crosses the host one: the host
        // replies and frees the session
        cam.send_spdu(&[0x95, 0x02, 0x00, 0x01]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x96, 0x03, 0x00, 0x00, 0x01]]);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionClosed {
                slot_id: 0,
                session_id: 1,
                resource_id: ResourceId::MMI,
            }]
        );

        // the stale response to the host close request is reported
        cam.send_spdu(&[0x96, 0x03, 0x00, 0x00, 0x01]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));
    }

    #[test]
    fn test_close_session_response_status() {
        // F0 says that the peer no longer has the session. The local half is
        // released just like it is after a successful close.
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);
        session.request_close(0, session_id).unwrap();
        pump(&mut session, &mut cam);

        cam.send_spdu(&[0x96, 0x03, spdu::SS_NOT_ALLOCATED, 0x00, 0x01]);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionClosed {
                slot_id: 0,
                session_id,
                resource_id: ResourceId::MMI,
            }]
        );
        assert!(session.mmi_menu_answer(0, 1).is_err());

        // Reserved status values do not prove that the peer closed the
        // session. Report the bad response and restore the active state so
        // the application is not left with an unusable closing session.
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::MMI);
        session.request_close(0, session_id).unwrap();
        pump(&mut session, &mut cam);

        cam.send_spdu(&[0x96, 0x03, 0x01, 0x00, 0x01]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        session.mmi_menu_answer(0, 1).unwrap();
        assert_eq!(
            pump(&mut session, &mut cam),
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x88, 0x0B, 0x01, 0x01]]
        );
    }

    #[test]
    fn test_malformed_spdu() {
        let (mut session, mut cam) = pair();

        // a wrong spdu length field
        cam.send_spdu(&[0x91, 0x05, 0x00, 0x01, 0x00, 0x41]);
        let spdus = pump(&mut session, &mut cam);
        assert!(spdus.is_empty());
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        // create_session is never sent to a host
        cam.send_spdu(&[0x93, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        // the slot keeps working
        assert_eq!(
            open_session(&mut session, &mut cam, ResourceId::RESOURCE_MANAGER),
            1
        );
    }

    #[test]
    fn test_rm_profile_change_from_module() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::RESOURCE_MANAGER);

        // the module profile changed: the host re-enquires
        cam.send_apdu(session_id, ApduTag::PROFILE_CHANGE, &[]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x80, 0x10, 0x00]]
        );
        assert!(events(&mut session).is_empty());
    }

    #[test]
    fn test_date_time_periodic() {
        let (mut session, mut cam) = pair();

        cam.send_spdu(&open_session_request(ResourceId::DATE_TIME));
        pump(&mut session, &mut cam);
        events(&mut session);

        cam.send_apdu(1, ApduTag::DATE_TIME_ENQ, &[30]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus.len(), 1);

        // nothing to send before the interval elapses
        session.tick().unwrap();
        assert!(pump(&mut session, &mut cam).is_empty());

        // the interval elapsed: tick resends the time
        session.resources.date_time.backdate(1, 31);
        session.tick().unwrap();
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus.len(), 1);
        assert_eq!(
            &spdus[0][.. 8],
            &[0x90, 0x02, 0x00, 0x01, 0x9F, 0x84, 0x41, 0x05]
        );

        // and the send resets the interval timer
        session.tick().unwrap();
        assert!(pump(&mut session, &mut cam).is_empty());
    }

    #[test]
    fn test_date_time_enq_empty_body() {
        let (mut session, mut cam) = pair();

        cam.send_spdu(&open_session_request(ResourceId::DATE_TIME));
        pump(&mut session, &mut cam);
        events(&mut session);

        cam.send_apdu(1, ApduTag::DATE_TIME_ENQ, &[]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus.len(), 1);
        assert_eq!(
            &spdus[0][.. 8],
            &[0x90, 0x02, 0x00, 0x01, 0x9F, 0x84, 0x41, 0x05]
        );
        assert!(events(&mut session).is_empty());
    }

    #[test]
    fn test_two_slots() {
        let (mut session, mut cam) = pair_slots(2);

        // slot 0: host control, slot 1: mmi
        let first = open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL);
        assert_eq!(first, 1);

        cam.send_slot(
            1,
            TpduTag::DATA_LAST,
            &open_session_request(ResourceId::MMI),
        );
        let spdus = pump_slot(&mut session, &mut cam, 1);
        assert_eq!(
            spdus,
            vec![vec![0x92, 0x07, 0x00, 0x00, 0x40, 0x00, 0x41, 0x00, 0x02]]
        );
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionOpened {
                slot_id: 1,
                session_id: 2,
                resource_id: ResourceId::MMI,
            }]
        );

        // session 1 belongs to slot 0: an apdu for it on slot 1 is refused
        let mut payload = spdu::build_session_number(1);
        apdu::build(&mut payload, ApduTag::CLEAR_REPLACE, &[0x01]);
        cam.send_slot(1, TpduTag::DATA_LAST, &payload);
        pump_slot(&mut session, &mut cam, 1);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 1, .. }]
        ));

        // dropping slot 0 leaves the slot 1 session alive
        session.drop_slot(0);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionClosed {
                slot_id: 0,
                session_id: 1,
                resource_id: ResourceId::HOST_CONTROL,
            }]
        );

        session.mmi_close(1).unwrap();
        let spdus = pump_slot(&mut session, &mut cam, 1);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x02, 0x9F, 0x88, 0x00, 0x01, 0x00]]
        );
    }

    #[test]
    fn test_date_time() {
        let (mut session, mut cam) = pair();

        cam.send_spdu(&open_session_request(ResourceId::DATE_TIME));
        let spdus = pump(&mut session, &mut cam);
        // open_session_response, then an unsolicited date_time object
        assert_eq!(spdus.len(), 2);
        assert_eq!(
            &spdus[1][.. 8],
            &[0x90, 0x02, 0x00, 0x01, 0x9F, 0x84, 0x41, 0x05]
        );
        assert_eq!(spdus[1].len(), 13);
        // MJD 61041 is 2026-01-01: the test runs later than that
        let mjd = (u16::from(spdus[1][8]) << 8) | u16::from(spdus[1][9]);
        assert!(mjd >= 61041, "mjd {}", mjd);

        // date_time_enq: the host replies right away
        cam.send_apdu(1, ApduTag::DATE_TIME_ENQ, &[30]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus.len(), 1);
        assert_eq!(
            &spdus[0][.. 8],
            &[0x90, 0x02, 0x00, 0x01, 0x9F, 0x84, 0x41, 0x05]
        );

        // the 30 seconds interval has not elapsed: tick sends nothing
        session.tick().unwrap();
        assert!(pump(&mut session, &mut cam).is_empty());
    }

    #[test]
    fn test_host_control() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL);

        cam.send_apdu(
            session_id,
            ApduTag::TUNE,
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        );
        cam.send_apdu(
            session_id,
            ApduTag::REPLACE,
            &[0x07, 0xFF, 0xFE, 0xE1, 0x23],
        );
        cam.send_apdu(session_id, ApduTag::CLEAR_REPLACE, &[0x07]);
        pump(&mut session, &mut cam);

        assert_eq!(
            events(&mut session),
            vec![
                CaEvent::Tune {
                    slot_id: 0,
                    network_id: 0x1122,
                    original_network_id: 0x3344,
                    transport_stream_id: 0x5566,
                    service_id: 0x7788,
                },
                CaEvent::Replace {
                    slot_id: 0,
                    replace_ref: 0x07,
                    replaced_pid: 0x1FFE,
                    replacement_pid: 0x0123,
                },
                CaEvent::ClearReplace {
                    slot_id: 0,
                    replace_ref: 0x07,
                },
            ]
        );

        session.ask_release(0).unwrap();
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(
            spdus,
            vec![vec![0x90, 0x02, 0x00, 0x01, 0x9F, 0x84, 0x03, 0x00]]
        );
    }

    #[test]
    fn test_session_number_reuse() {
        let (mut session, mut cam) = pair();

        assert_eq!(
            open_session(&mut session, &mut cam, ResourceId::RESOURCE_MANAGER),
            1
        );
        assert_eq!(
            open_session(&mut session, &mut cam, ResourceId::APPLICATION_INFORMATION),
            2
        );

        // the module closes the first session
        cam.send_spdu(&[0x95, 0x02, 0x00, 0x01]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x96, 0x03, 0x00, 0x00, 0x01]]);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::SessionClosed {
                slot_id: 0,
                session_id: 1,
                resource_id: ResourceId::RESOURCE_MANAGER,
            }]
        );

        // the freed session number is allocated again
        assert_eq!(open_session(&mut session, &mut cam, ResourceId::MMI), 1);
    }

    #[test]
    fn test_close_unknown_session() {
        let (mut session, mut cam) = pair();

        cam.send_spdu(&[0x95, 0x02, 0x00, 0x63]);
        let spdus = pump(&mut session, &mut cam);
        assert_eq!(spdus, vec![vec![0x96, 0x03, 0xF0, 0x00, 0x63]]);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));
    }

    #[test]
    fn test_malformed_apdu_keeps_slot_going() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL);

        // an apdu on a session that was never opened
        cam.send_apdu(0x63, ApduTag::TUNE, &[0; 8]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        // an apdu the resource does not accept
        cam.send_apdu(session_id, ApduTag::PROFILE_ENQ, &[]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        // a truncated apdu body
        cam.send_apdu(session_id, ApduTag::TUNE, &[0x11, 0x22]);
        pump(&mut session, &mut cam);
        assert!(matches!(
            events(&mut session).as_slice(),
            [CaEvent::Malformed { slot_id: 0, .. }]
        ));

        // the session keeps working after all of that
        cam.send_apdu(session_id, ApduTag::CLEAR_REPLACE, &[0x01]);
        pump(&mut session, &mut cam);
        assert_eq!(
            events(&mut session),
            vec![CaEvent::ClearReplace {
                slot_id: 0,
                replace_ref: 0x01,
            }]
        );
    }

    #[test]
    fn test_empty_session_number_tail() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::RESOURCE_MANAGER);

        cam.send_spdu(&spdu::build_session_number(session_id));
        pump(&mut session, &mut cam);
        assert!(events(&mut session).is_empty());
    }

    #[test]
    fn test_packed_apdus() {
        let (mut session, mut cam) = pair();
        let session_id = open_session(&mut session, &mut cam, ResourceId::HOST_CONTROL);

        // two apdus packed into one spdu
        let mut payload = spdu::build_session_number(session_id);
        apdu::build(
            &mut payload,
            ApduTag::REPLACE,
            &[0x01, 0x00, 0x64, 0x00, 0xC8],
        );
        apdu::build(&mut payload, ApduTag::CLEAR_REPLACE, &[0x01]);
        cam.send_spdu(&payload);
        pump(&mut session, &mut cam);

        assert_eq!(
            events(&mut session),
            vec![
                CaEvent::Replace {
                    slot_id: 0,
                    replace_ref: 0x01,
                    replaced_pid: 0x0064,
                    replacement_pid: 0x00C8,
                },
                CaEvent::ClearReplace {
                    slot_id: 0,
                    replace_ref: 0x01,
                },
            ]
        );
    }

    #[test]
    fn test_drop_slot() {
        let (mut session, mut cam) = pair();

        open_session(&mut session, &mut cam, ResourceId::APPLICATION_INFORMATION);
        open_session(&mut session, &mut cam, ResourceId::MMI);
        cam.send_apdu(
            1,
            ApduTag::APPLICATION_INFO,
            &[0x01, 0x12, 0x34, 0x56, 0x78, 0x00],
        );
        pump(&mut session, &mut cam);
        events(&mut session);
        assert!(session.app_info(0).is_some());

        session.drop_slot(0);
        assert_eq!(
            events(&mut session),
            vec![
                CaEvent::SessionClosed {
                    slot_id: 0,
                    session_id: 1,
                    resource_id: ResourceId::APPLICATION_INFORMATION,
                },
                CaEvent::SessionClosed {
                    slot_id: 0,
                    session_id: 2,
                    resource_id: ResourceId::MMI,
                },
            ]
        );
        // the stored application info went away with the slot
        assert!(session.app_info(0).is_none());
        assert!(session.mmi_menu_answer(0, 1).is_err());

        // the slot is usable again
        assert_eq!(
            open_session(&mut session, &mut cam, ResourceId::RESOURCE_MANAGER),
            1
        );
    }
}
