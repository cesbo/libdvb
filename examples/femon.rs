use std::{
    thread,
    time::Duration,
};

use libdvb::FeDevice;

fn main() {
    let mut args = std::env::args().skip(1);

    let adapter = match args.next() {
        Some(v) => v.parse().unwrap(),
        None => 0,
    };

    let device = match args.next() {
        Some(v) => v.parse().unwrap(),
        None => 0,
    };

    let fe = FeDevice::open_ro(adapter, device).unwrap();

    let delay = Duration::from_secs(1);
    loop {
        let stats = fe.get_stats().unwrap();
        println!("{}", stats.to_status_string());
        thread::sleep(delay);
    }
}
