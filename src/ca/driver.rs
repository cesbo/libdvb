//! Async driver for [`CiController`] on tokio.
//!
//! [`CiDriver::run`] owns the device event loop: it waits for CA link
//! frames, schedules `tick` deadlines, waits out a suspended link without
//! polling the descriptor, and retries a failed global CA_RESET until it
//! succeeds. The caller spawns the returned future on its own runtime -
//! the library never spawns tasks or owns a runtime. Commands arrive
//! through a cloneable, thread-safe [`CiDriverHandle`]; CAM activity is
//! delivered as [`CiDriverEvent`] values over an unbounded channel; the
//! CA_PMT readiness gate is published through a `watch` channel.

use std::{
    ops::ControlFlow,
    os::unix::io::AsRawFd,
    time::Duration,
};

use tokio::{
    io::{
        Interest,
        unix::AsyncFd,
    },
    sync::{
        mpsc,
        watch,
    },
    time::{
        self,
        Instant,
    },
};

use super::{
    CaEvent,
    CiController,
    CiControllerConfig,
    capmt::Program,
};
use crate::error::{
    Error,
    Result,
};

/// One notification from the async CI driver
#[derive(Debug)]
#[non_exhaustive]
pub enum CiDriverEvent {
    /// EN 50221 activity from the controller
    Ca(CaEvent),
    /// A processing pass (`tick` or link-frame handling) failed; when the
    /// failure is link-level, global recovery has already been started
    /// inside the controller. The loop keeps running.
    Fault(Error),
    /// An internal CA_RESET retry failed; the next attempt runs one
    /// `retry_interval` later
    ResetRetryFailed(Error),
    /// An internal CA_RESET retry succeeded; slot supervision resumed
    ResetRetrySucceeded,
    /// A handle command failed, for example an MMI answer for a session
    /// that has just closed. `command` names the handle method.
    CommandFailed { command: &'static str, error: Error },
}

/// One queued handle command
#[derive(Debug)]
enum Command {
    SetProgram(Program),
    RemoveProgram(u16),
    SetCaPmtInterval(Duration),
    EnterMenu {
        slot_id: u8,
    },
    MmiMenuAnswer {
        slot_id: u8,
        session_id: u16,
        choice: u8,
    },
    MmiAnswer {
        slot_id: u8,
        session_id: u16,
        answer: Option<Vec<u8>>,
    },
    MmiClose {
        slot_id: u8,
        session_id: u16,
    },
    MmiListClose {
        slot_id: u8,
        session_id: u16,
    },
    AskRelease {
        slot_id: u8,
    },
    Reset,
    Shutdown,
}

/// Thread-safe command handle for a running [`CiDriver`]. All methods are
/// synchronous and non-blocking; they may be called from non-async
/// threads. Commands sent after the driver stopped are silently dropped.
#[derive(Clone, Debug)]
pub struct CiDriverHandle {
    commands: mpsc::UnboundedSender<Command>,
    ready: watch::Receiver<bool>,
}

impl CiDriverHandle {
    /// Parses and validates one complete raw PMT section (including
    /// CRC32) immediately and queues the paced select; returns the parsed
    /// `program_number`. `Err` = parse/validation failure only.
    pub fn set_program(&self, pmt_section: &[u8]) -> Result<u16> {
        let program = Program::parse(pmt_section)?;
        let program_number = program.program_number();
        self.send(Command::SetProgram(program));
        Ok(program_number)
    }

    /// Queues the paced program remove; `Err` only for program_number 0
    pub fn remove_program(&self, program_number: u16) -> Result<()> {
        if program_number == 0 {
            return Err(Error::InvalidProperty(
                "CA program number must not be zero".to_owned(),
            ));
        }
        self.send(Command::RemoveProgram(program_number));
        Ok(())
    }

    /// Changes the CA_PMT pacing interval on the live controller;
    /// effective from the next driver tick
    pub fn set_ca_pmt_interval(&self, interval: Duration) {
        self.send(Command::SetCaPmtInterval(interval));
    }

    /// Asks the CAM to enter its menu. Failures (for example the slot is
    /// not active) are reported as [`CiDriverEvent::CommandFailed`].
    pub fn enter_menu(&self, slot_id: u8) {
        self.send(Command::EnterMenu { slot_id });
    }

    /// Answers a menu selection with a 1-based item number; 0 cancels
    pub fn mmi_menu_answer(&self, slot_id: u8, session_id: u16, choice: u8) {
        self.send(Command::MmiMenuAnswer {
            slot_id,
            session_id,
            choice,
        });
    }

    /// Answers an enquiry; `None` cancels it. The answer is copied.
    pub fn mmi_answer(&self, slot_id: u8, session_id: u16, answer: Option<&[u8]>) {
        self.send(Command::MmiAnswer {
            slot_id,
            session_id,
            answer: answer.map(<[u8]>::to_vec),
        });
    }

    /// Asks the CAM to close the MMI dialogue of the session
    pub fn mmi_close(&self, slot_id: u8, session_id: u16) {
        self.send(Command::MmiClose {
            slot_id,
            session_id,
        });
    }

    /// Finishes viewing a list
    pub fn mmi_list_close(&self, slot_id: u8, session_id: u16) {
        self.send(Command::MmiListClose {
            slot_id,
            session_id,
        });
    }

