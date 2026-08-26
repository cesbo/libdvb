//! Interactive access to the CAM menu of a CI adapter
//!
//! Usage: camenu [adapter] [device]  (both default to 0)
//!
//! Opens /dev/dvb/adapterN/caM and asks the inserted CAM to open its menu
//! once the module identifies itself. The menu is printed with numbered
//! items and the example waits on the terminal:
//!
//! - an item number answers the open menu, 0 cancels it
//! - any text answers an open enquiry, an empty line cancels it
//! - an empty line closes an open list
//! - q leaves
//!
//! The controller is driven from a plain sleep loop - `tick()` and
//! `poll_event()` never block - and the terminal is read from a thread, so
//! the loop never waits for the user. Watching the CA descriptor (`as_fd`)
//! with poll(2) instead would only cut the latency.
//!
//! Note that opening the controller resets the CI interface of the adapter
//! (`CiController::open` issues CA_RESET), so every CAM of the adapter
//! restarts and whatever it was descrambling stops for the duration.

use std::{
    error::Error,
    io::{
        self,
        BufRead,
        Write,
    },
    sync::mpsc,
    thread,
    time::{
        Duration,
        Instant,
    },
};

use libdvb::{
    CaEvent,
    CaSlotStatus,
    CiController,
    ca::{
        DvbText,
        MmiMenu,
        ResourceId,
    },
};

/// How long the loop sleeps between the controller ticks. `tick()` and
/// `poll_event()` never block, so a plain sleep loop is enough; watching the
/// CA descriptor (`as_fd`) with poll(2) instead would only cut the latency.
const TICK: Duration = Duration::from_millis(50);

/// What the open dialogue expects as its answer
#[derive(Clone, Copy)]
enum DialogueKind {
    /// An item number; 0 cancels the menu
    Menu,
    /// An empty line, closing the list
    List,
    /// A line of text; an empty line cancels the enquiry
    Enq,
}

/// MMI dialogue a module is waiting for an answer to. The example answers
/// one dialogue at a time - the most recent one; the session it belongs to
/// has to be answered exactly, as a module may take it down and open the
/// next one at any moment.
#[derive(Clone, Copy)]
struct Dialogue {
    slot_id: u8,
    session_id: u16,
    kind: DialogueKind,
}

struct App {
    ci: CiController,
    /// The dialogue the next line answers; the module replaces it with the
    /// next MMI object as the user walks the menu
    dialogue: Option<Dialogue>,
    /// Slots whose CAM has already been asked to open its menu, by slot id.
    /// A CAM may repeat its application info, and a second `enter_menu`
    /// would throw away the dialogue the user is in the middle of.
    menu_requested: Vec<bool>,
    /// MMI sessions the modules opened, closed politely on the way out
    mmi_sessions: Vec<(u8, u16)>,
}

impl App {
    fn new(ci: CiController) -> Self {
        App {
            menu_requested: vec![false; usize::from(ci.slots_num())],
            ci,
            dialogue: None,
            mmi_sessions: Vec::new(),
        }
    }

    /// Asks the CAM to open its menu, once per CAM. The request travels on
    /// the Application Information session, so right after the application
    /// info is the earliest moment it is valid.
    fn enter_menu_once(&mut self, slot_id: u8) {
        if let Some(requested) = self.menu_requested.get_mut(usize::from(slot_id))
            && !*requested
        {
            *requested = true;
            println!("CI slot {slot_id}: entering the CAM menu");
            log_error(self.ci.enter_menu(slot_id));
        }
    }

    /// Drops the dialogue of one session, if it is the one being answered
    fn drop_dialogue(&mut self, slot_id: u8, session_id: u16) {
        if self
            .dialogue
            .is_some_and(|open| (open.slot_id, open.session_id) == (slot_id, session_id))
        {
            self.dialogue = None;
        }
    }

    /// Forgets the state of a slot whose CAM went away, so that the menu of
    /// the next CAM is opened again
    fn forget_slot(&mut self, slot_id: u8) {
        if self.dialogue.is_some_and(|open| open.slot_id == slot_id) {
            self.dialogue = None;
        }

        self.mmi_sessions.retain(|&(slot, _)| slot != slot_id);

        if let Some(requested) = self.menu_requested.get_mut(usize::from(slot_id)) {
            *requested = false;
        }
    }

