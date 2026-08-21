//! Interactive access to the CAM menu of a CI adapter
//!
//! Usage: camenu [-h] <adapter> [device]
//!
//! Opens /dev/dvb/adapterN/caM, reports the inserted CAM - vendor,
//! manufacturer code, menu string and the CA systems it supports - and then
//! opens the CAM menu on its own, giving line-oriented access to the
//! high-level MMI dialogue: the provider menu, lists and PIN enquiries.
//!
//! The loop waits on the CI device and on the terminal with poll(2), so it
//! neither blocks on the user nor spins; a blind enquiry is typed with the
//! terminal echo off; SIGINT, SIGTERM and SIGHUP take the open dialogue down
//! before the example leaves.
//!
//! Note that opening the controller resets the CI interface of the adapter
//! (`CiController::with_config` issues CA_RESET), so every CAM of the adapter
//! restarts and whatever it was descrambling stops for the duration.

use std::{
    error::Error,
    io::{
        self,
        Write,
    },
    os::fd::AsFd,
    process::ExitCode,
    sync::atomic::{
        AtomicBool,
        Ordering,
    },
    time::{
        Duration,
        Instant,
    },
};

use libdvb::{
    CaEvent,
    CaSlotStatus,
    CamStatus,
    CiController,
    ca::{
        ApplicationInfo,
        MmiMenu,
        ResourceId,
    },
};
use nix::{
    errno::Errno,
    libc,
    poll::{
        PollFd,
        PollFlags,
        PollTimeout,
        poll,
    },
    sys::{
        signal::{
            SaFlags,
            SigAction,
            SigHandler,
            SigSet,
            Signal,
            sigaction,
        },
        termios::{
            LocalFlags,
            SetArg,
            Termios,
            tcgetattr,
            tcsetattr,
        },
    },
    unistd::{
        isatty,
        read,
    },
};

/// How long the loop waits before driving the controller again. The default
/// `CiControllerConfig::transport_poll_interval` is the shortest period the
/// controller needs a tick at; CI data and user input arrive as a wake-up
/// rather than at this rate.
const TICK_MS: u16 = 100;
const TICK_INTERVAL: Duration = Duration::from_millis(TICK_MS as u64);

/// How long the exit waits for the modules to acknowledge the MMI close
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

/// Longest input line accepted; no CAM answer comes close
const LINE_LIMIT: usize = 256;

const USAGE: &str = "\
Usage: camenu [OPTIONS] <adapter> [device]

Interactive access to the CAM menu of a DVB CI adapter (/dev/dvb/adapterN/caM).
The menu of an inserted CAM opens by itself. Opening the adapter resets its CI
interface: every CAM restarts and stops descrambling while camenu runs.

Options:
    -h, --help    print this text and exit
";

const COMMANDS: &str = "\
Commands:
    <number>      answer the open menu, 0 cancels it
    <text>        answer the open enquiry, an empty line cancels it
    menu [slot]   ask the CAM to open its menu again
    slot <n>      answer the dialogue of another slot
    close         take the open MMI dialogue down
    info          report the state of every slot
    reset         reset the CI interface - every slot of the adapter restarts
    help          print this text
    q, quit       leave

A command typed at an open menu or list runs as a command; an enquiry takes
every line as its answer, and an empty line cancels it.
";

