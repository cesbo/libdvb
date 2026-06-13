use {
    std::{
        time::Duration,
        thread,
    },

    libdvb::{
        FeDevice,
        FeStatus,
    },
};


fn main()  {
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
    let mut status = FeStatus::default();

    let delay = Duration::from_secs(1);
    loop {
        status.read(&fe).unwrap();
        println!("{}", &status);
        thread::sleep(delay);
    }
}
