use {
    std::{
        path::Path,
    },

    anyhow::Result,

    libdvb::{
        CaDevice,
    },
};


fn check_ca(path: &Path) -> Result<()> {
    println!("CA: {}", path.display());

    let mut ca = CaDevice::open(path, 0)?;

    // loop for about 3s
    for _ in 0 .. 30 {
        ca.poll()?;
    }

    Ok(())
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(path) = args.next() {
        let path = Path::new(&path);
        check_ca(&path)?;
    } else {
        eprintln!("path to ca device is not defined");
    }

    Ok(())
}