    fn handle_event(&mut self, event: CaEvent) {
        match event {
            CaEvent::SlotStatusChanged { slot_id, new, .. } => {
                println!("CI slot {slot_id}: {new:?}");
                // the transport is gone: after a recovery or a module change
                // the application info arrives again and the menu reopens
                if new != CaSlotStatus::Active {
                    self.forget_slot(slot_id);
                }
            }
            CaEvent::ApplicationInfo { slot_id, info } => {
                println!("CI slot {slot_id}: CAM {:?}", info.menu_string);
                self.enter_menu_once(slot_id);
            }
            CaEvent::SessionOpened {
                slot_id,
                session_id,
                resource_id,
            } => {
                if resource_id == ResourceId::MMI {
                    self.mmi_sessions.push((slot_id, session_id));
                }
            }
            CaEvent::SessionClosed {
                slot_id,
                session_id,
                ..
            } => {
                // the dialogue also ends when the module drops the session
                // without a close_mmi: the pending answer must not outlive it
                self.mmi_sessions
                    .retain(|&session| session != (slot_id, session_id));
                self.drop_dialogue(slot_id, session_id);
            }
            CaEvent::MmiMenu {
                slot_id,
                session_id,
                menu,
            } => {
                print_menu("menu", slot_id, &menu);
                println!(
                    "    -> enter an item number from 0 to {}, 0 cancels; q leaves",
                    menu.items.len()
                );
                self.dialogue = Some(Dialogue {
                    slot_id,
                    session_id,
                    kind: DialogueKind::Menu,
                });
            }
            CaEvent::MmiList {
                slot_id,
                session_id,
                menu,
            } => {
                print_menu("list", slot_id, &menu);
                println!("    -> press Enter when done");
                self.dialogue = Some(Dialogue {
                    slot_id,
                    session_id,
                    kind: DialogueKind::List,
                });
            }
            CaEvent::MmiEnq {
                slot_id,
                session_id,
                blind,
                answer_len,
                text,
            } => {
                println!("CI slot {slot_id}: enquiry");
                print_text("    ", &text);

                match answer_len {
                    0 => println!("    -> answer, an empty line cancels"),
                    len => println!("    -> answer ({len} characters), an empty line cancels"),
                }

                // there is no terminal handling here: a blind enquiry -
                // a PIN - is echoed like any other line
                if blind {
                    println!("    note: the answer will be visible on the screen");
                }

                self.dialogue = Some(Dialogue {
                    slot_id,
                    session_id,
                    kind: DialogueKind::Enq,
                });
            }
            CaEvent::MmiText { slot_id, text, .. } => {
                println!("CI slot {slot_id}: text");
                print_text("    ", &text);
            }
            CaEvent::MmiClose {
                slot_id,
                session_id,
                ..
            } => {
                // the session stack answers the close itself; nothing is
                // left to do but drop the dialogue
                self.drop_dialogue(slot_id, session_id);
            }
            // status details, CA info and host control are not the subject
            // of this example
            _ => {}
        }
    }

    /// Handles one line from the user; returns `false` to leave
    fn handle_line(&mut self, line: &str) -> bool {
        if matches!(line, "q" | "quit") {
            return false;
        }

        let Some(Dialogue {
            slot_id,
            session_id,
            kind,
        }) = self.dialogue
        else {
            if !line.is_empty() {
                println!("No dialogue is open; the CAM menu opens by itself, q leaves");
            }
            return true;
        };

        match kind {
            DialogueKind::Menu => match line.parse::<u8>() {
                Ok(choice) => {
                    log_error(self.ci.mmi_menu_answer(slot_id, session_id, choice));
                    // the module answers with the next object of the dialogue
                    self.dialogue = None;
                }
                Err(_) => println!("    -> enter an item number, 0 cancels; q leaves"),
            },
            DialogueKind::List => {
                if line.is_empty() {
                    log_error(self.ci.mmi_list_close(slot_id, session_id));
                    self.dialogue = None;
                } else {
                    println!("    -> press Enter to close the list");
                }
            }
            DialogueKind::Enq => {
                let answer = (!line.is_empty()).then(|| line.as_bytes());
                log_error(self.ci.mmi_answer(slot_id, session_id, answer));
                self.dialogue = None;
            }
        }

        true
    }

    /// Asks the modules to take the open dialogues down on the way out: a
    /// CAM left with a session open can refuse the next one. The transport
    /// writes the request out right away and the close needs no answer.
    fn close_mmi_sessions(&mut self) {
        for (slot_id, session_id) in std::mem::take(&mut self.mmi_sessions) {
            println!("CI slot {slot_id}: closing MMI session {session_id}");
            log_error(self.ci.mmi_close(slot_id, session_id));
        }
    }
}

/// Prints DVB text under a fixed indent, keeping the line breaks the module
/// put in it
fn print_text(indent: &str, text: &DvbText) {
    let decoded = text.to_string();

    if decoded.is_empty() {
        return;
    }

    for line in decoded.split('\n') {
        println!("{indent}{line}");
    }
}

fn print_menu(kind: &str, slot_id: u8, menu: &MmiMenu) {
    println!("CI slot {slot_id}: {kind}");

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
        let decoded = item.to_string();
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

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let adapter: u32 = args
        .next()
        .map(|v| v.parse().expect("invalid adapter value"))
        .unwrap_or(0);
    let device: u32 = args
        .next()
        .map(|v| v.parse().expect("invalid device value"))
        .unwrap_or(0);

    let ci = CiController::open(adapter, device)?;
    println!(
        "CI adapter {adapter}, device {device}: {} slot(s)",
        ci.slots_num()
    );
    println!("The CAM menu opens by itself once the CAM identifies itself; q leaves");

    // reading the terminal from a thread keeps the loop free to drive the
    // controller: a blocked stdin read must not stall the CI polling
    let (line_tx, lines) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut app = App::new(ci);

    'running: loop {
        if let Err(e) = app.ci.tick(Instant::now()) {
            eprintln!("camenu: tick: {e}");
        }

        loop {
            match app.ci.poll_event() {
                Ok(Some(event)) => app.handle_event(event),
                Ok(None) => break,
                Err(e) => {
                    eprintln!("camenu: {e}");
                    break;
                }
            }
        }

        loop {
            match lines.try_recv() {
                Ok(line) => {
                    if !app.handle_line(line.trim()) {
                        break 'running;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // stdin ended: nobody is left to answer the menu
                Err(mpsc::TryRecvError::Disconnected) => break 'running,
            }
        }

        // stdout is not a terminal when the output is redirected: the menu
        // must not sit in the buffer while the user waits for it
        let _ = io::stdout().flush();

        thread::sleep(TICK);
    }

    app.close_mmi_sessions();

    Ok(())
}
