use {
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
    println!("{}", &fe);

    let mut status = FeStatus::default();
    status.read(&fe)?;
    println!("Status: {}", &status);

    Ok(())
}