/// Set from the signal handler; the loop leaves on the next pass
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// A signal handler may touch nothing but an atomic. `poll` returns EINTR, so
/// the flag is seen without waiting for the tick.
extern "C" fn on_signal(_signal: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn install_signal_handlers() -> nix::Result<()> {
    let action = SigAction::new(
        SigHandler::Handler(on_signal),
        // no SA_RESTART: an interrupted poll must return to the loop
        SaFlags::empty(),
        SigSet::empty(),
    );

    // SAFETY: the handler only stores into a static AtomicBool, which is
    // async-signal-safe
    unsafe {
        sigaction(Signal::SIGINT, &action)?;
        sigaction(Signal::SIGTERM, &action)?;
        sigaction(Signal::SIGHUP, &action)?;
    }

    Ok(())
}

/// Hides what the user types while a blind enquiry - a PIN - is answered and
/// restores the terminal on drop, whichever way the example leaves
struct Echo {
    /// The settings to put back once the input may be shown again
    saved: Option<Termios>,
    /// The terminal refused to hide the input; the user is told once
    warned: bool,
}

impl Echo {
    const fn new() -> Self {
        Echo {
            saved: None,
            warned: false,
        }
    }

    /// Turns the echo off; reports whether the input is really hidden
    fn hide(&mut self) -> bool {
        if self.saved.is_some() {
            return true;
        }

        let stdin = io::stdin();

        // input from a pipe or a file is not echoed anywhere to begin with
        if !isatty(&stdin).unwrap_or(false) {
            return true;
        }

        let saved = match tcgetattr(&stdin) {
            Ok(saved) => saved,
            Err(e) => return self.warn(e),
        };

        let mut hidden = saved.clone();
        hidden.local_flags.remove(LocalFlags::ECHO);

        // TCSAFLUSH drops whatever was typed before the prompt appeared, so a
        // stray keystroke cannot end up inside a PIN
        if let Err(e) = tcsetattr(&stdin, SetArg::TCSAFLUSH, &hidden) {
            return self.warn(e);
        }

        self.saved = Some(saved);
        true
    }

    /// Puts the terminal back the way it was
    fn show(&mut self) {
        if let Some(saved) = self.saved.take() {
            let _ = tcsetattr(io::stdin(), SetArg::TCSANOW, &saved);
            // the line break the terminal did not echo when Enter was pressed
            println!();
        }
    }

    fn warn(&mut self, e: Errno) -> bool {
        if !self.warned {
            self.warned = true;
            eprintln!("camenu: cannot hide the terminal input ({e}): it stays visible");
        }

        false
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        self.show();
    }
}

/// Cuts what stdin has ready into lines; the loop is never blocked on the user
struct LineReader {
    buf: Vec<u8>,
    /// stdin is at its end: the controller keeps running without a user
    eof: bool,
    /// The line being read passed `LINE_LIMIT` and is discarded
    overflow: bool,
}

impl LineReader {
    fn new() -> Self {
        LineReader {
            buf: Vec::with_capacity(LINE_LIMIT),
            eof: false,
            overflow: false,
        }
    }

    fn at_eof(&self) -> bool {
        self.eof
    }

    /// Reads once - poll said stdin is readable - and returns the lines the
    /// data completed
    fn read_lines(&mut self) -> Vec<String> {
        let mut chunk = [0_u8; 512];

        let read = loop {
            match read(io::stdin(), &mut chunk) {
                Ok(len) => break len,
                Err(Errno::EINTR) => continue,
                // a readable stdin with nothing to read: try again later
                Err(Errno::EAGAIN) => return Vec::new(),
                Err(e) => {
                    eprintln!("camenu: stdin: {e}");
                    self.eof = true;
                    return Vec::new();
                }
            }
        };

        if read == 0 {
            self.eof = true;
            return Vec::new();
        }

        self.split(&chunk[.. read])
    }

    fn split(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();

        for &byte in data {
            match byte {
                b'\n' => {
                    if self.overflow {
                        eprintln!("camenu: the input line is too long, discarded");
                        self.overflow = false;
                    } else {
                        let line = String::from_utf8_lossy(&self.buf);
                        lines.push(line.trim_end_matches('\r').to_owned());
                    }

                    self.buf.clear();
                }
                _ if self.buf.len() >= LINE_LIMIT => {
                    self.overflow = true;
                    self.buf.clear();
                }
                _ => self.buf.push(byte),
            }
        }

        lines
    }
}

/// MMI dialogue a module is waiting for an answer to. A slot runs one
/// dialogue at a time; the session it belongs to has to be answered exactly,
/// as a module may take it down and open the next one at any moment.
enum Pending {
    None,
    Menu {
        session_id: u16,
        /// Highest item number the module offered
        items: u8,
    },
    List {
        session_id: u16,
    },
    Enq {
        session_id: u16,
        /// The answer must not appear on the screen
        blind: bool,
        /// Number of characters the module expects; 0 leaves it open
        answer_len: u8,
    },
}

impl Pending {
    fn session_id(&self) -> Option<u16> {
        match *self {
            Pending::None => None,
            Pending::Menu { session_id, .. }
            | Pending::List { session_id }
            | Pending::Enq { session_id, .. } => Some(session_id),
        }
    }

    fn is_open(&self) -> bool {
        !matches!(*self, Pending::None)
    }

    fn is_blind(&self) -> bool {
        matches!(*self, Pending::Enq { blind: true, .. })
    }

    /// One line reminding the user what the module is waiting for
    fn prompt(&self) -> Option<String> {
        match *self {
            Pending::None => None,
            Pending::Menu { items, .. } => {
                Some(format!("enter an item number from 0 to {items}, 0 cancels"))
            }
            Pending::List { .. } => Some("press Enter when done".to_owned()),
            Pending::Enq { answer_len: 0, .. } => Some("answer, an empty line cancels".to_owned()),
            Pending::Enq { answer_len, .. } => Some(format!(
                "answer ({answer_len} characters), an empty line cancels"
            )),
        }
    }
}

/// A line typed with no dialogue open, or a word typed at a menu
#[derive(Debug, PartialEq, Eq)]
enum Command<'a> {
    /// An empty line
    Nothing,
    Quit,
    /// `menu [slot]`
    Menu(Option<&'a str>),
    /// `slot <n>`
    Focus(Option<&'a str>),
    Close,
    Info,
    Reset,
    Help,
    /// Not a command at all
    Unknown(&'a str),
}

fn parse_command(line: &str) -> Command<'_> {
    let mut words = line.split_whitespace();
    let keyword = words.next().unwrap_or_default();
    let argument = words.next();

    match keyword {
        "" => Command::Nothing,
        "q" | "quit" | "exit" => Command::Quit,
        "menu" => Command::Menu(argument),
        "slot" => Command::Focus(argument),
        "close" => Command::Close,
        "info" => Command::Info,
        "reset" => Command::Reset,
        "help" | "?" => Command::Help,
        other => Command::Unknown(other),
    }
}

/// Whether a line is a command and not an answer. A menu is answered with a
/// number and a list with Enter, so a keyword typed at one is unambiguous: an
/// open dialogue must not lock the user out of the commands.
fn is_command(line: &str) -> bool {
    !matches!(parse_command(line), Command::Nothing | Command::Unknown(_))
}

/// What a line typed at an open enquiry means
#[derive(Debug, PartialEq, Eq)]
enum EnqAnswer<'a> {
    /// An empty line: the enquiry is cancelled
    Cancel,
    /// The answer to hand to the module
    Send(&'a str),
    /// The module would not accept it; the user is told why
    Reject(String),
}

/// Checks a line against what the module asked for. A module counts
/// characters, not multi-byte sequences, and understands the ASCII range
/// only; `answer_len` is the exact length it expects, 0 leaving it open.
fn check_enq_answer(line: &str, answer_len: u8) -> EnqAnswer<'_> {
    if line.is_empty() {
        return EnqAnswer::Cancel;
    }

    if !line.bytes().all(|byte| (0x20 ..= 0x7E).contains(&byte)) {
        return EnqAnswer::Reject("only printable ASCII characters are accepted".to_owned());
    }

    if answer_len != 0 && line.len() != usize::from(answer_len) {
        return EnqAnswer::Reject(format!(
            "the module expects exactly {answer_len} characters, {} were typed",
            line.len()
        ));
    }

    EnqAnswer::Send(line)
}

/// Checks a line against the items a menu offers; `None` is not an answer the
/// module could be given
fn check_menu_answer(line: &str, items: u8) -> Option<u8> {
    match line.parse::<u8>() {
        Ok(choice) if choice <= items => Some(choice),
        _ => None,
    }
}

struct App {
    /// The dialogue each slot is in, indexed by slot id. A two-slot adapter
    /// runs two of them: both CAMs open their menu when they identify
    /// themselves.
    dialogues: Vec<Pending>,
    /// The slot the next line is answered to
    focus: Option<u8>,
    /// Slots whose CAM has already been asked to open its menu, by slot id. A
    /// CAM may repeat its application info, and a second `enter_menu` would
    /// throw away the dialogue the user is in the middle of.
    menu_requested: Vec<bool>,
    /// MMI sessions the modules opened, closed politely on the way out
    mmi_sessions: Vec<(u8, u16)>,
    echo: Echo,
}

impl App {
    fn new(slots_num: u8) -> Self {
        let slots = usize::from(slots_num);

        App {
            dialogues: (0 .. slots).map(|_| Pending::None).collect(),
            focus: None,
            menu_requested: vec![false; slots],
            mmi_sessions: Vec::new(),
            echo: Echo::new(),
        }
    }

    fn slots_num(&self) -> u8 {
        self.dialogues.len() as u8
    }

    fn dialogue(&self, slot_id: u8) -> Option<&Pending> {
        self.dialogues.get(usize::from(slot_id))
    }

    /// The slot and the dialogue the next line is answered to
    fn focused(&self) -> Option<(u8, &Pending)> {
        let slot_id = self.focus?;
        let pending = self.dialogue(slot_id)?;

        pending.is_open().then_some((slot_id, pending))
    }

    /// Puts a slot into a dialogue - or out of one with `Pending::None` - and
    /// settles which slot is answered next
    fn set_dialogue(&mut self, slot_id: u8, pending: Pending) {
        let Some(slot) = self.dialogues.get_mut(usize::from(slot_id)) else {
            // a slot the device did not report when the controller was opened
            return;
        };

        let opened = pending.is_open();
        let waiting = opened && self.focus.is_some_and(|focus| focus != slot_id);
        *slot = pending;

        if waiting {
            println!(
                "CI slot {slot_id}: a dialogue is waiting; \"slot {slot_id}\" answers it instead"
            );
        }

        // handing the focus over to another slot is worth a line; a dialogue
        // whose prompt was just printed is not
        self.refocus(!opened);
    }

    /// Moves the focus off a slot that has nothing open and onto one that has
    fn refocus(&mut self, announce: bool) {
        if self.focused().is_some() {
            self.sync_echo();
            return;
        }

        self.focus = self
            .dialogues
            .iter()
            .position(|pending| pending.is_open())
            .map(|slot_id| slot_id as u8);

        self.sync_echo();

        // with a single slot the prompts name it already
        if announce
            && self.slots_num() > 1
            && let Some((slot_id, pending)) = self.focused()
            && let Some(prompt) = pending.prompt()
        {
            println!("CI slot {slot_id}: answering its dialogue now");
            println!("    -> {prompt}");
        }
    }

    /// Hides the terminal input while, and only while, the dialogue being
    /// answered is a blind enquiry; reports whether it is really hidden
    fn sync_echo(&mut self) -> bool {
        if self
            .focused()
            .is_some_and(|(_, pending)| pending.is_blind())
        {
            self.echo.hide()
        } else {
            self.echo.show();
            true
        }
    }

    /// `slot <n>`: answers the dialogue of another slot
    fn focus_command(&mut self, argument: Option<&str>) {
        let waiting: Vec<u8> = self
            .dialogues
            .iter()
            .enumerate()
            .filter(|(_, pending)| pending.is_open())
            .map(|(slot_id, _)| slot_id as u8)
            .collect();

        let slot_id = match argument.map(str::parse::<u8>) {
            Some(Ok(slot_id)) if waiting.contains(&slot_id) => slot_id,
            _ => {
                if waiting.is_empty() {
                    println!("No dialogue is open");
                } else {
                    println!("Slots with a dialogue open: {waiting:?}");
                }

                return;
            }
        };

        self.focus = Some(slot_id);
        self.sync_echo();

        if let Some(prompt) = self.dialogue(slot_id).and_then(Pending::prompt) {
            println!("CI slot {slot_id}: -> {prompt}");
        }
    }

    /// Drops the dialogue of one session, wherever the focus is
    fn clear_session(&mut self, slot_id: u8, session_id: u16) {
        if self
            .dialogue(slot_id)
            .and_then(Pending::session_id)
            .is_some_and(|open| open == session_id)
        {
            self.set_dialogue(slot_id, Pending::None);
        }
    }

    /// Forgets the state of a slot whose CAM went away, so that the menu of
    /// the next CAM is opened again
    fn forget_slot(&mut self, slot_id: u8) {
        self.set_dialogue(slot_id, Pending::None);
        self.mmi_sessions.retain(|&(slot, _)| slot != slot_id);

        if let Some(requested) = self.menu_requested.get_mut(usize::from(slot_id)) {
            *requested = false;
        }
    }

    /// Opens the CAM menu once per CAM, right after its application info: the
    /// request travels on the Application Information session, so this is the
    /// earliest moment it is valid
    fn auto_enter_menu(&mut self, ci: &mut CiController, slot_id: u8) {
        // a slot the device did not report when the controller was opened is
        // left alone
        if let Some(requested) = self.menu_requested.get_mut(usize::from(slot_id))
            && !*requested
        {
            *requested = true;
            enter_menu(ci, slot_id);
        }
    }

    fn handle_event(&mut self, ci: &mut CiController, event: CaEvent) {
        match event {
            CaEvent::SlotStatusChanged { slot_id, old, new } => {
                println!("CI slot {slot_id}: {old:?} -> {new:?}");
                if new != CaSlotStatus::Active {
                    self.forget_slot(slot_id);
                }
            }
            CaEvent::CamStatusChanged { slot_id, old, new } => {
                println!("CI slot {slot_id}: CAM {old:?} -> {new:?}");
                // the Application Information session is gone: after a
                // recovery or a new CAM the menu is opened again
                if !matches!(new, CamStatus::ApplicationInfo | CamStatus::Ready) {
                    self.forget_slot(slot_id);
                }
            }
            CaEvent::SlotFailed { slot_id, reason } => {
                eprintln!("CI slot {slot_id}: failed ({reason:?}); the controller recovers it");
                self.forget_slot(slot_id);
            }
            CaEvent::TransportReady { slot_id } => {
                println!("CI slot {slot_id}: transport connection established");
            }
            CaEvent::SessionOpened {
                slot_id,
                session_id,
                resource_id,
            } => {
                println!("CI slot {slot_id}: session {session_id} opened ({resource_id:?})");
                if resource_id == ResourceId::MMI {
                    self.mmi_sessions.push((slot_id, session_id));
                }
            }
            CaEvent::SessionRefused {
                slot_id,
                resource_id,
                status,
            } => {
                let reason = match status {
                    0xF0 => "the resource does not exist",
                    0xF2 => "only a lower version is available",
                    0xF3 => "no free session numbers",
                    _ => "unknown reason",
                };
                eprintln!(
                    "CI slot {slot_id}: session for {resource_id:?} refused: {reason} \
                     (0x{status:02X})"
                );
            }
            CaEvent::SessionClosed {
                slot_id,
                session_id,
                resource_id,
            } => {
                println!("CI slot {slot_id}: session {session_id} closed ({resource_id:?})");
                // the dialogue also ends when the module drops the session
                // without a close_mmi: the pending answer must not outlive it
                self.clear_session(slot_id, session_id);
                self.mmi_sessions
                    .retain(|&session| session != (slot_id, session_id));
            }
            CaEvent::ApplicationInfo { slot_id, info } => {
                print_app_info(slot_id, &info);
                self.auto_enter_menu(ci, slot_id);
            }
            CaEvent::CaInfo {
                slot_id,
                session_id,
                caids,
            } => {
                println!("CI slot {slot_id}, CA session {session_id}: CAIDs {caids:04X?}");
            }
            CaEvent::MmiMenu {
                slot_id,
                session_id,
                menu,
            } => {
                print_menu("menu", slot_id, session_id, &menu);
                let items = item_count(&menu);
                println!("    -> enter an item number from 0 to {items}, 0 cancels");
                self.set_dialogue(slot_id, Pending::Menu { session_id, items });
            }
            CaEvent::MmiList {
                slot_id,
                session_id,
                menu,
            } => {
                print_menu("list", slot_id, session_id, &menu);
                println!("    -> press Enter when done");
                self.set_dialogue(slot_id, Pending::List { session_id });
            }
            CaEvent::MmiEnq {
                slot_id,
                session_id,
                blind,
                answer_len,
                text,
            } => {
                println!("CI slot {slot_id}, MMI session {session_id}: enquiry");
                print_text("    ", &text);

                self.set_dialogue(
                    slot_id,
                    Pending::Enq {
                        session_id,
                        blind,
                        answer_len,
                    },
                );

                match answer_len {
                    0 => println!("    -> answer, an empty line cancels"),
                    len => println!("    -> answer ({len} characters), an empty line cancels"),
                }

                // the input is hidden only while this enquiry is the one being
                // answered; another slot in the middle of a dialogue keeps it
                if blind && !self.sync_echo() {
                    println!("    note: the answer will be visible on the screen");
                }
            }
            CaEvent::MmiText {
                slot_id,
                session_id,
                text,
            } => {
                println!("CI slot {slot_id}, MMI session {session_id}: text");
                print_text("    ", &text);
            }
            CaEvent::MmiClose {
                slot_id,
                session_id,
                delay,
            } => {
                // the session stack answers the close itself; nothing is left
                // to do but drop the dialogue
                let delay = match delay {
                    Some(seconds) => format!(" in {seconds}s"),
                    None => String::new(),
                };
                println!("CI slot {slot_id}, MMI session {session_id}: closed{delay}");
                self.clear_session(slot_id, session_id);
            }
            CaEvent::Tune {
                slot_id,
                network_id,
                original_network_id,
                transport_stream_id,
                service_id,
            } => {
                println!(
                    "CI slot {slot_id}: the module asks to tune to service {service_id} \
                     (network {network_id}, original network {original_network_id}, \
                     transport stream {transport_stream_id}); this example does not tune"
                );
            }
            CaEvent::Replace {
                slot_id,
                replace_ref,
                replaced_pid,
                replacement_pid,
            } => {
                println!(
                    "CI slot {slot_id}: the module asks to replace PID {replaced_pid} with \
                     {replacement_pid} (reference {replace_ref}); this example passes no stream"
                );
            }
            CaEvent::ClearReplace {
                slot_id,
                replace_ref,
            } => {
                println!("CI slot {slot_id}: replace reference {replace_ref} withdrawn");
            }
            CaEvent::Malformed { slot_id, context } => {
                eprintln!("CI slot {slot_id}: warning: malformed data: {context}");
            }
        }
    }

    /// Handles one line from the user; returns `false` to leave the loop
    fn handle_line(&mut self, ci: &mut CiController, line: &str) -> bool {
        let (slot_id, answer_to) = match self.focused() {
            Some((slot_id, pending)) => (slot_id, pending),
            None => return self.run(ci, parse_command(line)),
        };

        match *answer_to {
            Pending::None => unreachable!("the focus is only set on an open dialogue"),
            // an enquiry answer may look like anything - a name, a PIN - so
            // nothing is taken out of it: an empty line cancels the enquiry
            // and a signal leaves at any time
            Pending::Enq {
                session_id,
                answer_len,
                ..
            } => self.answer_enq(ci, slot_id, session_id, answer_len, line),
            Pending::Menu { session_id, items } => match check_menu_answer(line, items) {
                Some(choice) => {
                    log_error(ci.mmi_menu_answer(slot_id, session_id, choice));
                    // the module answers with the next object of the dialogue
                    self.set_dialogue(slot_id, Pending::None);
                }
                None if is_command(line) => return self.run(ci, parse_command(line)),
                None => println!("    -> enter an item number from 0 to {items}, 0 cancels"),
            },
            Pending::List { session_id } => {
                if line.is_empty() {
                    log_error(ci.mmi_list_close(slot_id, session_id));
                    self.set_dialogue(slot_id, Pending::None);
                } else if is_command(line) {
                    return self.run(ci, parse_command(line));
                } else {
                    println!("    -> press Enter to close the list");
                }
            }
        }

        true
    }

    fn answer_enq(
        &mut self,
        ci: &mut CiController,
        slot_id: u8,
        session_id: u16,
        answer_len: u8,
        line: &str,
    ) {
        let answer = match check_enq_answer(line, answer_len) {
            EnqAnswer::Cancel => None,
            EnqAnswer::Send(answer) => Some(answer),
            EnqAnswer::Reject(reason) => {
                println!("    -> {reason}");
                return;
            }
        };

        log_error(ci.mmi_answer(slot_id, session_id, answer.map(str::as_bytes)));
        self.set_dialogue(slot_id, Pending::None);
    }

    /// Runs a command; returns `false` to leave the loop
    fn run(&mut self, ci: &mut CiController, command: Command<'_>) -> bool {
        match command {
            Command::Nothing => {}
            Command::Quit => return false,
            Command::Menu(argument) => self.menu_command(ci, argument),
            Command::Focus(argument) => self.focus_command(argument),
            Command::Close => self.close_command(ci),
            Command::Info => info_command(ci),
            Command::Reset => {
                println!("CI: resetting the interface: every slot of the adapter restarts");
                log_error(ci.reset());
            }
            Command::Help => print!("{COMMANDS}"),
            Command::Unknown(keyword) => {
                println!("Unknown command {keyword:?}; \"help\" lists the commands");
            }
        }

        true
    }

    /// `menu [slot]`: asks the CAM of a slot to open its menu again
    fn menu_command(&mut self, ci: &mut CiController, argument: Option<&str>) {
        let slots_num = self.slots_num();

        let slot_id = match argument {
            None if slots_num == 1 => 0,
            None => {
                println!("The adapter has {slots_num} slots: \"menu <slot>\"");
                return;
            }
            Some(argument) => match argument.parse::<u8>() {
                Ok(slot_id) if slot_id < slots_num => slot_id,
                _ => {
                    println!("The slot number must be below {slots_num}");
                    return;
                }
            },
        };

        if let Some(requested) = self.menu_requested.get_mut(usize::from(slot_id)) {
            *requested = true;
        }

        enter_menu(ci, slot_id);
    }

    /// `close`: takes the open dialogues down without leaving the example
    fn close_command(&mut self, ci: &mut CiController) {
        let sessions = self.mmi_sessions.clone();

        if sessions.is_empty() {
            println!("No MMI dialogue is open");
            return;
        }

        for (slot_id, session_id) in sessions {
            println!("CI slot {slot_id}: closing MMI session {session_id}");
            log_error(ci.mmi_close(slot_id, session_id));
        }
    }

    /// Asks the modules to take their dialogues down and gives them a moment
    /// to answer: a CAM left with a session open can refuse the next one
    fn shutdown(&mut self, ci: &mut CiController) {
        self.focus = None;
        self.dialogues
            .iter_mut()
            .for_each(|slot| *slot = Pending::None);
        self.echo.show();

        let sessions = std::mem::take(&mut self.mmi_sessions);
        if sessions.is_empty() {
            return;
        }

        for &(slot_id, session_id) in &sessions {
            println!("CI slot {slot_id}: closing MMI session {session_id}");
            log_error(ci.mmi_close(slot_id, session_id));
        }

        let deadline = Instant::now() + SHUTDOWN_GRACE;
        let mut left = sessions.len();

        while left > 0 && Instant::now() < deadline {
            // the terminal is not watched here: the example is leaving
            wait(ci, false);

            if let Err(e) = ci.tick(Instant::now()) {
                eprintln!("camenu: {e}");
                break;
            }

            while let Ok(Some(event)) = ci.poll_event() {
                if let CaEvent::SessionClosed { session_id, .. } = event
                    && sessions.iter().any(|&(_, open)| open == session_id)
                {
                    left = left.saturating_sub(1);
                }
            }
        }
    }
}

/// Asks the CAM to open its menu. The request travels on the Application
/// Information session, so it is only valid once that session has confirmed
/// the application info.
fn enter_menu(ci: &mut CiController, slot_id: u8) {
    match ci.cam_status(slot_id) {
        Ok(CamStatus::ApplicationInfo | CamStatus::Ready) => {
            println!("CI slot {slot_id}: entering the CAM menu");
            log_error(ci.enter_menu(slot_id));
        }
        Ok(status) => println!("CI slot {slot_id}: the CAM is not ready yet ({status:?})"),
        Err(e) => eprintln!("camenu: {e}"),
    }
}

/// `info`: where every slot of the adapter stands
fn info_command(ci: &CiController) {
    for slot_id in 0 .. ci.slots_num() {
        match (ci.status(slot_id), ci.cam_status(slot_id)) {
            (Ok(status), Ok(cam_status)) => {
                println!("CI slot {slot_id}: {status:?}, CAM {cam_status:?}");
            }
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("camenu: slot {slot_id}: {e}");
                continue;
            }
        }

        if let Some(info) = ci.app_info(slot_id) {
            print_app_info(slot_id, info);
        }

        match ci.caids(slot_id) {
            Ok(caids) if !caids.is_empty() => println!("    CAIDs: {caids:04X?}"),
            Ok(_) => {}
            Err(e) => eprintln!("camenu: slot {slot_id}: {e}"),
        }
    }
}

