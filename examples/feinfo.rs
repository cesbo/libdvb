use {
    libdvb::{
        FeDevice,
        FeStatus,
    },
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
    println!("{}", &fe);

    let mut status = FeStatus::default();
    status.read(&fe).unwrap();
    println!("Status: {}", &status);
}
