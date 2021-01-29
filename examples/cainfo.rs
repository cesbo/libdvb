use {
    std::{
        path::Path,
    },

    anyhow::{
        Context,
        Result,
    },

    libdvb::{
        CaDevice,
    },
};


fn check_ca(path: &Path) -> Result<()> {
    println!("CA: {}", path.display());

    let ca = CaDevice::open(path, 0)?;

    Ok(())
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(path) = args.next() {
        let path = Path::new(path);
        check_ca(&path)?;
    } else {
        eprintln!("path to ca device is not defined");
    }

    Ok(())
}
