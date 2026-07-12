//! High-level, externally driven DVB-CI slot controller
//!
//! [`CiController`] owns the physical slot lifecycle and drives the
//! existing transport/session primitives without creating a thread or an
//! event loop. The caller watches [`AsFd::as_fd`], drains
//! [`CiController::poll_event`] when readable and calls
//! [`CiController::tick`] with a monotonic time value.

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
    time::{
        Duration,
        Instant,
    },
};

use super::{
    CaDevice,
    resource::{
        ApplicationInfo,
        ResourceId,
    },
    session::{
        CaEvent,
        CiSession,
    },
    sys::{
        CA_CI_LINK,
        CA_CI_MODULE_PRESENT,
        CA_CI_MODULE_READY,
        CaSlotInfo,
    },
    tpdu::TpduTag,
    transport::CiTransport,
};
use crate::error::{
    Error,
    Result,
};

/// Physical and transport-connection state of one CI slot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaSlotStatus {
    /// No module is inserted
    Absent,
    /// A module is present, but is not ready or has no transport
    /// connection yet
    Present,
    /// TT_CREATE_TC was issued and the controller waits for TT_CTC_REPLY
    CreatingTc,
    /// The transport connection is active
    Active,
    /// The connection failed and is either waiting for recovery or cannot
    /// be used
    Failed,
}

/// Confirmed application-level progress of a CAM
///
/// The current resource set can advance this status through
/// [`CamStatus::ApplicationInfo`]. Conditional Access Support will use
/// the remaining states when it is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CamStatus {
    /// No confirmed CAM application data
    None,
    /// A valid APPLICATION_INFO object was received
    ApplicationInfo,
    /// A valid CA_INFO object was received
    CaInfo,
    /// All resources required for CA PMT operation are ready
    Ready,
}

/// Reason why the controller abandoned a slot connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaSlotFailure {
    /// The slot does not expose the CI link-layer interface
    UnsupportedInterface { slot_type: u32 },
    /// TT_CTC_REPLY did not arrive in time
    CreateTcTimeout,
    /// An active command did not receive a response in time
    ResponseTimeout,
    /// A queued command could not be written before the timeout
    WriteTimeout,
    /// Reading slot information failed
    SlotInfoFailed,
    /// Link-layer I/O failed
    LinkFailed,
    /// The global CA_RESET ioctl failed
    ResetFailed,
}

/// Timings used by [`CiController`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CiControllerConfig {
    /// How often `CA_GET_SLOT_INFO` is queried
    pub slot_status_interval: Duration,
    /// Interval between empty TT_DATA_LAST polls while the slot is idle
    pub transport_poll_interval: Duration,
    /// Maximum time to wait for TT_CTC_REPLY
    pub create_tc_timeout: Duration,
    /// Maximum time to write an active command or wait for its response
    pub response_timeout: Duration,
    /// Delay before retrying after a successful global reset
    pub retry_interval: Duration,
}

impl Default for CiControllerConfig {
    fn default() -> Self {
        CiControllerConfig {
            slot_status_interval: Duration::from_millis(250),
            transport_poll_interval: Duration::from_millis(100),
            create_tc_timeout: Duration::from_secs(2),
            response_timeout: Duration::from_secs(2),
            retry_interval: Duration::from_secs(1),
        }
    }
}

struct ControllerSlot {
    status: CaSlotStatus,
    cam_status: CamStatus,
    present: bool,
    ready: bool,
    /// Time when CREATE_TC was actually accepted by the link. Unlike an
    /// active command timer this survives unrelated status responses.
    create_since: Option<Instant>,
    command_since: Option<Instant>,
    receive_pending: bool,
    next_poll: Option<Instant>,
    retry_at: Option<Instant>,
}

impl ControllerSlot {
    fn new() -> Self {
        ControllerSlot {
            status: CaSlotStatus::Absent,
            cam_status: CamStatus::None,
            present: false,
            ready: false,
            create_since: None,
            command_since: None,
            receive_pending: false,
            next_poll: None,
            retry_at: None,
        }
    }
}

/// Control-plane operations kept separate from the data link so the state
/// machine can be tested without DVB hardware.
trait ControllerIo: Send + Sync {
    fn reset(&self, device: &CaDevice) -> Result<()>;
    fn slot_info(&self, device: &CaDevice, slot_id: u8) -> Result<CaSlotInfo>;
}

struct KernelControllerIo;

impl ControllerIo for KernelControllerIo {
    fn reset(&self, device: &CaDevice) -> Result<()> {
        device.reset()
    }

    fn slot_info(&self, device: &CaDevice, slot_id: u8) -> Result<CaSlotInfo> {
        device.slot_info(slot_id)
    }
}

/// High-level DVB-CI controller over the EN 50221 session stack
///
/// This type is deliberately runtime-neutral: it never blocks and creates
/// no worker thread. `poll_event()` consumes already-readable link frames;
/// `tick(now)` performs status checks, transport polling and timeout work.
///
/// Linux `CA_RESET` resets the whole CA interface rather than one slot.
/// Consequently recovery of one failed slot clears the transport and
/// session state of every slot owned by this controller. `SlotFailed`
/// identifies the initiating slot; collateral slots report their transition
/// to [`CaSlotStatus::Failed`] through `SlotStatusChanged`.
pub struct CiController {
    session: CiSession,
    slots: Vec<ControllerSlot>,
    events: VecDeque<CaEvent>,
    config: CiControllerConfig,
    io: Box<dyn ControllerIo>,
    next_status_check: Option<Instant>,
    last_tick: Option<Instant>,
    link_suspended: bool,
    recovery_at: Option<Instant>,
    deferred_link_failure: bool,
}

impl AsRawFd for CiController {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl AsFd for CiController {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.session.as_fd()
    }
}

