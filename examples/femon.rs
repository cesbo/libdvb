use {
    std::{
        path::Path,
        time::Duration,
        thread,
    },

    anyhow::{
        anyhow,
        Result,
    },

    libdvb::{
        FeDevice,
        FeStatus,
    },
};


pub fn start(fepath: &str) -> Result<()> {
    let fepath = Path::new(fepath);
    println!("Frontend: {}", fepath.display());

    let fe = FeDevice::open_rd(fepath)?;
    let mut status = FeStatus::default();

    let delay = Duration::from_secs(1);

    loop {
        status.read(&fe)?;
        println!("{}", &status.display(0));
        thread::sleep(delay);
    }
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(ref fepath) = args.next() {
        start(fepath)
    } else {
        Err(anyhow!("Path to frontend not defined"))
    }
}
