//! Report the CAM of a CI adapter and exit
//!
//! Usage: cainfo [adapter] [device]  (both default to 0)
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
    thread::sleep,
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
};

/// How long the loop sleeps between the controller ticks. `tick()` and
/// `poll_event()` never block, so a plain sleep loop is enough; watching the
/// CA descriptor (`as_fd`) with poll(2) instead would only cut the latency.
const TICK: Duration = Duration::from_millis(50);

/// How long a slot may stay unreported before it counts as empty: the
/// controller reads the physical slot state within its first
/// `CiControllerConfig::slot_status_interval`
const ABSENT_GRACE: Duration = Duration::from_secs(2);

/// How long a module may take to identify itself after the reset before the
/// report goes out without it; a CAM regularly takes 10-20 seconds to boot
const IDENTIFY_LIMIT: Duration = Duration::from_secs(30);

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
        "    module slots: {}, {:?}",
        caps.slot_num,
        caps.slot_types()
    );

    if caps.descr_num != 0 {
        println!(
            "    descramblers: {}, {:?}",
            caps.descr_num,
            caps.descr_types()
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

/// Drives the controller until every slot settles or the deadline passes.
/// The events are not the report - the final state queried afterwards is -
/// but the queue has to be drained for the controller to make progress.
fn collect(ci: &mut CiController) {
    let started = Instant::now();

    loop {
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

        sleep(TICK);
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
                println!("    menu string: {}", info.menu_string);
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

    let device = CaDevice::open(adapter, device)?;

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