impl CiController {
    /// Opens a CA device and creates a controller with default timings
    pub fn open(adapter: u32, device: u32) -> Result<Self> {
        Self::new(CaDevice::open(adapter, device)?)
    }

    /// Creates a controller with default timings
    pub fn new(device: CaDevice) -> Result<Self> {
        Self::with_config(device, CiControllerConfig::default())
    }

    /// Creates a controller with explicit timings
    pub fn with_config(device: CaDevice, config: CiControllerConfig) -> Result<Self> {
        let caps = device.caps()?;
        if caps.slot_num == 0 {
            return Err(Error::InvalidProperty(
                "ca device has no module slots".to_owned(),
            ));
        }
        let slots_num = u8::try_from(caps.slot_num).map_err(|_| {
            Error::InvalidProperty(format!(
                "ca device has too many module slots: {}",
                caps.slot_num
            ))
        })?;
        if (caps.slot_type & CA_CI_LINK) == 0 {
            return Err(Error::InvalidProperty(format!(
                "ca device does not support the CI link interface: 0x{:X}",
                caps.slot_type
            )));
        }

        device.reset()?;
        let session = CiSession::new(CiTransport::new(device, slots_num));
        Ok(Self::from_parts(
            session,
            config,
            Box::new(KernelControllerIo),
        ))
    }

    fn from_parts(
        session: CiSession,
        config: CiControllerConfig,
        io: Box<dyn ControllerIo>,
    ) -> Self {
        let slots_num = session.transport().slots_num();
        CiController {
            session,
            slots: (0 .. slots_num).map(|_| ControllerSlot::new()).collect(),
            events: VecDeque::new(),
            config,
            io,
            next_status_check: None,
            last_tick: None,
            link_suspended: false,
            recovery_at: None,
            deferred_link_failure: false,
        }
    }

    /// Number of CI slots managed by the controller
    pub fn slots_num(&self) -> u8 {
        self.slots.len() as u8
    }

    /// Physical/transport status of a slot
    pub fn status(&self, slot_id: u8) -> Result<CaSlotStatus> {
        self.slots
            .get(usize::from(slot_id))
            .map(|slot| slot.status)
            .ok_or_else(|| Error::InvalidProperty(format!("ca invalid slot id {}", slot_id)))
    }

    /// Confirmed CAM application status of a slot
    pub fn cam_status(&self, slot_id: u8) -> Result<CamStatus> {
        self.slots
            .get(usize::from(slot_id))
            .map(|slot| slot.cam_status)
            .ok_or_else(|| Error::InvalidProperty(format!("ca invalid slot id {}", slot_id)))
    }

    /// Last application information received from a CAM
    pub fn app_info(&self, slot_id: u8) -> Option<&ApplicationInfo> {
        self.session.app_info(slot_id)
    }

    /// Asks the CAM to enter its menu
    pub fn enter_menu(&mut self, slot_id: u8) -> Result<()> {
        self.require_active(slot_id)?;
        let result = self.session.enter_menu(slot_id);
        self.finish_slot_command(slot_id, result)
    }

    /// Answers a menu selection with a 1-based item number; 0 cancels
    pub fn mmi_menu_answer(&mut self, slot_id: u8, choice: u8) -> Result<()> {
        self.require_active(slot_id)?;
        let result = self.session.mmi_menu_answer(slot_id, choice);
        self.finish_slot_command(slot_id, result)
    }

    /// Answers an enquiry; `None` cancels it
    pub fn mmi_answer(&mut self, slot_id: u8, answer: Option<&[u8]>) -> Result<()> {
        self.require_active(slot_id)?;
        let result = self.session.mmi_answer(slot_id, answer);
        self.finish_slot_command(slot_id, result)
    }

    /// Asks the CAM to close the current MMI dialogue
    pub fn mmi_close(&mut self, slot_id: u8) -> Result<()> {
        self.require_active(slot_id)?;
        let result = self.session.mmi_close(slot_id);
        self.finish_slot_command(slot_id, result)
    }

    /// Asks the CAM to release the host-control resource
    pub fn ask_release(&mut self, slot_id: u8) -> Result<()> {
        self.require_active(slot_id)?;
        let result = self.session.ask_release(slot_id);
        self.finish_slot_command(slot_id, result)
    }

    /// Resets the complete CA interface and drops all slot sessions
    ///
    /// A following `tick(now)` re-reads the physical slot flags and starts
    /// new transport connections for ready modules.
    pub fn reset(&mut self) -> Result<()> {
        self.collect_session_events();
        self.drop_all_slots();
        self.deferred_link_failure = false;

        match self.io.reset(self.session.transport().link()) {
            Ok(()) => {
                for slot_id in 0 .. self.slots_num() {
                    let present = self.slots[usize::from(slot_id)].present;
                    self.slots[usize::from(slot_id)].ready = false;
                    self.slots[usize::from(slot_id)].retry_at = None;
                    self.set_cam_status(slot_id, CamStatus::None);
                    self.set_slot_status(
                        slot_id,
                        if present {
                            CaSlotStatus::Present
                        } else {
                            CaSlotStatus::Absent
                        },
                    );
                }
                self.next_status_check = None;
                self.link_suspended = false;
                self.recovery_at = None;
                self.deferred_link_failure = false;
                Ok(())
            }
            Err(error) => {
                self.link_suspended = true;
                self.recovery_at = None;
                self.mark_reset_failed();
                Err(error)
            }
        }
    }

