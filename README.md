# libdvb

libdvb is an interface library for DVB API v5 devices in Linux.

Supports three types of delivery systems:

- Satellite: DVB-S, DVB-S2
- Terrestrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C

TODO:

- Cenelec EN 50221 - Common Interface Specification for Conditional Access and
  other Digital Video Broadcasting Decoder Applications
- DiSEqC 1.0
- DiSEqC 1.1
- EN 50494 - Unicable I
- EN 50607 - Unicable II

## FeDevice

Frontend tune commands use typed DVBv5 properties instead of raw `u32`
property/value pairs.

Example DVB-S2 tune:

```rust
use libdvb::{
    DtvProperty,
    FeDevice,
    fe::sys::{
        DeliverySystem,
        Fec,
        Inversion,
        Modulation,
        Pilot,
        Rolloff,
        SecTone,
        SecVoltage,
    },
};

let cmdseq = vec![
    DtvProperty::DeliverySystem(DeliverySystem::Dvbs2),
    DtvProperty::Frequency((11044 - 9750) * 1000),
    DtvProperty::Modulation(Modulation::Psk8),
    DtvProperty::Voltage(SecVoltage::V13),
    DtvProperty::Tone(SecTone::Off),
    DtvProperty::Inversion(Inversion::Auto),
    DtvProperty::SymbolRate(27500 * 1000),
    DtvProperty::InnerFec(Fec::Auto),
    DtvProperty::Pilot(Pilot::Auto),
    DtvProperty::Rolloff(Rolloff::R35),
    DtvProperty::Tune,
];

let fe = FeDevice::open_rw(0, 0)?;
fe.set_properties(&cmdseq)?;
```

Frontend information is available through explicit accessors:

```rust
let fe = FeDevice::open_ro(0, 0)?;
println!("DVB API: {}", fe.api_version());
println!("Frontend: {}", fe.name());

print!("Delivery system:");
for v in fe.delivery_systems() {
    print!(" {}", v);
}
println!();

println!("Frequency range: {:?}", fe.frequency_range());
println!("Symbolrate range: {:?}", fe.symbolrate_range());
println!("Frontend capabilities: {:?}", fe.caps());
```

Frontend status:

```rust
let fe = FeDevice::open_ro(0, 0)?;
let mut status = FeStatus::default();
status.read(&fe)?;
println!("{}", status.to_status_string());
```

`FeStatus` also exposes parsed values via methods such as
`delivery_system()`, `modulation()`, `signal_strength_decibel()`,
`signal_strength()`, `snr_decibel()`, `snr()`, `ber()`, and `unc()`.

## Demux

`DmxDevice` opens `/dev/dvb/adapterN/demuxM` and supports PES filters,
buffer sizing, and explicit start/stop:

```rust
use libdvb::dmx::{
    DmxDevice,
    sys::{
        DMX_IN_FRONTEND,
        DMX_OUT_TS_TAP,
        DMX_PES_OTHER,
        DmxFilterFlags,
        DmxPesFilterParams,
    },
};

let dmx = DmxDevice::open(0, 0)?;
let filter = DmxPesFilterParams {
    pid: 8192,
    input: DMX_IN_FRONTEND,
    output: DMX_OUT_TS_TAP,
    pes_type: DMX_PES_OTHER,
    flags: DmxFilterFlags::IMMEDIATE_START.bits(),
};

dmx.set_pes_filter(&filter)?;
```

## NetDevice

Network interfaces are removed automatically when `NetInterface` is dropped.
Use `mac()` to read the interface MAC address.

```rust
use libdvb::NetDevice;

let dev = NetDevice::open(0, 0)?;
let interface = dev.add_if(0, libdvb::net::sys::DVB_NET_FEEDTYPE_MPE)?;
println!("Interface: {}", interface);
println!("MAC: {}", interface.mac());
```

## File Descriptors

CA, demux, frontend, and network device handles implement `AsFd` and
`AsRawFd`, so they can be passed to APIs that operate on borrowed or raw file
descriptors.

## Code Formatting

```
rustfmt --config "group_imports=StdExternalCrate,imports_granularity=Crate,imports_layout=Vertical,newline_style=Unix,spaces_around_ranges=true,struct_lit_single_line=true,use_field_init_shorthand=true"
```