/// The highest item number a menu offers, as `mmi_menu_answer` takes it
fn item_count(menu: &MmiMenu) -> u8 {
    match u8::try_from(menu.items.len()) {
        Ok(items) => items,
        // menu_answ carries one byte: a module cannot be answered past 255
        Err(_) => {
            println!(
                "    note: the menu has {} items, only 255 can be answered",
                menu.items.len()
            );
            u8::MAX
        }
    }
}

/// Prints DVB text under a fixed indent, keeping the line breaks the module
/// put in it
fn print_text(indent: &str, data: &[u8]) {
    let decoded = textcode::dvb::decode(data);

    if decoded.is_empty() {
        return;
    }

    for line in decoded.split('\n') {
        println!("{indent}{line}");
    }
}

fn print_app_info(slot_id: u8, info: &ApplicationInfo) {
    println!("CI slot {slot_id}: CAM application info");
    println!("    application type: 0x{:02X}", info.application_type);
    println!("    vendor: 0x{:04X}", info.application_manufacturer);
    println!("    manufacturer code: 0x{:04X}", info.manufacturer_code);
    println!("    menu string: {}", textcode::dvb::decode(&info.menu_string));
}

fn print_menu(kind: &str, slot_id: u8, session_id: u16, menu: &MmiMenu) {
    println!("CI slot {slot_id}, MMI session {session_id}: {kind}");

    for (label, field) in [
        ("title", &menu.title),
        ("sub-title", &menu.sub_title),
        ("bottom", &menu.bottom),
    ] {
        if !field.is_empty() {
            println!("    {label}:");
            print_text("        ", field);
        }
    }

    for (index, item) in menu.items.iter().enumerate() {
        let decoded = textcode::dvb::decode(item);
        let mut lines = decoded.split('\n');
        println!("    {:2}. {}", index + 1, lines.next().unwrap_or_default());

        for line in lines {
            println!("        {line}");
        }
    }
}

