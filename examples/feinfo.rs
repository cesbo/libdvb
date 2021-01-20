use {
    std::{
        path::Path,
    },

    anyhow::{
        Context,
        Result,
    },

    libdvb::{
        FeDevice,
        FeStatus,
    },
};


fn check_frontend(path: &Path) -> Result<()> {
    println!("Frontend: {}", path.display());

    let fe = FeDevice::open(path, false)?;
    println!("{}", &fe);

    let mut status = FeStatus::default();
    status.read(&fe)?;
    println!("{}", &status.display(1));

    Ok(())
}


fn list_devices(path: &Path) -> Result<()> {
    for entry in path.read_dir().context("list adapter directory")? {
        let entry = entry?;
        let device_path = entry.path();

        let file_name = device_path.file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default();

        if file_name.starts_with("frontend") {
            if let Err(e) = check_frontend(&device_path) {
                eprintln!("failed to get frontend info");
                for cause in e.chain() {
                    eprintln!("> {}", &cause);
                }
            }
            println!("");
        }
    }

    Ok(())
}


fn start(fepath: Option<&str>) -> Result<()> {
    if let Some(fepath) = fepath {
        let path = Path::new(fepath);
        check_frontend(&path)?;
    } else {
        let path = Path::new("/dev/dvb");

        for entry in path.read_dir().context("list dvb directory")? {
            let entry = entry?;
            let adapter_path = entry.path();
            if let Err(e) = list_devices(&adapter_path) {
                eprintln!("failed to list devices in {}\n{}", adapter_path.display(), e);
            }
        }
    }

    Ok(())
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    start(args.next().as_deref())
}