    /// Asks the CAM to release the host-control resource
    pub fn ask_release(&self, slot_id: u8) {
        self.send(Command::AskRelease { slot_id });
    }

    /// Requests a global CA_RESET. On failure the driver keeps retrying
    /// internally at `retry_interval`.
    pub fn reset(&self) {
        self.send(Command::Reset);
    }

    /// Stops the driver: the loop exits, the controller is dropped and
    /// the CA device closes. Other handle clones become no-ops.
    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
    }

    /// Current CA_PMT readiness (see [`CiController::ca_pmt_ready`])
    pub fn ca_pmt_ready(&self) -> bool {
        *self.ready.borrow()
    }

    /// A watch receiver of the readiness gate. The driver publishes every
    /// transition, including gate openings on quiet tick passes that
    /// produce no event. After the driver stops, `changed()` errors;
    /// [`CiDriver::run`] publishes a final `false` on every return, so
    /// only an aborted task can leave a stale `true` behind.
    pub fn ready_watch(&self) -> watch::Receiver<bool> {
        self.ready.clone()
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// Owns a [`CiController`] and drives it; consume with [`CiDriver::run`]
pub struct CiDriver {
    controller: CiController,
    commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<CiDriverEvent>,
    ready: watch::Sender<bool>,
    /// Next internal CA_RESET retry while the link is suspended with no
    /// scheduled recovery (the suspending CA_RESET failed)
    reset_retry_at: Option<Instant>,
}

impl CiDriver {
    /// Wraps an already-open controller. Returns the driver (spawn
    /// `driver.run()` on a tokio runtime), the command handle and the
    /// event stream. Device open, pre-open settling and open retries are
    /// the caller's concern.
    pub fn new(
        controller: CiController,
    ) -> (
        CiDriver,
        CiDriverHandle,
        mpsc::UnboundedReceiver<CiDriverEvent>,
    ) {
        let (commands, command_queue) = mpsc::unbounded_channel();
        let (events, event_stream) = mpsc::unbounded_channel();
        let (ready, ready_watch) = watch::channel(false);
        let driver = CiDriver {
            controller,
            commands: command_queue,
            events,
            ready,
            reset_retry_at: None,
        };
        let handle = CiDriverHandle {
            commands,
            ready: ready_watch,
        };
        (driver, handle, event_stream)
    }

    /// The event loop. Runs until every handle clone is dropped or
    /// [`CiDriverHandle::shutdown`] is called, then drops the controller
    /// and closes the CA device; returns `Ok(())` on orderly shutdown.
    /// `Err` is reserved for reactor-level descriptor failures - failed
    /// registration of the CA descriptor or an I/O error from readiness
    /// polling - and can surface after events were already produced.
    /// Every return publishes `false` on the readiness watch first.
    /// Cancel-safe: aborting the task deregisters the descriptor and
    /// closes the device.
    pub async fn run(mut self) -> Result<()> {
        let result = self.run_loop().await;
        // A CAM left with an MMI dialogue open can refuse the next one
        let _ = self.controller.close_all_mmi();
        // The device is about to close: publish not-ready on every exit
        let _ = self.ready.send(false);
        result
    }

    async fn run_loop(&mut self) -> Result<()> {
        // afd is a local (not a struct field): the readable() guard
        // borrows it while self.drain_events() needs &mut self. Local
        // drop order also guarantees the reactor deregistration happens
        // before the controller drops and closes the descriptor.
        let afd = AsyncFd::with_interest(self.controller.as_raw_fd(), Interest::READABLE)
            .map_err(Error::Io)?;
        let tick_period = tick_period(self.controller.config());
        let retry_interval = self.controller.config().retry_interval;

        // The first tick arms the controller's timestamp base: before it,
        // timeouts are not armed, RCV flushing is skipped and the CA_PMT
        // gate cannot arm. It also makes the deferred-link-failure path
        // unreachable for the rest of the loop.
        self.tick_now();
        let mut next_tick = Instant::now() + tick_period;

        loop {
            if self.controller.link_suspended() {
                // While suspended the descriptor is not part of the
                // select, so a still-readable device causes zero wakeups
                // until recovery.
                let deadline = match self.controller.recovery_at() {
                    Some(at) => Instant::from_std(at),
                    // The suspending CA_RESET failed and the controller
                    // schedules nothing - retry internally
                    None => *self
                        .reset_retry_at
                        .get_or_insert_with(|| Instant::now() + retry_interval),
                };
                tokio::select! {
                    biased;
                    command = self.commands.recv() => match command {
                        Some(command) => {
                            if self.apply(command).is_break() {
                                break;
                            }
                        }
                        None => break,
                    },
                    _ = time::sleep_until(deadline) => {
                        if self.controller.recovery_at().is_some() {
                            // Scheduled recovery: this tick lifts the
                            // suspension, restarts slot supervision and
                            // its drain consumes frames buffered while
                            // suspended (buffered data raises no new
                            // reactor edge)
                            self.tick_now();
                        } else {
                            self.retry_reset(retry_interval);
                        }
                        next_tick = Instant::now() + tick_period;
                    },
                }
                continue;
            }
            self.reset_retry_at = None;

            // Commands first (rare, cheap, must not be starved by a
            // babbling CAM), the tick deadline second (a readable storm
            // must not stall timeouts or pacing), readability last
            tokio::select! {
                biased;
                command = self.commands.recv() => match command {
                    Some(command) => {
                        if self.apply(command).is_break() {
                            break;
                        }
                    }
                    None => break,
                },
                _ = time::sleep_until(next_tick) => {
                    self.tick_now();
                    next_tick = Instant::now() + tick_period;
                },
                guard = afd.readable() => {
                    let mut guard = guard.map_err(Error::Io)?;
                    if self.drain_events() {
                        guard.clear_ready();
                    }
                },
            }
        }

        Ok(())
    }

    /// One tick pass: advances the controller clock, forwards the
    /// fallout. tick errors never terminate the loop: link-level failures
    /// have already triggered recovery inside the controller (including
    /// the failed-CA_RESET case, observed next pass as suspended with no
    /// recovery deadline); other errors leave the controller running.
    fn tick_now(&mut self) {
        if let Err(error) = self.controller.tick(Instant::now().into_std()) {
            self.emit(CiDriverEvent::Fault(error));
        }
        self.drain_events();
    }

    /// Forwards queued controller events, reading link frames as needed.
    /// Returns true when the pass ended with the link drained while not
    /// suspended - only then is it safe to clear descriptor readiness
    /// (an `Ok(None)` from a suspended controller is a guard, not proof
    /// of a drained socket buffer, and clearing on it would strand
    /// buffered frames that raise no new reactor edge).
    fn drain_events(&mut self) -> bool {
        for _ in 0 .. DRAIN_BATCH {
            match self.controller.poll_event() {
                Ok(Some(event)) => self.emit(CiDriverEvent::Ca(event)),
                Ok(None) => {
                    self.publish_ready();
                    return !self.controller.link_suspended();
                }
                Err(error) => {
                    // Recovery already ran inside poll_event: the first
                    // tick precedes every drain, so the failure is never
                    // deferred. Keep draining - queued SlotFailed events
                    // must still be forwarded, and the suspension guard
                    // ends the loop.
                    self.emit(CiDriverEvent::Fault(error));
                }
            }
        }
        // Batch limit hit: yield to the select without clearing readiness
        // so the tick deadline stays live against a babbling CAM
        self.publish_ready();
        false
    }

    /// One internal CA_RESET retry (the suspending reset failed)
    fn retry_reset(&mut self, retry_interval: Duration) {
        match self.controller.reset() {
            Ok(()) => {
                self.reset_retry_at = None;
                self.emit(CiDriverEvent::ResetRetrySucceeded);
                // reset() clears the suspension with no recovery wait;
                // restart slot supervision immediately
                self.tick_now();
            }
            Err(error) => {
                self.reset_retry_at = Some(Instant::now() + retry_interval);
                self.emit(CiDriverEvent::ResetRetryFailed(error));
                self.publish_ready();
            }
        }
    }

    fn apply(&mut self, command: Command) -> ControlFlow<()> {
        match command {
            Command::SetProgram(program) => self.controller.queue_program(program),
            // program_number != 0 verified by the handle
            Command::RemoveProgram(pnr) => drop(self.controller.remove_program(pnr)),
            Command::SetCaPmtInterval(interval) => self.controller.set_ca_pmt_interval(interval),
            Command::EnterMenu { slot_id } => {
                self.slot_command("enter_menu", |c| c.enter_menu(slot_id))
            }
            Command::MmiMenuAnswer {
                slot_id,
                session_id,
                choice,
            } => self.slot_command("mmi_menu_answer", |c| {
                c.mmi_menu_answer(slot_id, session_id, choice)
            }),
            Command::MmiAnswer {
                slot_id,
                session_id,
                answer,
            } => self.slot_command("mmi_answer", |c| {
                c.mmi_answer(slot_id, session_id, answer.as_deref())
            }),
            Command::MmiClose {
                slot_id,
                session_id,
            } => self.slot_command("mmi_close", |c| c.mmi_close(slot_id, session_id)),
            Command::MmiListClose {
                slot_id,
                session_id,
            } => self.slot_command("mmi_list_close", |c| c.mmi_list_close(slot_id, session_id)),
            Command::AskRelease { slot_id } => {
                self.slot_command("ask_release", |c| c.ask_release(slot_id))
            }
            Command::Reset => match self.controller.reset() {
                Ok(()) => self.tick_now(),
                // A failed reset suspends with no recovery deadline; the
                // next loop pass starts the internal retry cadence
                Err(error) => self.emit(CiDriverEvent::CommandFailed {
                    command: "reset",
                    error,
                }),
            },
            Command::Shutdown => return ControlFlow::Break(()),
        }
        // Slot commands can trigger recovery; forward its events and
        // publish the (possibly closed) gate
        self.drain_events();
        ControlFlow::Continue(())
    }

    fn slot_command(
        &mut self,
        command: &'static str,
        f: impl FnOnce(&mut CiController) -> Result<()>,
    ) {
        if let Err(error) = f(&mut self.controller) {
            self.emit(CiDriverEvent::CommandFailed { command, error });
        }
    }

    fn emit(&self, event: CiDriverEvent) {
        let _ = self.events.send(event);
    }

    fn publish_ready(&self) {
        let ready = self.controller.ca_pmt_ready();
        self.ready.send_if_modified(|current| {
            if *current != ready {
                *current = ready;
                true
            } else {
                false
            }
        });
    }
}

const TICK_PERIOD_MIN: Duration = Duration::from_millis(10);
const TICK_PERIOD_MAX: Duration = Duration::from_millis(100);
/// Maximum events forwarded per drain pass before yielding to the select
const DRAIN_BATCH: usize = 64;

/// Internal tick cadence: `transport_poll_interval` is the fastest
/// controller timer, so ticking faster gains nothing (internal deadlines
/// rate-limit all device work), while ticking slower would quantize
/// transport polls, timeouts and CA_PMT pacing. The clamp bounds
/// pathological configurations in both directions.
fn tick_period(config: &CiControllerConfig) -> Duration {
    config
        .transport_poll_interval
        .clamp(TICK_PERIOD_MIN, TICK_PERIOD_MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::{
            Arc,
            Mutex,
            atomic::{
                AtomicBool,
                Ordering,
            },
        },
    };

    use tokio::task::JoinHandle;

    use super::{
        super::{
            ApduTag,
            CaSlotFailure,
            CaSlotStatus,
            ResourceId,
            apdu,
            capmt::{
                CaPmtCommand,
                CaPmtListManagement,
            },
            controller::test_support::*,
            spdu,
            sys::{
                CA_CI_MODULE_PRESENT,
                CA_CI_MODULE_READY,
            },
            tpdu,
            tpdu::TpduTag,
        },
        *,
    };

    /// Granularity of the virtual-time polling helpers
    const STEP: Duration = Duration::from_millis(1);
    /// Virtual-time budget for every await in the tests: with the paused
    /// clock a stalled driver trips this instead of hanging the test
    const TEST_DEADLINE: Duration = Duration::from_secs(60);

    /// Converts a wedged driver into an attributable failure. A driver
    /// that busy-loops never yields, so the paused clock stands still and
    /// no virtual timeout can fire; only real time keeps moving. The
    /// watchdog aborts the process with a message unless dropped within
    /// the real-time budget.
    struct RealTimeWatchdog {
        disarmed: Arc<AtomicBool>,
    }

    impl RealTimeWatchdog {
        fn arm(message: &'static str) -> Self {
            let disarmed = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&disarmed);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(30));
                if !flag.load(Ordering::SeqCst) {
                    eprintln!("real-time watchdog: {message}");
                    std::process::abort();
                }
            });
            RealTimeWatchdog { disarmed }
        }
    }

    impl Drop for RealTimeWatchdog {
        fn drop(&mut self) {
            self.disarmed.store(true, Ordering::SeqCst);
        }
    }

    fn start(
        controller: CiController,
    ) -> (
        CiDriverHandle,
        mpsc::UnboundedReceiver<CiDriverEvent>,
        JoinHandle<Result<()>>,
    ) {
        let (driver, handle, events) = CiDriver::new(controller);
        (handle, events, tokio::spawn(driver.run()))
    }

    /// Waits for one frame on the CAM socket; the 1 ms virtual sleep is
    /// what lets the paused clock advance and the driver's timers fire
    async fn cam_recv(cam: &mut TestCam) -> Vec<u8> {
        time::timeout(TEST_DEADLINE, async {
            loop {
                if let Some(frame) = cam.recv() {
                    return frame;
                }
                time::sleep(STEP).await;
            }
        })
        .await
        .expect("cam frame within the test deadline")
    }

    /// Asserts no CAM frame and no driver event for `span` of virtual time
    async fn cam_quiet(
        cam: &mut TestCam,
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        span: Duration,
    ) {
        let deadline = Instant::now() + span;
        while Instant::now() < deadline {
            if let Some(frame) = cam.recv() {
                panic!("unexpected cam frame: {frame:X?}");
            }
            if let Ok(event) = events.try_recv() {
                panic!("unexpected event: {event:?}");
            }
            time::sleep(STEP).await;
        }
    }

    async fn next_event(events: &mut mpsc::UnboundedReceiver<CiDriverEvent>) -> CiDriverEvent {
        time::timeout(TEST_DEADLINE, events.recv())
            .await
            .expect("event within the test deadline")
            .expect("event stream open")
    }

    /// Consumes the stream until an event matches
    async fn wait_event(
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        pred: impl Fn(&CiDriverEvent) -> bool,
    ) -> CiDriverEvent {
        time::timeout(TEST_DEADLINE, async {
            loop {
                let event = events.recv().await.expect("event stream open");
                if pred(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("matching event within the test deadline")
    }

    /// Consumes the stream until a CA event matches
    async fn wait_ca_event(
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        pred: impl Fn(&CaEvent) -> bool,
    ) -> CaEvent {
        let event = wait_event(
            events,
            |event| matches!(event, CiDriverEvent::Ca(event) if pred(event)),
        )
        .await;
        match event {
            CiDriverEvent::Ca(event) => event,
            _ => unreachable!(),
        }
    }

    /// Flags the module ready and completes the transport handshake from
    /// the driver's internal ticks alone; consumes the event stream
    /// through `TransportReady`
    async fn activate_async(
        cam: &mut TestCam,
        state: &Arc<Mutex<MockState>>,
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        slot_id: u8,
    ) {
        set_flags(state, slot_id, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        assert_eq!(
            cam_recv(cam).await,
            tpdu::build(slot_id, TpduTag::CREATE_TC, &[]).unwrap()
        );
        cam.send_ctc_reply(slot_id, false);
        wait_ca_event(events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: event_slot,
                    new: CaSlotStatus::Active,
                    ..
                } if *event_slot == slot_id
            )
        })
        .await;
        wait_ca_event(events, |event| {
            matches!(
                event,
                CaEvent::TransportReady { slot_id: event_slot } if *event_slot == slot_id
            )
        })
        .await;
    }

    /// The first tick of an Active slot issues the initial transport
    /// poll; acknowledge it so the pacing tests keep the link idle
    async fn ack_initial_poll_async(cam: &mut TestCam) {
        assert_eq!(
            cam_recv(cam).await,
            tpdu::build(0, TpduTag::DATA_LAST, &[]).unwrap()
        );
        cam.send_status(0, false);
    }

    /// Mirror of `test_support::open_resource` driven through the event
    /// stream. When `enquiry` names the host enquiry APDU of the resource
    /// (AI and CA resources), the open-session response is acknowledged
    /// and the enquiry is consumed; the MMI host resource sends nothing
    /// on open, so `None` skips that exchange.
    async fn open_resource_async(
        cam: &mut TestCam,
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        slot_id: u8,
        resource_id: ResourceId,
        enquiry: Option<ApduTag>,
    ) -> u16 {
        let raw = resource_id.raw();
        cam.send_spdu(
            slot_id,
            &[
                0x91,
                0x04,
                (raw >> 24) as u8,
                (raw >> 16) as u8,
                (raw >> 8) as u8,
                raw as u8,
            ],
        );
        let event = wait_ca_event(events, |event| {
            matches!(
                event,
                CaEvent::SessionOpened {
                    slot_id: event_slot,
                    resource_id: event_resource,
                    ..
                } if *event_slot == slot_id && event_resource.base() == resource_id.base()
            )
        })
        .await;
        let CaEvent::SessionOpened { session_id, .. } = event else {
            unreachable!();
        };

        let response = spdu::build_open_session_response(spdu::SS_OK, resource_id, session_id);
        assert_eq!(
            cam_recv(cam).await,
            tpdu::build(slot_id, TpduTag::DATA_LAST, &response).unwrap()
        );

        if let Some(enquiry_tag) = enquiry {
            cam.send_status(slot_id, false);
            let mut enquiry = spdu::build_session_number(session_id);
            apdu::build(&mut enquiry, enquiry_tag, &[]);
            assert_eq!(
                cam_recv(cam).await,
                tpdu::build(slot_id, TpduTag::DATA_LAST, &enquiry).unwrap()
            );
        }

        session_id
    }

    /// Opens a Conditional Access Support session and confirms CA_INFO
    /// with one CAID, arming the CA_PMT readiness countdown
    async fn arm_ca_handshake_async(
        cam: &mut TestCam,
        events: &mut mpsc::UnboundedReceiver<CiDriverEvent>,
        caid: u16,
    ) -> u16 {
        let ca_session = open_resource_async(
            cam,
            events,
            0,
            ResourceId::CONDITIONAL_ACCESS_SUPPORT,
            Some(ApduTag::CA_INFO_ENQ),
        )
        .await;
        cam.send_apdu(0, ca_session, ApduTag::CA_INFO, &caid.to_be_bytes());
        wait_ca_event(events, |event| {
            matches!(
                event,
                CaEvent::CaInfo { session_id, .. } if *session_id == ca_session
            )
        })
        .await;
        ca_session
    }

    #[tokio::test(start_paused = true)]
    async fn driver_supervises_slot_lifecycle_from_internal_ticks() {
        let (controller, mut cam, state) = pair(1);
        let (_handle, mut events, _task) = start(controller);

        // no consumer ticking: CREATE_TC and the activation both come
        // from the driver's own deadlines and drains
        activate_async(&mut cam, &state, &mut events, 0).await;
    }

    #[tokio::test(start_paused = true)]
    async fn create_tc_timeout_fires_without_external_ticks() {
        let (controller, mut cam, state) = pair(1);
        let (_handle, mut events, _task) = start(controller);

        set_flags(&state, 0, CA_CI_MODULE_PRESENT | CA_CI_MODULE_READY);
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        // CREATE_TC is never answered: the timeout comes from internal
        // time alone
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotFailed {
                    slot_id: 0,
                    reason: CaSlotFailure::CreateTcTimeout,
                }
            )
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn paced_ca_pmt_end_to_end() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        let caid = 0x0B00_u16;
        let section = pmt_section(100, 1, caid);
        let ca_session = arm_ca_handshake_async(&mut cam, &mut events, caid).await;
        let armed = Instant::now();
        assert_eq!(handle.set_program(&section).unwrap(), 100);

        // the gate arms first: nothing reaches the CAM before the
        // interval elapses twice (arm pass + release pass)
        cam_quiet(&mut cam, &mut events, Duration::from_millis(250)).await;
        assert_eq!(
            cam_recv(&mut cam).await,
            ca_pmt_frame(
                0,
                ca_session,
                &section,
                caid,
                CaPmtListManagement::Only,
                CaPmtCommand::OkDescrambling,
            )
        );
        // the two-interval bound, measured from the CA_INFO confirmation:
        // the gate advances only on tick passes strictly past the stamp,
        // so a correctly paced release (arm pass, then release pass)
        // lands at or above two intervals, while a release on the
        // arm-to-ready transition pass lands near one interval
        let elapsed = armed.elapsed();
        assert!(
            elapsed >= pacing_config().ca_pmt_interval * 2,
            "first release after {elapsed:?}"
        );
        cam.send_status(0, false);

        // a second change is released one interval later, not sooner
        let second = pmt_section(200, 2, caid);
        let released = Instant::now();
        assert_eq!(handle.set_program(&second).unwrap(), 200);
        cam_quiet(&mut cam, &mut events, Duration::from_millis(150)).await;
        assert_eq!(
            cam_recv(&mut cam).await,
            ca_pmt_frame(
                0,
                ca_session,
                &second,
                caid,
                CaPmtListManagement::Add,
                CaPmtCommand::OkDescrambling,
            )
        );
        assert!(released.elapsed() >= pacing_config().ca_pmt_interval);
        cam.send_status(0, false);
    }

    #[tokio::test(start_paused = true)]
    async fn pre_handshake_commands_apply_after_gate_opens() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);

        // queued before the CAM is even present
        let caid = 0x0B00_u16;
        let section = pmt_section(100, 1, caid);
        assert_eq!(handle.set_program(&section).unwrap(), 100);
        cam_quiet(&mut cam, &mut events, Duration::from_millis(100)).await;

        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;
        let ca_session = arm_ca_handshake_async(&mut cam, &mut events, caid).await;

        cam_quiet(&mut cam, &mut events, Duration::from_millis(250)).await;
        assert_eq!(
            cam_recv(&mut cam).await,
            ca_pmt_frame(
                0,
                ca_session,
                &section,
                caid,
                CaPmtListManagement::Only,
                CaPmtCommand::OkDescrambling,
            )
        );
        cam.send_status(0, false);

        // exactly one release: later pacing passes have nothing queued
        cam_quiet(&mut cam, &mut events, Duration::from_millis(400)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn ready_watch_flips_on_gate_open_and_close() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        let mut ready = handle.ready_watch();
        assert!(!*ready.borrow_and_update());
        assert!(!handle.ca_pmt_ready());

        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;
        arm_ca_handshake_async(&mut cam, &mut events, 0x0B00).await;

        // the gate opens on a quiet tick pass that produces no event
        time::timeout(TEST_DEADLINE, ready.changed())
            .await
            .expect("gate opening within the test deadline")
            .unwrap();
        assert!(*ready.borrow_and_update());
        assert!(handle.ca_pmt_ready());

        // a global recovery closes the gate and the watch follows
        state.lock().unwrap().fail_slot_info = true;
        time::timeout(TEST_DEADLINE, ready.changed())
            .await
            .expect("gate closing within the test deadline")
            .unwrap();
        assert!(!*ready.borrow_and_update());
        assert!(!handle.ca_pmt_ready());
        state.lock().unwrap().fail_slot_info = false;
    }

    #[tokio::test(start_paused = true)]
    async fn suspended_link_waits_for_recovery_and_drains_after() {
        // a regression that keeps the descriptor in the select while the
        // link is suspended busy-loops without yielding, so the paused
        // clock never advances and the test would wedge instead of fail
        let _watchdog = RealTimeWatchdog::arm(
            "suspended_link_waits_for_recovery_and_drains_after wedged: the descriptor was \
             selected while the link was suspended",
        );
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (_handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // one failing slot-info pass suspends the link; the CA_RESET
        // succeeds, so recovery is scheduled at retry_interval
        state.lock().unwrap().fail_slot_info = true;
        assert!(matches!(
            next_event(&mut events).await,
            CiDriverEvent::Fault(_)
        ));
        state.lock().unwrap().fail_slot_info = false;
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotFailed {
                    slot_id: 0,
                    reason: CaSlotFailure::SlotInfoFailed,
                }
            )
        })
        .await;
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: 0,
                    new: CaSlotStatus::Failed,
                    ..
                }
            )
        })
        .await;

        // a stale frame sits in the descriptor for the whole suspension;
        // the driver neither reads it nor spins on the readable fd (a
        // busy-looping driver would never let the paused clock advance)
        cam.send_status(0, false);
        cam_quiet(&mut cam, &mut events, Duration::from_millis(40)).await;

        // recovery resumes supervision by itself: slot info is re-read, a
        // fresh CREATE_TC arrives and the stale frame is consumed
        // harmlessly by the resumption drain
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        cam.send_ctc_reply(0, false);
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: 0,
                    new: CaSlotStatus::Active,
                    ..
                }
            )
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn failed_reset_is_retried_internally_until_success() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        let mut ready = handle.ready_watch();
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // recovery runs and the global CA_RESET fails: the link suspends
        // with no scheduled recovery
        {
            let mut state = state.lock().unwrap();
            state.fail_reset = true;
            state.fail_slot_info = true;
        }
        assert!(matches!(
            next_event(&mut events).await,
            CiDriverEvent::Fault(_)
        ));
        state.lock().unwrap().fail_slot_info = false;
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotFailed {
                    slot_id: 0,
                    reason: CaSlotFailure::ResetFailed,
                }
            )
        })
        .await;

        // with no consumer action the driver retries CA_RESET at
        // retry_interval cadence
        let resets = state.lock().unwrap().resets;
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetryFailed(_))
        })
        .await;
        let stamp = Instant::now();
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetryFailed(_))
        })
        .await;
        let elapsed = stamp.elapsed();
        assert!(
            elapsed >= pacing_config().retry_interval,
            "cadence {elapsed:?}"
        );
        assert!(
            elapsed < pacing_config().retry_interval * 2,
            "cadence {elapsed:?}"
        );
        assert!(state.lock().unwrap().resets >= resets + 2);

        // the first successful retry resumes slot supervision
        state.lock().unwrap().fail_reset = false;
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetrySucceeded)
        })
        .await;
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        cam.send_ctc_reply(0, false);
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: 0,
                    new: CaSlotStatus::Active,
                    ..
                }
            )
        })
        .await;
        wait_ca_event(&mut events, |event| {
            matches!(event, CaEvent::TransportReady { slot_id: 0 })
        })
        .await;
        ack_initial_poll_async(&mut cam).await;

        // the ready machinery re-arms after a fresh handshake
        arm_ca_handshake_async(&mut cam, &mut events, 0x0B00).await;
        time::timeout(TEST_DEADLINE, ready.changed())
            .await
            .expect("gate opening within the test deadline")
            .unwrap();
        assert!(*ready.borrow());
    }

    #[tokio::test(start_paused = true)]
    async fn mmi_menu_answer_roundtrip() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        let session_id = open_resource_async(&mut cam, &mut events, 0, ResourceId::MMI, None).await;
        cam.send_apdu(0, session_id, ApduTag::MENU_LAST, &[]);
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::MmiMenu {
                    session_id: event_session,
                    ..
                } if *event_session == session_id
            )
        })
        .await;

        handle.mmi_menu_answer(0, session_id, 13);
        let mut answer = spdu::build_session_number(session_id);
        apdu::build(&mut answer, ApduTag::MENU_ANSW, &[13]);
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::DATA_LAST, &answer).unwrap()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn mmi_command_race_degrades_to_command_failed() {
        let (controller, _cam, _state) = pair(1);
        let (handle, mut events, task) = start(controller);

        // no active slot: the command fails as an event, the loop lives on
        handle.mmi_menu_answer(0, 5, 13);
        match next_event(&mut events).await {
            CiDriverEvent::CommandFailed { command, error } => {
                assert_eq!(command, "mmi_menu_answer");
                assert!(matches!(error, Error::InvalidProperty(_)));
            }
            event => panic!("unexpected event: {event:?}"),
        }

        handle.shutdown();
        assert!(matches!(task.await, Ok(Ok(()))));
    }

    #[test]
    fn handle_validation_is_synchronous() {
        let (controller, _cam, _state) = pair(1);
        let (_driver, handle, _events) = CiDriver::new(controller);

        assert!(handle.set_program(&[0u8; 8]).is_err());
        assert_eq!(
            handle.set_program(&pmt_section(100, 1, 0x0B00)).unwrap(),
            100
        );
        assert!(handle.remove_program(0).is_err());
        handle.remove_program(100).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn set_ca_pmt_interval_applies_live() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        let caid = 0x0B00_u16;
        let section = pmt_section(100, 1, caid);
        let ca_session = arm_ca_handshake_async(&mut cam, &mut events, caid).await;
        handle.set_program(&section).unwrap();
        assert_eq!(
            cam_recv(&mut cam).await,
            ca_pmt_frame(
                0,
                ca_session,
                &section,
                caid,
                CaPmtListManagement::Only,
                CaPmtCommand::OkDescrambling,
            )
        );
        cam.send_status(0, false);
        let released = Instant::now();

        // the new interval paces the next release
        let interval = Duration::from_millis(600);
        handle.set_ca_pmt_interval(interval);
        let second = pmt_section(200, 2, caid);
        handle.set_program(&second).unwrap();

        cam_quiet(&mut cam, &mut events, Duration::from_millis(550)).await;
        assert_eq!(
            cam_recv(&mut cam).await,
            ca_pmt_frame(
                0,
                ca_session,
                &second,
                caid,
                CaPmtListManagement::Add,
                CaPmtCommand::OkDescrambling,
            )
        );
        let elapsed = released.elapsed();
        assert!(elapsed >= interval, "released after {elapsed:?}");
        assert!(
            elapsed <= interval + Duration::from_millis(110),
            "released after {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_command_closes_device_and_streams() {
        let (controller, mut cam, _state) = pair(1);
        let (handle, mut events, task) = start(controller);
        let mut ready = handle.ready_watch();

        handle.shutdown();
        assert!(matches!(task.await, Ok(Ok(()))));

        // the event stream closes and the watch reports not-ready last
        while events.recv().await.is_some() {}
        while ready.changed().await.is_ok() {}
        assert!(!*ready.borrow());

        // the host descriptor is closed: the CaDevice dropped with the
        // controller
        assert!(cam.file.write(&[0]).is_err(), "host device stays open");
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_all_handles_stops_the_driver() {
        let (controller, _cam, _state) = pair(1);
        let (handle, mut events, task) = start(controller);

        drop(handle);
        assert!(matches!(task.await, Ok(Ok(()))));
        while events.recv().await.is_some() {}
    }

    #[tokio::test(start_paused = true)]
    async fn task_abort_closes_device() {
        let (controller, mut cam, _state) = pair(1);
        let (_handle, _events, task) = start(controller);

        // let the driver reach its select before aborting it
        time::sleep(STEP).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        // cancel-safe teardown released the descriptor
        assert!(cam.file.write(&[0]).is_err(), "host device stays open");
    }

    #[tokio::test(start_paused = true)]
    async fn link_failure_faults_and_keeps_retrying() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // kill the peer, then force one recovery; every following
        // recovery cycle fails writing CREATE_TC to the dead link and
        // schedules the next one
        drop(cam);
        state.lock().unwrap().fail_slot_info = true;
        assert!(matches!(
            next_event(&mut events).await,
            CiDriverEvent::Fault(_)
        ));
        state.lock().unwrap().fail_slot_info = false;

        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotFailed {
                    slot_id: 0,
                    reason: CaSlotFailure::LinkFailed,
                }
            )
        })
        .await;
        let resets = state.lock().unwrap().resets;
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::Fault(_))
        })
        .await;
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::Fault(_))
        })
        .await;
        assert!(state.lock().unwrap().resets >= resets + 2);
        assert!(!task.is_finished());

        handle.shutdown();
        assert!(matches!(task.await, Ok(Ok(()))));
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_frame_faults_suspends_and_recovers() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (_handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // a malformed frame fails the link read inside the readable-arm
        // drain: the fault is forwarded first, the recovery events queued
        // behind it are still drained, and the suspended pass leaves the
        // descriptor readiness alone
        cam.file.write_all(&[0xFF]).unwrap();
        assert!(matches!(
            next_event(&mut events).await,
            CiDriverEvent::Fault(_)
        ));
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotFailed {
                    slot_id: 0,
                    reason: CaSlotFailure::LinkFailed,
                }
            )
        })
        .await;

        // the driver sits out the suspension, then resumes supervision by
        // itself and the slot re-activates
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        cam.send_ctc_reply(0, false);
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: 0,
                    new: CaSlotStatus::Active,
                    ..
                }
            )
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn drain_batch_limit_keeps_buffered_frames_flowing() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (_handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;
        let session_id = open_resource_async(&mut cam, &mut events, 0, ResourceId::MMI, None).await;

        // buffer more frames than two drain batches before the driver
        // runs, so a readable-arm pass hits the batch limit with frames
        // still buffered
        let burst = 2 * DRAIN_BATCH + 6;
        for _ in 0 .. burst {
            cam.send_apdu(0, session_id, ApduTag::MENU_LAST, &[]);
        }
        let event = next_event(&mut events).await;
        assert!(
            matches!(event, CiDriverEvent::Ca(CaEvent::MmiMenu { .. })),
            "unexpected event: {event:?}"
        );
        // the batch limit yields to the select without clearing the
        // descriptor readiness, so once the first frame is handled the
        // remainder flows in the same instant; clearing at the limit
        // would strand the tail until a later tick's drain
        let stamp = Instant::now();
        for _ in 1 .. burst {
            let event = next_event(&mut events).await;
            assert!(
                matches!(event, CiDriverEvent::Ca(CaEvent::MmiMenu { .. })),
                "unexpected event: {event:?}"
            );
        }
        assert_eq!(stamp.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn reset_command_restarts_slot_supervision() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // a successful reset drops every session and restarts slot
        // supervision on the spot: no recovery wait, no retry cadence
        let resets = state.lock().unwrap().resets;
        handle.reset();
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
        cam.send_ctc_reply(0, false);
        wait_ca_event(&mut events, |event| {
            matches!(
                event,
                CaEvent::SlotStatusChanged {
                    slot_id: 0,
                    new: CaSlotStatus::Active,
                    ..
                }
            )
        })
        .await;
        assert_eq!(state.lock().unwrap().resets, resets + 1);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_reset_command_hands_over_to_internal_retry() {
        let (controller, mut cam, state) = pair_with(1, pacing_config());
        let (handle, mut events, _task) = start(controller);
        activate_async(&mut cam, &state, &mut events, 0).await;
        ack_initial_poll_async(&mut cam).await;

        // the failed reset surfaces as a command failure and suspends the
        // link with no scheduled recovery
        state.lock().unwrap().fail_reset = true;
        handle.reset();
        let event = wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::CommandFailed { .. })
        })
        .await;
        let CiDriverEvent::CommandFailed { command, .. } = event else {
            unreachable!();
        };
        assert_eq!(command, "reset");

        // the internal retry cadence takes over without any further
        // consumer action
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetryFailed(_))
        })
        .await;
        let stamp = Instant::now();
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetryFailed(_))
        })
        .await;
        let elapsed = stamp.elapsed();
        assert!(
            elapsed >= pacing_config().retry_interval,
            "cadence {elapsed:?}"
        );

        state.lock().unwrap().fail_reset = false;
        wait_event(&mut events, |event| {
            matches!(event, CiDriverEvent::ResetRetrySucceeded)
        })
        .await;
        assert_eq!(
            cam_recv(&mut cam).await,
            tpdu::build(0, TpduTag::CREATE_TC, &[]).unwrap()
        );
    }
}