    /// Returns one queued event, reading non-blocking link frames as needed
    ///
    /// Call repeatedly until `Ok(None)` after the descriptor becomes
    /// readable or after `tick()`. Status-only frames are consumed until an
    /// application-visible event is produced or the link would block.
    /// Derived status changes precede their causal `TransportReady` or
    /// `ApplicationInfo` event; teardown events precede the final physical
    /// slot-status change.
    pub fn poll_event(&mut self) -> Result<Option<CaEvent>> {
        self.collect_session_events();
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        if self.link_suspended {
            return Ok(None);
        }

        loop {
            let active_slots: Vec<bool> = self
                .slots
                .iter()
                .map(|slot| slot.status == CaSlotStatus::Active)
                .collect();
            let slot_id = match self.session.recv_controller(&active_slots) {
                Ok(Some(slot_id)) => slot_id,
                Ok(None) => return Ok(self.events.pop_front()),
                Err(error) => {
                    if let Some(now) = self.event_time() {
                        let _ = self.recover_all(None, CaSlotFailure::LinkFailed, now);
                    } else {
                        self.deferred_link_failure = true;
                    }
                    return Err(error);
                }
            };

            if self.slots[usize::from(slot_id)].status == CaSlotStatus::Active {
                self.slots[usize::from(slot_id)].command_since = None;
            }
            let data_pending = self.session.transport_mut().take_data_pending(slot_id);

            // TT_CTC_REPLY must activate the slot before DATA_INDICATOR is
            // acted upon.
            self.collect_session_events();
            if data_pending && self.status(slot_id)? == CaSlotStatus::Active {
                self.slots[usize::from(slot_id)].receive_pending = true;
            }

            let now = self.event_time();
            if let Some(now) = now
                && let Err(error) = self.flush_receive_pending(slot_id, now)
            {
                if let Some(now) = self.event_time() {
                    let _ = self.recover_all(Some(slot_id), CaSlotFailure::LinkFailed, now);
                }
                return Err(error);
            }

            if let Some(now) = now {
                self.mark_outstanding(slot_id, now);
            }

            if let Some(event) = self.events.pop_front() {
                return Ok(Some(event));
            }
        }
    }

    /// Advances physical slot checks, transport polling and timeouts
    /// without blocking
    pub fn tick(&mut self, now: Instant) -> Result<()> {
        self.last_tick = Some(now);
        self.collect_session_events();

        if std::mem::take(&mut self.deferred_link_failure) {
            self.recover_all(None, CaSlotFailure::LinkFailed, now)?;
            return Ok(());
        }

        if self.link_suspended {
            match self.recovery_at {
                Some(recovery_at) if now >= recovery_at => {
                    self.link_suspended = false;
                    self.recovery_at = None;
                }
                _ => return Ok(()),
            }
        }

        if self
            .next_status_check
            .is_none_or(|deadline| now >= deadline)
        {
            self.refresh_slot_info(now)?;
            self.next_status_check = Some(deadline(now, self.config.slot_status_interval));
        }

        if let Err(error) = self.session.tick_at(now) {
            if is_link_error(&error) {
                let _ = self.recover_all(None, CaSlotFailure::LinkFailed, now);
            }
            return Err(error);
        }

        for slot_id in 0 .. self.slots_num() {
            match self.drive_slot(slot_id, now) {
                Ok(None) => {}
                Ok(Some(reason)) => {
                    self.recover_all(Some(slot_id), reason, now)?;
                    break;
                }
                Err(error) => {
                    let _ = self.recover_all(Some(slot_id), CaSlotFailure::LinkFailed, now);
                    return Err(error);
                }
            }
        }

        self.collect_session_events();
        Ok(())
    }

    fn require_active(&self, slot_id: u8) -> Result<()> {
        match self.status(slot_id)? {
            CaSlotStatus::Active => Ok(()),
            status => Err(Error::InvalidProperty(format!(
                "ca slot {} is not active ({:?})",
                slot_id, status
            ))),
        }
    }

    fn finish_slot_command(&mut self, slot_id: u8, result: Result<()>) -> Result<()> {
        match result {
            Ok(()) => {
                if let Some(now) = self.event_time() {
                    self.mark_outstanding(slot_id, now);
                }
                Ok(())
            }
            Err(error) => {
                if is_link_error(&error)
                    && let Some(now) = self.event_time()
                {
                    let _ = self.recover_all(Some(slot_id), CaSlotFailure::LinkFailed, now);
                }
                Err(error)
            }
        }
    }

    fn mark_outstanding(&mut self, slot_id: u8, now: Instant) {
        let index = usize::from(slot_id);
        let busy = self.session.transport().is_busy(slot_id);
        let queued = self.session.transport().queue_len(slot_id) != 0;

        match self.slots[index].status {
            CaSlotStatus::CreatingTc if busy && self.slots[index].create_since.is_none() => {
                self.slots[index].create_since = Some(now);
                self.slots[index].command_since = None;
            }
            CaSlotStatus::CreatingTc if queued => {
                self.slots[index].command_since.get_or_insert(now);
            }
            CaSlotStatus::Active if busy || queued => {
                self.slots[index].command_since.get_or_insert(now);
            }
            _ => {}
        }
    }

    /// Coalesces DATA_INDICATOR across acknowledgements. Resource replies
    /// already queued by session dispatch must leave the link first; once
    /// the slot is completely idle, exactly one RCV is issued.
    fn flush_receive_pending(&mut self, slot_id: u8, now: Instant) -> Result<()> {
        let index = usize::from(slot_id);
        if self.slots[index].status != CaSlotStatus::Active
            || !self.slots[index].receive_pending
            || self.session.transport().is_busy(slot_id)
            || self.session.transport().queue_len(slot_id) != 0
        {
            return Ok(());
        }

        self.session
            .transport_mut()
            .send_tpdu(slot_id, TpduTag::RCV, &[])?;
        self.slots[index].receive_pending = false;
        self.mark_outstanding(slot_id, now);
        Ok(())
    }

    fn event_time(&self) -> Option<Instant> {
        // Active/CreatingTc can only be reached from tick(), so protocol
        // writes use exactly the caller's monotonic clock. Before the first
        // tick there is no controller deadline to timestamp.
        self.last_tick
    }

