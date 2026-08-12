//! CA_PMT pacing policy: a deduplicated queue of program changes behind a
//! readiness gate.
//!
//! CAMs are sensitive to CA_PMT timing: many ignore or reject a command
//! sent right after the CA handshake, and rapid successive commands can
//! wedge the application. The pacer holds queued changes until one full
//! interval has passed since the confirmed handshake and then releases at
//! most one change per interval.

use std::{
    collections::VecDeque,
    time::{
        Duration,
        Instant,
    },
};

use super::capmt::Program;

/// One queued change of the desired-program registry
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CaPmtChange {
    Set(Program),
    Remove(u16),
}

impl CaPmtChange {
    fn program_number(&self) -> u16 {
        match self {
            CaPmtChange::Set(program) => program.program_number(),
            CaPmtChange::Remove(program_number) => *program_number,
        }
    }
}

/// CA_PMT readiness gate. `stamp` is the start of the current pacing
/// interval; the gate advances once per interval after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// No confirmed CAM handshake
    NotReady,
    /// APPLICATION_INFO or a non-empty CA_INFO arrived; one full interval
    /// after `stamp` the CAM counts as ready (no change is released on
    /// the transition pass)
    Armed { stamp: Instant },
    /// The CAM accepts CA_PMT; one queued change is released per interval
    Ready { stamp: Instant },
}

/// Paced, deduplicated CA_PMT change queue
pub(super) struct CaPmtPacer {
    interval: Duration,
    settle: Duration,
    gate: Gate,
    queue: VecDeque<CaPmtChange>,
}

impl CaPmtPacer {
    pub fn new(interval: Duration, settle: Duration) -> Self {
        CaPmtPacer {
            interval,
            settle,
            gate: Gate::NotReady,
            queue: VecDeque::new(),
        }
    }

    /// Changes the pacing interval; effective from the next poll
    pub fn set_interval(&mut self, interval: Duration) {
        self.interval = interval;
    }

    /// Whether the readiness gate is open: the CAM accepts CA_PMT
    pub fn ready(&self) -> bool {
        matches!(self.gate, Gate::Ready { .. })
    }

    /// Queues a program select. A queued select for the same program is
    /// replaced; a queued remove stays ahead of the new select.
    pub fn push_set(&mut self, program: Program) {
        let program_number = program.program_number();
        self.queue.retain(|change| {
            !matches!(change, CaPmtChange::Set(queued) if queued.program_number() == program_number)
        });
        self.queue.push_back(CaPmtChange::Set(program));
    }

    /// Queues a program remove, dropping every queued change for the same
    /// program first
    pub fn push_remove(&mut self, program_number: u16) {
        self.queue
            .retain(|change| change.program_number() != program_number);
        self.queue.push_back(CaPmtChange::Remove(program_number));
    }

    /// (Re)arms the readiness countdown with the extra settle hold: some
    /// CAMs (NDS Videoguard) reject the CA handshake continuation right
    /// after identification
    pub fn arm_application_info(&mut self, now: Instant) {
        let stamp = now.checked_add(self.settle).unwrap_or(now);
        self.gate = Gate::Armed { stamp };
    }

    /// (Re)arms the readiness countdown from a confirmed non-empty CA_INFO
    pub fn arm_ca_info(&mut self, now: Instant) {
        self.gate = Gate::Armed { stamp: now };
    }

    /// Closes the gate; queued changes are kept for the next handshake
    pub fn reset_gate(&mut self) {
        self.gate = Gate::NotReady;
    }

