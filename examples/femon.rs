use {
    std::{
        time::Duration,
        thread,
    },

    anyhow::{
        bail,
        Context,
        Result,
    },

    libdvb::{
        FeDevice,
        FeStatus,
    },
};


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let adapter = match args.next() {
        Some(v) => v.parse::<u32>().context("adapter number")?,
        None => bail!("adapter number not defined"),
    };

    let device = match args.next() {
        Some(v) => v.parse::<u32>().context("device number")?,
        None => 0,
    };

    let fe = FeDevice::open_ro(adapter, device)?;
    let mut status = FeStatus::default();

    let delay = Duration::from_secs(1);
    loop {
        status.read(&fe)?;
        println!("{}", &status);
        thread::sleep(delay);
    }
}
