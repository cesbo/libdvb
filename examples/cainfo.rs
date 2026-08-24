//! Report the CAM of a CI adapter and exit
//!
//! Usage: cainfo [-h] <adapter> [device]
//!
//! Opens /dev/dvb/adapterN/caM, prints the CA device capabilities, brings
//! the en50221 stack up and waits for the inserted CAMs to identify
//! themselves. Everything the host learns is printed - the slot and CAM
//! states, the application info (vendor, manufacturer code, menu string)
//! and the CA system ids each module supports - and the example leaves.
//!
//! Note that bringing the stack up resets the CI interface of the adapter
//! (`CiController::new` issues CA_RESET), so every CAM of the adapter
//! restarts and whatever it was descrambling stops for the duration.

use std::{
    error::Error,
    os::fd::AsFd,
    process::ExitCode,
    time::{
        Duration,
        Instant,
    },
};

use libdvb::{
    CaDevice,
    CaSlotStatus,
    CamStatus,
    CiController,
    ca::sys::{
        CA_CI,
        CA_CI_LINK,
        CA_CI_PHYS,
        CA_DESCR,
        CA_DSS,
        CA_ECD,
        CA_NDS,
        CA_SC,
    },
};
use nix::{
    errno::Errno,
    poll::{
        PollFd,
        PollFlags,
        PollTimeout,
        poll,
    },
};

/// How long the loop waits before driving the controller again. The default
/// `CiControllerConfig::transport_poll_interval` is the shortest period the
/// controller needs a tick at; CI data arrives as a wake-up rather than at
/// this rate.
const TICK_MS: u16 = 100;

/// How long a slot may stay unreported before it counts as empty: the
/// controller reads the physical slot state within its first
/// `CiControllerConfig::slot_status_interval`
const ABSENT_GRACE: Duration = Duration::from_secs(2);

/// How long a module may take to identify itself after the reset before the
/// report goes out without it; a CAM regularly takes 10-20 seconds to boot
const IDENTIFY_LIMIT: Duration = Duration::from_secs(30);

const USAGE: &str = "\
Usage: cainfo [OPTIONS] <adapter> [device]

Reports the CAM of a DVB CI adapter (/dev/dvb/adapterN/caM): the device
capabilities, the application info and the CA systems each inserted module
supports. Opening the adapter resets its CI interface: every CAM restarts
and stops descrambling while cainfo runs.

Options:
    -h, --help    print this text and exit
";

struct Args {
    adapter: u32,
    device: u32,
}