    /// One pacing pass. Nothing happens within one interval of the gate
    /// stamp. On a pass that clears the gate the stamp always advances (a
    /// change never waits longer than about one interval), and: `Armed`
    /// becomes `Ready` without releasing a change, `Ready` releases at
    /// most one queued change.
    pub fn poll(&mut self, now: Instant) -> Option<CaPmtChange> {
        let stamp = match self.gate {
            Gate::NotReady => return None,
            Gate::Armed { stamp } | Gate::Ready { stamp } => stamp,
        };
        let gated = stamp
            .checked_add(self.interval)
            .is_none_or(|deadline| now <= deadline);
        if gated {
            return None;
        }

        let open = matches!(self.gate, Gate::Ready { .. });
        self.gate = Gate::Ready { stamp: now };
        if open {
            self.queue.pop_front()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use libmpegts::psi::{
        PmtBuilder,
        PmtConfig,
        PmtStream,
    };

    use super::*;

    const INTERVAL: Duration = Duration::from_secs(20);
    const SETTLE: Duration = Duration::from_secs(10);

    fn program(program_number: u16, version: u8) -> Program {
        let descriptor = vec![0x09, 0x04, 0x01, 0x00, 0xE1, 0xEC];
        let section = &PmtBuilder::build(PmtConfig {
            program_number,
            pcr_pid: 0x0100,
            version,
            program_descriptors: descriptor,
            streams: vec![PmtStream {
                stream_type: 0x1B,
                elementary_pid: 0x0100,
                stream_descriptors: Vec::new(),
            }],
        })[0];
        Program::parse(section).unwrap()
    }

    fn set(program_number: u16, version: u8) -> CaPmtChange {
        CaPmtChange::Set(program(program_number, version))
    }

    fn pacer() -> CaPmtPacer {
        CaPmtPacer::new(INTERVAL, SETTLE)
    }

    fn queued(pacer: &CaPmtPacer) -> Vec<CaPmtChange> {
        pacer.queue.iter().cloned().collect()
    }

    #[test]
    fn queue_set_replaces_queued_set() {
        let mut pacer = pacer();
        pacer.push_set(program(100, 1));
        pacer.push_set(program(200, 2));
        pacer.push_set(program(100, 3));

        assert_eq!(queued(&pacer), vec![set(200, 2), set(100, 3)]);
    }

    #[test]
    fn queue_remove_drops_all_changes_for_program() {
        let mut pacer = pacer();
        pacer.push_set(program(100, 1));
        pacer.push_remove(100);
        pacer.push_remove(100);
        pacer.push_set(program(200, 2));
        pacer.push_remove(100);

        assert_eq!(queued(&pacer), vec![set(200, 2), CaPmtChange::Remove(100)]);
    }

    #[test]
    fn queue_set_keeps_queued_remove_ahead() {
        let mut pacer = pacer();
        pacer.push_remove(100);
        pacer.push_set(program(100, 1));

        assert_eq!(queued(&pacer), vec![CaPmtChange::Remove(100), set(100, 1)]);
    }

    #[test]
    fn poll_not_ready_never_releases() {
        let mut pacer = CaPmtPacer::new(Duration::ZERO, Duration::ZERO);
        pacer.push_set(program(100, 1));

        assert_eq!(pacer.poll(Instant::now()), None);
        assert!(!pacer.ready());
        assert_eq!(pacer.queue.len(), 1);
    }

    #[test]
    fn poll_armed_gates_then_opens_without_release() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.push_set(program(100, 1));
        pacer.arm_ca_info(start);

        // still inside the interval (the boundary itself stays gated)
        assert_eq!(pacer.poll(start + INTERVAL), None);
        assert!(!pacer.ready());

        // past the interval: transition only, no change on this pass
        assert_eq!(pacer.poll(start + INTERVAL + Duration::from_secs(1)), None);
        assert!(pacer.ready());
        assert_eq!(pacer.queue.len(), 1);
    }

    #[test]
    fn poll_ready_releases_one_change_per_interval() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.push_set(program(100, 1));
        pacer.push_set(program(200, 2));
        pacer.arm_ca_info(start);

        let t1 = start + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t1), None);

        let t2 = t1 + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t2), Some(set(100, 1)));

        // the next change waits for the next interval
        assert_eq!(pacer.poll(t2 + Duration::from_secs(1)), None);

        let t3 = t2 + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t3), Some(set(200, 2)));
    }

    #[test]
    fn poll_stamp_advances_on_empty_queue() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.arm_ca_info(start);

        let t1 = start + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t1), None);
        assert!(pacer.ready());

        // the gate clears on an empty queue and still advances the stamp:
        // a change queued right after waits for the next interval
        let t2 = t1 + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t2), None);
        pacer.push_set(program(100, 1));
        assert_eq!(pacer.poll(t2 + Duration::from_secs(1)), None);
        assert_eq!(pacer.queue.len(), 1);
    }

    #[test]
    fn arm_application_info_adds_settle_hold() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.arm_application_info(start);

        assert_eq!(pacer.poll(start + SETTLE + INTERVAL), None);
        assert!(!pacer.ready());

        assert_eq!(
            pacer.poll(start + SETTLE + INTERVAL + Duration::from_secs(1)),
            None
        );
        assert!(pacer.ready());
    }

    #[test]
    fn arm_ca_info_overrides_settle_hold() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.arm_application_info(start);
        pacer.arm_ca_info(start);

        assert_eq!(pacer.poll(start + INTERVAL + Duration::from_secs(1)), None);
        assert!(pacer.ready());
    }

    #[test]
    fn reset_gate_closes_and_keeps_queue() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.push_set(program(100, 1));
        pacer.arm_ca_info(start);
        assert_eq!(pacer.poll(start + INTERVAL + Duration::from_secs(1)), None);
        assert!(pacer.ready());

        pacer.reset_gate();

        assert!(!pacer.ready());
        assert_eq!(pacer.poll(start + INTERVAL * 10), None);
        assert_eq!(pacer.queue.len(), 1);
    }

    #[test]
    fn set_interval_takes_effect_next_poll() {
        let mut pacer = pacer();
        let start = Instant::now();
        pacer.push_set(program(100, 1));
        pacer.arm_ca_info(start);
        let t1 = start + INTERVAL + Duration::from_secs(1);
        assert_eq!(pacer.poll(t1), None);

        pacer.set_interval(Duration::from_secs(1));

        let t2 = t1 + Duration::from_secs(2);
        assert_eq!(pacer.poll(t2), Some(set(100, 1)));
    }
}
