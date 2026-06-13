use {
    libdvb::NetDevice,
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

    let dev = NetDevice::open(adapter, device).unwrap();

    let interface = dev.add_if(0, libdvb::net::sys::DVB_NET_FEEDTYPE_MPE).unwrap();
    println!("Interface: {}", &interface);
    let mac = interface.get_mac();
    println!("MAC: {}", &mac);
    dev.remove_if(interface).unwrap();
}
