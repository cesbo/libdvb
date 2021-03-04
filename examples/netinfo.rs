use {
    std::{
        path::Path,
    },

    anyhow::Result,

    libdvb::NetDevice,
};


fn check_net(path: &Path) -> Result<()> {
    println!("NET: {}", path.display());

    let dev = NetDevice::open(path)?;

    let mut info = libdvb::net::sys::DvbNetIf {
        pid: 0,
        if_num: 0,
        feedtype: libdvb::net::sys::DVB_NET_FEEDTYPE_MPE,
    };

    dev.add_if(&mut info)?;
    println!("Interface: {}", dev.get_name());

    let mac = match dev.get_mac() {
        Ok(v) => v,
        Err(e) => e.to_string(),
    };
    println!("MAC: {}", mac);

    dev.remove_if(&info)?;

    Ok(())
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(path) = args.next() {
        let path = Path::new(&path);
        check_net(&path)?;
    } else {
        eprintln!("path to ca device is not defined");
    }

    Ok(())
}
