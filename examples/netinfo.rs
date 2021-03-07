use {
    anyhow::{
        bail,
        Context,
        Result,
    },

    libdvb::NetDevice,
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

    let dev = NetDevice::open(adapter, device)?;

    let interface = dev.add_if(0, libdvb::net::sys::DVB_NET_FEEDTYPE_MPE)?;
    println!("Interface: {}", &interface);
    let mac = interface.get_mac();
    println!("MAC: {}", &mac);
    dev.remove_if(interface)?;

    Ok(())
}