    fn refresh_slot_info(&mut self, now: Instant) -> Result<()> {
        let mut infos = Vec::with_capacity(self.slots.len());
        for slot_id in 0 .. self.slots_num() {
            match self.io.slot_info(self.session.transport().link(), slot_id) {
                Ok(info) => infos.push(info),
                Err(error) => {
                    let _ = self.recover_all(Some(slot_id), CaSlotFailure::SlotInfoFailed, now);
                    return Err(error);
                }
            }
        }

        for (slot_id, info) in infos.into_iter().enumerate() {
            self.apply_slot_info(slot_id as u8, info);
        }
        Ok(())
    }

    fn apply_slot_info(&mut self, slot_id: u8, info: CaSlotInfo) {
        let present = (info.flags & (CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY)) != 0;
        let ready = (info.flags & CA_CI_MODULE_READY) != 0;
        let index = usize::from(slot_id);
        self.slots[index].present = present;
        self.slots[index].ready = ready;

        if !present {
            if self.slots[index].status != CaSlotStatus::Absent {
                self.session.drop_slot(slot_id);
                self.collect_session_events();
                self.clear_slot_timers(slot_id);
                self.set_cam_status(slot_id, CamStatus::None);
                self.set_slot_status(slot_id, CaSlotStatus::Absent);
            }
            return;
        }

        if (info.slot_type & CA_CI_LINK) == 0 {
            if self.slots[index].status != CaSlotStatus::Failed
                || self.slots[index].retry_at.is_some()
            {
                self.session.drop_slot(slot_id);
                self.collect_session_events();
                self.clear_slot_timers(slot_id);
                self.events.push_back(CaEvent::SlotFailed {
                    slot_id,
                    reason: CaSlotFailure::UnsupportedInterface {
                        slot_type: info.slot_type,
                    },
                });
                self.set_cam_status(slot_id, CamStatus::None);
                self.set_slot_status(slot_id, CaSlotStatus::Failed);
            }
            return;
        }

        match self.slots[index].status {
            CaSlotStatus::Absent => self.set_slot_status(slot_id, CaSlotStatus::Present),
            CaSlotStatus::CreatingTc | CaSlotStatus::Active if !ready => {
                self.session.drop_slot(slot_id);
                self.collect_session_events();
                self.clear_slot_timers(slot_id);
                self.set_cam_status(slot_id, CamStatus::None);
                self.set_slot_status(slot_id, CaSlotStatus::Present);
            }
            _ => {}
        }
    }

    fn drive_slot(&mut self, slot_id: u8, now: Instant) -> Result<Option<CaSlotFailure>> {
        let index = usize::from(slot_id);

        if self.slots[index].status == CaSlotStatus::Failed {
            if self.slots[index]
                .retry_at
                .is_some_and(|retry_at| now >= retry_at)
            {
                self.slots[index].retry_at = None;
                self.set_slot_status(slot_id, CaSlotStatus::Present);
            } else {
                return Ok(None);
            }
        }

        match self.slots[index].status {
            CaSlotStatus::Absent | CaSlotStatus::Failed => Ok(None),
            CaSlotStatus::Present => {
                if !self.slots[index].ready {
                    return Ok(None);
                }

                self.session
                    .transport_mut()
                    .send_tpdu(slot_id, TpduTag::CREATE_TC, &[])?;
                self.slots[index].receive_pending = false;
                self.set_slot_status(slot_id, CaSlotStatus::CreatingTc);
                self.mark_outstanding(slot_id, now);
                Ok(None)
            }
            CaSlotStatus::CreatingTc => {
                self.session.transport_mut().flush(slot_id)?;
                self.mark_outstanding(slot_id, now);

                if self.slots[index].create_since.is_none()
                    && self.slots[index].command_since.is_some_and(|since| {
                        now.saturating_duration_since(since) >= self.config.response_timeout
                    })
                {
                    return Ok(Some(CaSlotFailure::WriteTimeout));
                }

                let timed_out = self.slots[index].create_since.is_some_and(|since| {
                    now.saturating_duration_since(since) >= self.config.create_tc_timeout
                });
                Ok(timed_out.then_some(CaSlotFailure::CreateTcTimeout))
            }
            CaSlotStatus::Active => {
                self.session.transport_mut().flush(slot_id)?;

                let busy = self.session.transport().is_busy(slot_id);
                let queued = self.session.transport().queue_len(slot_id) != 0;
                let pending = busy || queued;
                if pending {
                    let since = *self.slots[index].command_since.get_or_insert(now);
                    if now.saturating_duration_since(since) >= self.config.response_timeout {
                        return Ok(Some(if busy {
                            CaSlotFailure::ResponseTimeout
                        } else {
                            CaSlotFailure::WriteTimeout
                        }));
                    }
                    return Ok(None);
                }

                self.slots[index].command_since = None;

                if self.slots[index].receive_pending {
                    self.flush_receive_pending(slot_id, now)?;
                    return Ok(None);
                }

                if self.slots[index]
                    .next_poll
                    .is_none_or(|next_poll| now >= next_poll)
                {
                    self.session
                        .transport_mut()
                        .send_tpdu(slot_id, TpduTag::DATA_LAST, &[])?;
                    self.slots[index].command_since = Some(now);
                    self.slots[index].next_poll =
                        Some(deadline(now, self.config.transport_poll_interval));
                }

                Ok(None)
            }
        }
    }