/// Reads the command line; `None` means the usage was asked for and printed
fn parse_args() -> Result<Option<Args>, String> {
    let mut numbers = Vec::new();

    for argument in std::env::args().skip(1) {
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

/// Names the ca_slot_type bits of CA_GET_CAP
fn slot_type_names(slot_type: u32) -> String {
    bit_names(
        slot_type,
        &[
            (CA_CI, "CI high level"),
            (CA_CI_LINK, "CI link layer"),
            (CA_CI_PHYS, "CI physical layer"),
            (CA_DESCR, "built-in descrambler"),
            (CA_SC, "smart card"),
        ],
    )
}

/// Names the ca_descr_type bits of CA_GET_CAP
fn descr_type_names(descr_type: u32) -> String {
    bit_names(
        descr_type,
        &[(CA_ECD, "ECD"), (CA_NDS, "NDS"), (CA_DSS, "DSS")],
    )
}

fn bit_names(value: u32, names: &[(u32, &str)]) -> String {
    let mut text = String::new();
    let mut rest = value;

    for &(bit, name) in names {
        if value & bit != 0 {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(name);
            rest &= !bit;
        }
    }

    if rest != 0 || text.is_empty() {
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(&format!("0x{rest:X}"));
    }

    text
}

/// en50221 Table 61
fn application_type_name(application_type: u8) -> &'static str {
    match application_type {
        0x01 => " (Conditional Access)",
        0x02 => " (Electronic Programme Guide)",
        _ => "",
    }
}

/// The capabilities the CA device reports before the stack comes up
fn print_device(device: &CaDevice) -> Result<(), Box<dyn Error>> {
    println!(
        "CA adapter {}, device {}",
        device.adapter(),
        device.device()
    );

    if let (Some(vendor_id), Some(device_id)) = (device.vendor_id(), device.device_id()) {
        println!("    PCI id: {vendor_id:04X}:{device_id:04X}");
    }

    let caps = device.caps()?;
    println!(
        "    module slots: {} ({})",
        caps.slot_num,
        slot_type_names(caps.slot_type)
    );

    if caps.descr_num != 0 {
        println!(
            "    descramblers: {} ({})",
            caps.descr_num,
            descr_type_names(caps.descr_type)
        );
    }

    Ok(())
}

/// Whether every slot has told all it will: an identified CAM is `Ready`,
/// and a slot still `Absent` after the grace period has no module in it
fn all_settled(ci: &CiController, elapsed: Duration) -> bool {
    (0 .. ci.slots_num()).all(
        |slot_id| match (ci.status(slot_id), ci.cam_status(slot_id)) {
            (Ok(CaSlotStatus::Absent), _) => elapsed >= ABSENT_GRACE,
            (_, Ok(CamStatus::Ready)) => true,
            _ => false,
        },
    )
}

/// Waits for CI data or for the tick deadline
fn wait(ci: &CiController) {
    let mut fds = [PollFd::new(ci.as_fd(), PollFlags::POLLIN)];

    match poll(&mut fds, PollTimeout::from(TICK_MS)) {
        Ok(_) | Err(Errno::EINTR) => {}
        Err(e) => {
            eprintln!("cainfo: poll: {e}");
            // a broken poll must not turn the loop into a spin
            std::thread::sleep(Duration::from_millis(u64::from(TICK_MS)));
        }
    }
}

/// Drives the controller until every slot settles or the deadline passes.
/// The events are not the report - the final state queried afterwards is -
/// but the queue has to be drained for the controller to make progress.
fn collect(ci: &mut CiController) {
    let started = Instant::now();

    loop {
        wait(ci);

        if let Err(e) = ci.tick(Instant::now()) {
            eprintln!("cainfo: tick: {e}");
        }

        loop {
            match ci.poll_event() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => {
                    eprintln!("cainfo: {e}");
                    break;
                }
            }
        }

        let elapsed = started.elapsed();

        if all_settled(ci, elapsed) {
            return;
        }

        if elapsed >= IDENTIFY_LIMIT {
            eprintln!(
                "cainfo: not every module identified itself within {}s; \
                 reporting what arrived",
                IDENTIFY_LIMIT.as_secs()
            );
            return;
        }
    }
}

/// Everything the stack has learned about the slots and their modules
fn report(ci: &CiController) {
    for slot_id in 0 .. ci.slots_num() {
        let status = match ci.status(slot_id) {
            Ok(status) => status,
            Err(e) => {
                eprintln!("cainfo: slot {slot_id}: {e}");
                continue;
            }
        };

        if status == CaSlotStatus::Absent {
            println!("CI slot {slot_id}: no module inserted");
            continue;
        }

        let cam_status = ci.cam_status(slot_id).unwrap_or(CamStatus::None);
        println!("CI slot {slot_id}: {status:?}, CAM {cam_status:?}");

        match ci.app_info(slot_id) {
            Some(info) => {
                println!(
                    "    menu string: {}",
                    textcode::dvb::decode(&info.menu_string)
                );
                println!(
                    "    application type: 0x{:02X}{}",
                    info.application_type,
                    application_type_name(info.application_type)
                );
                println!("    manufacturer: 0x{:04X}", info.application_manufacturer);
                println!("    manufacturer code: 0x{:04X}", info.manufacturer_code);
            }
            None => println!("    the module did not report its application info"),
        }

        match ci.caids(slot_id) {
            Ok(caids) if caids.is_empty() => println!("    CA system ids: none reported"),
            Ok(caids) => println!("    CA system ids: {caids:04X?}"),
            Err(e) => eprintln!("cainfo: slot {slot_id}: {e}"),
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    let (adapter, device) = (args.adapter, args.device);
    let device = CaDevice::open(adapter, device)
        .map_err(|e| format!("/dev/dvb/adapter{adapter}/ca{device}: {e}"))?;

    // printed before the controller takes the device: a CA device without
    // the CI link interface still gets its capabilities reported
    print_device(&device)?;

    let mut ci = CiController::new(device)?;
    println!(
        "The CI interface resets; waiting up to {}s for the modules to identify themselves...",
        IDENTIFY_LIMIT.as_secs()
    );

    collect(&mut ci);
    report(&ci);

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cainfo: {e}");
            ExitCode::FAILURE
        }
    }
}
