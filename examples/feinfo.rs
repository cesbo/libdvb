use libdvb::{
    FeDevice,
    FeStatus,
};

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
    println!("DVB API: {}", fe.api_version());
    println!("Frontend: {}", fe.name());

    print!("Delivery system:");
    for v in fe.delivery_systems() {
        print!(" {}", v);
    }
    println!();

    println!("Frequency range: {:?}", fe.frequency_range());
    println!("Symbolrate range: {:?}", fe.symbolrate_range());

    println!("Frontend capabilities: {:?}", fe.caps());

    let mut status = FeStatus::default();
    status.read(&fe).unwrap();
    println!("Status: {}", status.to_status_string());
}