    fn collect_session_events(&mut self) {
        while let Some(event) = self.session.next_event() {
            match event {
                CaEvent::TransportReady { slot_id } => {
                    if self
                        .slots
                        .get(usize::from(slot_id))
                        .is_some_and(|slot| slot.status == CaSlotStatus::CreatingTc)
                    {
                        let slot = &mut self.slots[usize::from(slot_id)];
                        slot.create_since = None;
                        slot.command_since = None;
                        slot.next_poll = None;
                        self.set_slot_status(slot_id, CaSlotStatus::Active);
                        self.events.push_back(CaEvent::TransportReady { slot_id });
                    } else {
                        self.events.push_back(CaEvent::Malformed {
                            slot_id,
                            context: format!(
                                "ca slot {}: unexpected transport connection reply",
                                slot_id
                            ),
                        });
                    }
                }
                CaEvent::ApplicationInfo { slot_id, info } => {
                    if self
                        .slots
                        .get(usize::from(slot_id))
                        .is_some_and(|slot| slot.status == CaSlotStatus::Active)
                    {
                        self.set_cam_status(slot_id, CamStatus::ApplicationInfo);
                        self.events
                            .push_back(CaEvent::ApplicationInfo { slot_id, info });
                    } else {
                        self.events.push_back(CaEvent::Malformed {
                            slot_id,
                            context: format!(
                                "ca slot {}: application information on an inactive slot",
                                slot_id
                            ),
                        });
                    }
                }
                CaEvent::SessionClosed {
                    slot_id,
                    session_id,
                    resource_id,
                } => {
                    if resource_id.base() == ResourceId::APPLICATION_INFORMATION.base()
                        && self.session.app_info(slot_id).is_none()
                    {
                        self.set_cam_status(slot_id, CamStatus::None);
                    }
                    self.events.push_back(CaEvent::SessionClosed {
                        slot_id,
                        session_id,
                        resource_id,
                    });
                }
                event => self.events.push_back(event),
            }
        }
    }

    fn recover_all(
        &mut self,
        failed_slot: Option<u8>,
        reason: CaSlotFailure,
        now: Instant,
    ) -> Result<()> {
        if let Some(slot_id) = failed_slot {
            self.events
                .push_back(CaEvent::SlotFailed { slot_id, reason });
        } else {
            for slot_id in 0 .. self.slots_num() {
                if self.slots[usize::from(slot_id)].status != CaSlotStatus::Absent {
                    self.events
                        .push_back(CaEvent::SlotFailed { slot_id, reason });
                }
            }
        }

        self.drop_all_slots();
        let reset_result = self.io.reset(self.session.transport().link());
        let retry_at = deadline(now, self.config.retry_interval);
        self.link_suspended = true;
        self.recovery_at = reset_result.as_ref().ok().map(|_| retry_at);
        self.deferred_link_failure = false;

        for slot_id in 0 .. self.slots_num() {
            let index = usize::from(slot_id);
            self.slots[index].ready = false;
            self.slots[index].retry_at = reset_result.as_ref().ok().map(|_| retry_at);
            self.set_cam_status(slot_id, CamStatus::None);
            self.set_slot_status(
                slot_id,
                if self.slots[index].present {
                    CaSlotStatus::Failed
                } else {
                    CaSlotStatus::Absent
                },
            );
        }
        self.next_status_check = self.recovery_at;

        if reset_result.is_err() {
            self.mark_reset_failed();
        }
        reset_result
    }

    fn mark_reset_failed(&mut self) {
        for slot_id in 0 .. self.slots_num() {
            let index = usize::from(slot_id);
            if self.slots[index].present {
                self.slots[index].retry_at = None;
                self.events.push_back(CaEvent::SlotFailed {
                    slot_id,
                    reason: CaSlotFailure::ResetFailed,
                });
                self.set_slot_status(slot_id, CaSlotStatus::Failed);
            }
        }
    }

    fn drop_all_slots(&mut self) {
        for slot_id in 0 .. self.slots_num() {
            self.session.drop_slot(slot_id);
            self.clear_slot_timers(slot_id);
        }
        self.collect_session_events();
    }

    fn clear_slot_timers(&mut self, slot_id: u8) {
        let slot = &mut self.slots[usize::from(slot_id)];
        slot.create_since = None;
        slot.command_since = None;
        slot.receive_pending = false;
        slot.next_poll = None;
        slot.retry_at = None;
    }

    fn set_slot_status(&mut self, slot_id: u8, new: CaSlotStatus) {
        let slot = &mut self.slots[usize::from(slot_id)];
        let old = std::mem::replace(&mut slot.status, new);
        if old != new {
            self.events
                .push_back(CaEvent::SlotStatusChanged { slot_id, old, new });
        }
    }

    fn set_cam_status(&mut self, slot_id: u8, new: CamStatus) {
        let slot = &mut self.slots[usize::from(slot_id)];
        let old = std::mem::replace(&mut slot.cam_status, new);
        if old != new {
            self.events
                .push_back(CaEvent::CamStatusChanged { slot_id, old, new });
        }
    }
}

fn is_link_error(error: &Error) -> bool {
    match error {
        Error::Io(_) | Error::Nix(_) => true,
        Error::InvalidData(context) => context == "ca link frame short write",
        Error::InvalidProperty(_) => false,
    }
}

fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
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
        sync::{
            Arc,
            Mutex,
        },
    };

    use super::{
        super::{
            ApduTag,
            apdu,
            asn1,
            spdu,
            tpdu,
        },
        *,
    };

    struct MockState {
        infos: Vec<CaSlotInfo>,
        resets: usize,
        fail_slot_info: bool,
        fail_reset: bool,
    }

    #[derive(Clone)]
    struct MockIo(Arc<Mutex<MockState>>);

    impl ControllerIo for MockIo {
        fn reset(&self, _device: &CaDevice) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            state.resets += 1;
            if state.fail_reset {
                Err(std::io::Error::from(ErrorKind::Other).into())
            } else {
                Ok(())
            }
        }

        fn slot_info(&self, _device: &CaDevice, slot_id: u8) -> Result<CaSlotInfo> {
            let state = self.0.lock().unwrap();
            if state.fail_slot_info {
                return Err(std::io::Error::from(ErrorKind::Other).into());
            }
            Ok(state.infos[usize::from(slot_id)])
        }
    }

    struct TestCam {
        file: File,
    }

    impl TestCam {
        fn recv(&mut self) -> Option<Vec<u8>> {
            let mut frame = [0; 4096];
            match self.file.read(&mut frame) {
                Ok(len) => Some(frame[.. len].to_vec()),
                Err(error) if error.kind() == ErrorKind::WouldBlock => None,
                Err(error) => panic!("cam read: {error}"),
            }
        }

        fn send_ctc_reply(&mut self, slot_id: u8, data_pending: bool) {
            let t_c_id = slot_id + 1;
            self.file
                .write_all(&[
                    slot_id,
                    t_c_id,
                    TpduTag::CTC_REPLY.raw(),
                    1,
                    t_c_id,
                    TpduTag::SB.raw(),
                    2,
                    t_c_id,
                    if data_pending { 0x80 } else { 0 },
                ])
                .unwrap();
        }

        fn send_status(&mut self, slot_id: u8, data_pending: bool) {
            let t_c_id = slot_id + 1;
            self.file
                .write_all(&[
                    slot_id,
                    t_c_id,
                    TpduTag::SB.raw(),
                    2,
                    t_c_id,
                    if data_pending { 0x80 } else { 0 },
                ])
                .unwrap();
        }

        fn send_spdu(&mut self, slot_id: u8, spdu: &[u8]) {
            self.send_spdu_with_pending(slot_id, spdu, false);
        }

        fn send_spdu_with_pending(&mut self, slot_id: u8, spdu: &[u8], data_pending: bool) {
            let t_c_id = slot_id + 1;
            let mut frame = vec![slot_id, t_c_id, TpduTag::DATA_LAST.raw()];
            asn1::encode(spdu.len() as u16 + 1, &mut frame);
            frame.push(t_c_id);
            frame.extend_from_slice(spdu);
            frame.extend_from_slice(&[
                TpduTag::SB.raw(),
                2,
                t_c_id,
                if data_pending { 0x80 } else { 0 },
            ]);
            self.file.write_all(&frame).unwrap();
        }

        fn send_apdu(&mut self, slot_id: u8, session_id: u16, tag: ApduTag, body: &[u8]) {
            let mut payload = spdu::build_session_number(session_id);
            apdu::build(&mut payload, tag, body);
            self.send_spdu(slot_id, &payload);
        }
    }

    fn config() -> CiControllerConfig {
        CiControllerConfig {
            slot_status_interval: Duration::ZERO,
            transport_poll_interval: Duration::from_millis(10),
            create_tc_timeout: Duration::from_millis(100),
            response_timeout: Duration::from_millis(100),
            retry_interval: Duration::from_millis(50),
        }
    }

    fn pair(slots_num: u8) -> (CiController, TestCam, Arc<Mutex<MockState>>) {
        let (host, cam) = UnixDatagram::pair().unwrap();
        host.set_nonblocking(true).unwrap();
        cam.set_nonblocking(true).unwrap();

        let host = File::from(OwnedFd::from(host));
        let cam = File::from(OwnedFd::from(cam));
        let session = CiSession::new(CiTransport::new(CaDevice::from_file(host), slots_num));
        let state = Arc::new(Mutex::new(MockState {
            infos: (0 .. slots_num)
                .map(|slot_id| CaSlotInfo {
                    slot_num: u32::from(slot_id),
                    slot_type: CA_CI_LINK,
                    flags: 0,
                })
                .collect(),
            resets: 0,
            fail_slot_info: false,
            fail_reset: false,
        }));
        let controller =
            CiController::from_parts(session, config(), Box::new(MockIo(Arc::clone(&state))));

        (controller, TestCam { file: cam }, state)
    }

    fn set_flags(state: &Arc<Mutex<MockState>>, slot_id: u8, flags: u32) {
        state.lock().unwrap().infos[usize::from(slot_id)].flags = flags;
    }

    fn drain(controller: &mut CiController) -> Vec<CaEvent> {
        let mut events = Vec::new();
        while let Some(event) = controller.poll_event().unwrap() {
            events.push(event);
        }
        events
    }

    fn activate(
        controller: &mut CiController,
        cam: &mut TestCam,
        state: &Arc<Mutex<MockState>>,
        slot_id: u8,
        now: Instant,
    ) {
        set_flags(state, slot_id, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        controller.tick(now).unwrap();
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(slot_id, TpduTag::CREATE_TC, &[]).unwrap()
        );
        let _ = drain(controller);

        cam.send_ctc_reply(slot_id, false);
        let events = drain(controller);
        assert!(events.iter().any(|event| matches!(
            event,
            CaEvent::SlotStatusChanged {
                slot_id: event_slot,
                new: CaSlotStatus::Active,
                ..
            } if *event_slot == slot_id
        )));
        assert_eq!(controller.status(slot_id).unwrap(), CaSlotStatus::Active);
    }

    #[test]
    fn test_poll_event_is_nonblocking_when_empty() {
        let (mut controller, _cam, _state) = pair(1);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Absent);
    }

    #[test]
    fn test_ready_starts_create_and_ctc_reply_activates() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();

        set_flags(&state, 0, CA_CI_MODULE_PRESENT);
        controller.tick(now).unwrap();
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Present);
        assert!(cam.recv().is_none());
        assert_eq!(
            drain(&mut controller),
            vec![CaEvent::SlotStatusChanged {
                slot_id: 0,
                old: CaSlotStatus::Absent,
                new: CaSlotStatus::Present,
            }]
        );

        set_flags(&state, 0, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        controller.tick(now).unwrap();
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::CreatingTc);
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        assert!(cam.recv().is_none());
        let _ = drain(&mut controller);

        cam.send_ctc_reply(0, false);
        assert_eq!(
            controller.poll_event().unwrap(),
            Some(CaEvent::SlotStatusChanged {
                slot_id: 0,
                old: CaSlotStatus::CreatingTc,
                new: CaSlotStatus::Active,
            })
        );
        assert_eq!(
            controller.poll_event().unwrap(),
            Some(CaEvent::TransportReady { slot_id: 0 })
        );
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Active);
    }

    #[test]
    fn test_unexpected_ctc_reply_does_not_activate_absent_slot() {
        let (mut controller, mut cam, _state) = pair(1);
        cam.send_ctc_reply(0, false);

        assert!(matches!(
            controller.poll_event().unwrap(),
            Some(CaEvent::Malformed { slot_id: 0, .. })
        ));
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Absent);
    }

    #[test]
    fn test_data_indicator_from_absent_generation_is_not_reused() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();

        cam.send_status(0, true);
        assert_eq!(controller.poll_event().unwrap(), None);

        activate(&mut controller, &mut cam, &state, 0, now);
        assert!(
            cam.recv().is_none(),
            "stale DATA_INDICATOR must not emit RCV after a new CTC"
        );
    }

    #[test]
    fn test_status_instead_of_ctc_reply_does_not_cancel_create_timeout() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        set_flags(&state, 0, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        controller.tick(now).unwrap();
        assert!(cam.recv().is_some());
        let _ = drain(&mut controller);

        // A valid response clears transport busy, but only CTC_REPLY may
        // complete the controller's CREATE_TC deadline.
        cam.send_status(0, false);
        assert_eq!(controller.poll_event().unwrap(), None);
        controller
            .tick(now + config().create_tc_timeout - Duration::from_nanos(1))
            .unwrap();
        assert_eq!(state.lock().unwrap().resets, 0);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::CreatingTc);

        controller.tick(now + config().create_tc_timeout).unwrap();
        assert_eq!(state.lock().unwrap().resets, 1);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Failed);
        assert!(drain(&mut controller).iter().any(|event| matches!(
            event,
            CaEvent::SlotFailed {
                slot_id: 0,
                reason: CaSlotFailure::CreateTcTimeout,
            }
        )));
    }

    #[test]
    fn test_active_slot_polls_and_data_indicator_sends_rcv() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        controller.tick(now).unwrap();
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::DATA_LAST, &[]).unwrap()
        );
        controller.tick(now + Duration::from_millis(99)).unwrap();
        assert!(cam.recv().is_none(), "a busy slot must not be polled twice");

        cam.send_status(0, true);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::RCV, &[]).unwrap()
        );
        assert!(cam.recv().is_none());
    }

    #[test]
    fn test_rcv_uses_last_tick_for_response_timeout() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        controller.tick(now).unwrap();
        assert!(cam.recv().is_some());
        cam.send_status(0, true);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::RCV, &[]).unwrap()
        );

        controller.tick(now + config().response_timeout).unwrap();
        assert_eq!(state.lock().unwrap().resets, 1);
        assert!(drain(&mut controller).iter().any(|event| matches!(
            event,
            CaEvent::SlotFailed {
                slot_id: 0,
                reason: CaSlotFailure::ResponseTimeout,
            }
        )));
    }

    #[test]
    fn test_repeated_data_indicator_coalesces_one_rcv_after_resource_queue() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        controller.tick(now).unwrap();
        assert!(cam.recv().is_some(), "initial poll");
        cam.send_spdu_with_pending(0, &[0x91, 0x04, 0x00, 0x02, 0x00, 0x41], true);
        let _ = drain(&mut controller);

        // Session dispatch emitted open_session_response and queued
        // application_info_enq. RCV must not jump ahead of either.
        assert!(cam.recv().is_some(), "open_session_response");
        assert!(cam.recv().is_none());

        cam.send_status(0, true);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert!(cam.recv().is_some(), "application_info_enq");
        assert!(cam.recv().is_none());

        // DI stayed set on every acknowledgement, but only one coalesced
        // RCV is emitted after the resource queue becomes idle.
        cam.send_status(0, true);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::RCV, &[]).unwrap()
        );
        assert!(cam.recv().is_none());
    }

    #[test]
    fn test_spdu_is_rejected_before_transport_is_active() {
        let (mut controller, mut cam, _state) = pair(1);
        cam.send_spdu(0, &[0x91, 0x04, 0x00, 0x01, 0x00, 0x41]);

        assert!(matches!(
            controller.poll_event().unwrap(),
            Some(CaEvent::Malformed { slot_id: 0, .. })
        ));
        assert!(
            cam.recv().is_none(),
            "an inactive slot must not open a session"
        );
    }

    #[test]
    fn test_response_timeout_resets_every_slot_once_and_retries() {
        let (mut controller, mut cam, state) = pair(2);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        // Keep the second slot logically active too. Recovery must clear it
        // because Linux CA_RESET is global.
        set_flags(&state, 1, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        controller.slots[1].present = true;
        controller.slots[1].ready = true;
        controller.set_slot_status(1, CaSlotStatus::Active);
        controller.set_cam_status(1, CamStatus::ApplicationInfo);
        let _ = drain(&mut controller);

        controller.tick(now).unwrap();
        let mut polls = vec![cam.recv().unwrap(), cam.recv().unwrap()];
        polls.sort();
        let mut expected = vec![
            tpdu::build(0, TpduTag::DATA_LAST, &[]).unwrap(),
            tpdu::build(1, TpduTag::DATA_LAST, &[]).unwrap(),
        ];
        expected.sort();
        assert_eq!(polls, expected);

        controller
            .tick(now + config().response_timeout - Duration::from_nanos(1))
            .unwrap();
        assert_eq!(state.lock().unwrap().resets, 0);

        controller.tick(now + config().response_timeout).unwrap();
        assert_eq!(state.lock().unwrap().resets, 1);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Failed);
        assert_eq!(controller.status(1).unwrap(), CaSlotStatus::Failed);
        assert_eq!(controller.cam_status(1).unwrap(), CamStatus::None);
        assert!(!controller.session.transport().is_busy(0));
        assert!(!controller.session.transport().is_busy(1));
        assert_eq!(controller.session.transport().queue_len(0), 0);
        assert_eq!(controller.session.transport().queue_len(1), 0);
        assert!(drain(&mut controller).iter().any(|event| matches!(
            event,
            CaEvent::SlotFailed {
                slot_id: 0,
                reason: CaSlotFailure::ResponseTimeout,
            }
        )));

        controller
            .tick(
                now + config().response_timeout + config().retry_interval - Duration::from_nanos(1),
            )
            .unwrap();
        assert_eq!(
            state.lock().unwrap().resets,
            1,
            "no reset storm before retry"
        );

        controller
            .tick(now + config().response_timeout + config().retry_interval)
            .unwrap();
        assert_eq!(state.lock().unwrap().resets, 1);
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::CreatingTc);
        assert_eq!(controller.status(1).unwrap(), CaSlotStatus::CreatingTc);
    }

    #[test]
    fn test_slot_info_error_is_rate_limited_by_retry_interval() {
        let (mut controller, _cam, state) = pair(1);
        let now = Instant::now();
        state.lock().unwrap().fail_slot_info = true;

        assert!(controller.tick(now).is_err());
        assert_eq!(state.lock().unwrap().resets, 1);
        let _ = drain(&mut controller);

        controller
            .tick(now + config().retry_interval - Duration::from_nanos(1))
            .unwrap();
        assert_eq!(state.lock().unwrap().resets, 1);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert_eq!(state.lock().unwrap().resets, 1);

        assert!(controller.tick(now + config().retry_interval).is_err());
        assert_eq!(state.lock().unwrap().resets, 2);
    }

    #[test]
    fn test_removed_date_time_session_cannot_send_from_same_tick() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        controller.tick(now).unwrap();
        assert!(cam.recv().is_some(), "initial transport poll");
        cam.send_spdu(0, &[0x91, 0x04, 0x00, 0x24, 0x00, 0x41]);
        let _ = drain(&mut controller);

        assert!(cam.recv().is_some(), "open_session_response");
        cam.send_status(0, false);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert!(cam.recv().is_some(), "initial date_time object");

        cam.send_apdu(0, 1, ApduTag::DATE_TIME_ENQ, &[1]);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert!(cam.recv().is_some(), "date_time enquiry response");
        cam.send_status(0, false);
        assert_eq!(controller.poll_event().unwrap(), None);

        // Anchor the one-second resource interval to the injected clock.
        controller.tick(now).unwrap();
        assert!(cam.recv().is_none());

        // The status refresh must drop the session before resource timers
        // run, otherwise a stale periodic date_time would be sent here.
        set_flags(&state, 0, 0);
        controller.tick(now + Duration::from_secs(2)).unwrap();
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Absent);
        assert!(cam.recv().is_none());
    }

    #[test]
    fn test_application_info_and_removal_clear_cam_state_and_session() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        // Poll gives the module a command to answer with open_session.
        controller.tick(now).unwrap();
        assert_eq!(
            cam.recv().unwrap(),
            tpdu::build(0, TpduTag::DATA_LAST, &[]).unwrap()
        );
        cam.send_spdu(0, &[0x91, 0x04, 0x00, 0x02, 0x00, 0x41]);
        assert!(drain(&mut controller).iter().any(|event| matches!(
            event,
            CaEvent::SessionOpened {
                slot_id: 0,
                resource_id: ResourceId::APPLICATION_INFORMATION,
                ..
            }
        )));

        // open_session_response, then its acknowledgement releases the
        // queued application_info_enq.
        assert!(cam.recv().is_some());
        cam.send_status(0, false);
        assert_eq!(controller.poll_event().unwrap(), None);
        assert!(cam.recv().is_some());

        cam.send_apdu(
            0,
            1,
            ApduTag::APPLICATION_INFO,
            &[0x01, 0x12, 0x34, 0x56, 0x78, 0x03, b'C', b'A', b'M'],
        );
        let events = drain(&mut controller);
        assert!(events.iter().any(|event| matches!(
            event,
            CaEvent::CamStatusChanged {
                slot_id: 0,
                new: CamStatus::ApplicationInfo,
                ..
            }
        )));
        assert_eq!(
            controller.cam_status(0).unwrap(),
            CamStatus::ApplicationInfo
        );
        assert_eq!(controller.app_info(0).unwrap().menu_string, b"CAM");

        set_flags(&state, 0, 0);
        controller.tick(now).unwrap();
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Absent);
        assert_eq!(controller.cam_status(0).unwrap(), CamStatus::None);
        assert!(controller.app_info(0).is_none());
        let events = drain(&mut controller);
        assert!(events.iter().any(|event| matches!(
            event,
            CaEvent::SessionClosed {
                slot_id: 0,
                resource_id: ResourceId::APPLICATION_INFORMATION,
                ..
            }
        )));
    }

    #[test]
    fn test_ready_loss_returns_active_slot_to_present() {
        let (mut controller, mut cam, state) = pair(1);
        let now = Instant::now();
        activate(&mut controller, &mut cam, &state, 0, now);

        set_flags(&state, 0, CA_CI_MODULE_PRESENT);
        controller.tick(now).unwrap();
        assert_eq!(controller.status(0).unwrap(), CaSlotStatus::Present);
        assert_eq!(state.lock().unwrap().resets, 0);
        assert!(!controller.session.transport().is_busy(0));
    }
}