/// A failed command is reported and the loop goes on: the CAM may have taken
/// the session down while the user was typing
fn log_error(result: libdvb::error::Result<()>) {
    if let Err(e) = result {
        eprintln!("camenu: {e}");
    }
}

/// Waits for CI data, for user input or for the tick deadline; reports whether
/// stdin has something to read
fn wait(ci: &CiController, watch_stdin: bool) -> bool {
    let stdin = io::stdin();
    let mut fds = Vec::with_capacity(2);
    fds.push(PollFd::new(ci.as_fd(), PollFlags::POLLIN));

    if watch_stdin {
        fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));
    }

    match poll(&mut fds, PollTimeout::from(TICK_MS)) {
        // a signal is a wake-up like any other; the caller checks the flag
        Ok(_) | Err(Errno::EINTR) => {}
        Err(e) => {
            eprintln!("camenu: poll: {e}");
            // a broken poll must not turn the loop into a spin
            std::thread::sleep(TICK_INTERVAL);
            return false;
        }
    }

    // POLLHUP counts as readable: the read that follows reports the end of
    // the input
    fds.get(1).and_then(PollFd::any).unwrap_or(false)
}

struct Args {
    adapter: u32,
    device: u32,
}

/// Reads the command line; `None` means the usage was asked for and printed
fn parse_args() -> Result<Option<Args>, String> {
    parse_arguments(std::env::args().skip(1))
}

fn parse_arguments<I>(arguments: I) -> Result<Option<Args>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut numbers = Vec::new();

    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            option if option.starts_with('-') => {
                return Err(format!("unknown option {option:?}\n\n{USAGE}"));
            }
            number => match number.parse::<u32>() {
                Ok(number) if numbers.len() < 2 => numbers.push(number),
                Ok(_) => return Err(format!("too many arguments\n\n{USAGE}")),
                Err(_) => return Err(format!("{number:?} is not a device number\n\n{USAGE}")),
            },
        }
    }

    match numbers.as_slice() {
        [adapter] => Ok(Some(Args {
            adapter: *adapter,
            device: 0,
        })),
        [adapter, device] => Ok(Some(Args {
            adapter: *adapter,
            device: *device,
        })),
        _ => Err(format!("the adapter number is required\n\n{USAGE}")),
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    install_signal_handlers()?;

    let (adapter, device) = (args.adapter, args.device);
    let mut ci = CiController::open(adapter, device)
        .map_err(|e| format!("/dev/dvb/adapter{adapter}/ca{device}: {e}"))?;
    let slots_num = ci.slots_num();
    println!("CI adapter {adapter}, device {device}: {slots_num} slot(s)");
    println!("The CAM menu opens by itself; \"help\" lists the commands");

    let mut app = App::new(slots_num);
    let mut reader = LineReader::new();
    let mut leave = false;

    while !leave && !SHUTDOWN.load(Ordering::Relaxed) {
        let readable = wait(&ci, !reader.at_eof());

        if let Err(e) = ci.tick(Instant::now()) {
            eprintln!("camenu: tick: {e}");
        }

        loop {
            match ci.poll_event() {
                Ok(Some(event)) => app.handle_event(&mut ci, event),
                Ok(None) => break,
                Err(e) => {
                    eprintln!("camenu: {e}");
                    break;
                }
            }
        }

        if readable {
            for line in reader.read_lines() {
                if !app.handle_line(&mut ci, line.trim()) {
                    leave = true;
                    break;
                }
            }
        }

        // stdout is not a terminal when the output is redirected: the report
        // must not sit in the buffer while the user waits for it
        let _ = io::stdout().flush();
    }

    if !leave {
        // the line the shell left after ^C
        println!();
    }

    app.shutdown(&mut ci);

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("camenu: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(arguments: &[&str]) -> Result<Option<Args>, String> {
        parse_arguments(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn adapter_alone_defaults_the_device_to_zero() {
        let parsed = args(&["1"]).unwrap().unwrap();
        assert_eq!((parsed.adapter, parsed.device), (1, 0));
    }

    #[test]
    fn adapter_and_device_are_both_read() {
        let parsed = args(&["2", "3"]).unwrap().unwrap();
        assert_eq!((parsed.adapter, parsed.device), (2, 3));
    }

    #[test]
    fn help_asks_for_no_device() {
        assert!(args(&["--help"]).unwrap().is_none());
        assert!(args(&["-h"]).unwrap().is_none());
    }

    #[test]
    fn a_missing_adapter_is_an_error() {
        assert!(args(&[]).is_err());
    }

    #[test]
    fn a_bad_argument_is_an_error() {
        assert!(args(&["ca0"]).is_err());
        assert!(args(&["-x"]).is_err());
        assert!(args(&["0", "1", "2"]).is_err());
    }

    #[test]
    fn an_empty_enquiry_answer_cancels() {
        assert_eq!(check_enq_answer("", 4), EnqAnswer::Cancel);
        assert_eq!(check_enq_answer("", 0), EnqAnswer::Cancel);
    }

    #[test]
    fn an_enquiry_answer_of_the_expected_length_is_sent() {
        assert_eq!(check_enq_answer("1234", 4), EnqAnswer::Send("1234"));
    }

    #[test]
    fn a_short_or_long_enquiry_answer_is_rejected() {
        // a PIN of the wrong length would cost the user one of the attempts
        // the CAM allows
        assert!(matches!(check_enq_answer("123", 4), EnqAnswer::Reject(_)));
        assert!(matches!(check_enq_answer("12345", 4), EnqAnswer::Reject(_)));
    }

    #[test]
    fn an_open_length_accepts_any_answer() {
        assert_eq!(check_enq_answer("anything", 0), EnqAnswer::Send("anything"));
    }

    #[test]
    fn a_non_ascii_enquiry_answer_is_rejected() {
        // one character, two bytes: the module would count two
        assert!(matches!(check_enq_answer("é", 1), EnqAnswer::Reject(_)));
        assert!(matches!(check_enq_answer("код", 0), EnqAnswer::Reject(_)));
    }

    #[test]
    fn commands_are_parsed_from_their_keyword() {
        assert_eq!(parse_command(""), Command::Nothing);
        assert_eq!(parse_command("q"), Command::Quit);
        assert_eq!(parse_command("quit"), Command::Quit);
        assert_eq!(parse_command("exit"), Command::Quit);
        assert_eq!(parse_command("menu"), Command::Menu(None));
        assert_eq!(parse_command("menu 1"), Command::Menu(Some("1")));
        assert_eq!(parse_command("slot 1"), Command::Focus(Some("1")));
        assert_eq!(parse_command("close"), Command::Close);
        assert_eq!(parse_command("info"), Command::Info);
        assert_eq!(parse_command("reset"), Command::Reset);
        assert_eq!(parse_command("help"), Command::Help);
        assert_eq!(parse_command("?"), Command::Help);
        assert_eq!(parse_command("1"), Command::Unknown("1"));
    }

    #[test]
    fn a_keyword_typed_at_a_menu_is_a_command() {
        // a menu takes numbers only, so a keyword cannot be an answer: the
        // user stays able to reach info, close and quit from inside a dialogue
        assert!(is_command("info"));
        assert!(is_command("close"));
        assert!(is_command("q"));
        assert!(is_command("menu 1"));

        // an answer, a typo and an empty line are not commands
        assert!(!is_command("1"));
        assert!(!is_command("inf"));
        assert!(!is_command(""));
    }

    #[test]
    fn menu_answers_stay_inside_the_offered_items() {
        assert_eq!(check_menu_answer("0", 3), Some(0));
        assert_eq!(check_menu_answer("3", 3), Some(3));
        assert_eq!(check_menu_answer("4", 3), None);
        assert_eq!(check_menu_answer("300", 3), None);
        assert_eq!(check_menu_answer("-1", 3), None);
        assert_eq!(check_menu_answer("two", 3), None);
        assert_eq!(check_menu_answer("", 3), None);
    }

    #[test]
    fn the_line_reader_splits_on_newlines_only() {
        let mut reader = LineReader::new();
        assert!(reader.split(b"menu").is_empty());
        assert_eq!(reader.split(b"\n1234\n"), vec!["menu", "1234"]);
    }

    #[test]
    fn the_line_reader_drops_the_carriage_return() {
        let mut reader = LineReader::new();
        assert_eq!(reader.split(b"menu\r\n"), vec!["menu"]);
    }

    #[test]
    fn the_line_reader_keeps_an_empty_line() {
        // an empty line cancels an enquiry and closes a list
        let mut reader = LineReader::new();
        assert_eq!(reader.split(b"\n"), vec![""]);
    }

    #[test]
    fn the_line_reader_discards_an_overlong_line() {
        let mut reader = LineReader::new();
        let long = vec![b'x'; LINE_LIMIT + 10];
        assert!(reader.split(&long).is_empty());
        // the overlong line is dropped, the next one reads as usual
        assert!(reader.split(b"\n").is_empty());
        assert_eq!(reader.split(b"menu\n"), vec!["menu"]);
    }

    #[test]
    fn the_line_reader_replaces_invalid_utf8() {
        let mut reader = LineReader::new();
        assert_eq!(reader.split(b"a\xFFb\n"), vec!["a\u{FFFD}b"]);
    }

    #[test]
    fn a_dialogue_reports_the_session_it_belongs_to() {
        assert_eq!(Pending::None.session_id(), None);
        assert!(!Pending::None.is_open());

        let enq = Pending::Enq {
            session_id: 7,
            blind: true,
            answer_len: 4,
        };
        assert_eq!(enq.session_id(), Some(7));
        assert!(enq.is_open());
        assert!(enq.is_blind());
        assert!(!Pending::List { session_id: 7 }.is_blind());
    }

    #[test]
    fn every_open_dialogue_has_a_prompt() {
        assert!(Pending::None.prompt().is_none());
        assert!(
            Pending::Menu {
                session_id: 1,
                items: 3,
            }
            .prompt()
            .is_some_and(|prompt| prompt.contains('3'))
        );
        assert!(
            Pending::Enq {
                session_id: 1,
                blind: true,
                answer_len: 4,
            }
            .prompt()
            .is_some_and(|prompt| prompt.contains('4'))
        );
        assert!(Pending::List { session_id: 1 }.prompt().is_some());
    }

    #[test]
    fn the_focus_follows_the_slot_with_a_dialogue() {
        let mut app = App::new(2);
        assert!(app.focused().is_none());

        // the first dialogue is answered right away
        app.set_dialogue(1, Pending::List { session_id: 5 });
        assert_eq!(app.focused().map(|(slot_id, _)| slot_id), Some(1));

        // a second slot waits its turn instead of stealing the answer
        app.set_dialogue(
            0,
            Pending::Menu {
                session_id: 6,
                items: 2,
            },
        );
        assert_eq!(app.focused().map(|(slot_id, _)| slot_id), Some(1));

        // closing the answered one hands the focus over
        app.set_dialogue(1, Pending::None);
        assert_eq!(app.focused().map(|(slot_id, _)| slot_id), Some(0));

        // and with nothing open the focus is gone
        app.set_dialogue(0, Pending::None);
        assert!(app.focused().is_none());
    }

    #[test]
    fn a_dialogue_of_an_unreported_slot_is_ignored() {
        let mut app = App::new(1);
        app.set_dialogue(7, Pending::List { session_id: 1 });
        assert!(app.focused().is_none());
    }

    #[test]
    fn only_the_matching_session_clears_the_dialogue() {
        let mut app = App::new(1);
        app.set_dialogue(0, Pending::List { session_id: 5 });

        // a stale close of another session leaves the dialogue alone
        app.clear_session(0, 6);
        assert!(app.focused().is_some());

        app.clear_session(0, 5);
        assert!(app.focused().is_none());
    }

    #[test]
    fn a_lost_cam_forgets_the_slot() {
        let mut app = App::new(1);
        app.mmi_sessions.push((0, 5));
        app.menu_requested[0] = true;
        app.set_dialogue(0, Pending::List { session_id: 5 });

        app.forget_slot(0);

        assert!(app.focused().is_none());
        assert!(app.mmi_sessions.is_empty());
        // the menu of the next CAM opens by itself again
        assert!(!app.menu_requested[0]);
    }
}
