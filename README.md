# libdvb

libdvb is an interface library for DVB API v5 devices in Linux.

Supports three types of delivery systems:

- Satellite: DVB-S, DVB-S2
- Terrestrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C
- DiSEqC 1.0
- DiSEqC 1.1
- EN 50494 - Unicable I
- EN 50607 - Unicable II

DVB-CI (EN 50221) support currently includes a runtime-neutral
`CiController`, the link, transport and session layers, and Resource
Manager, Application Information, Conditional Access Support, Host
Control, Date-Time and high-level MMI resources, including CA PMT program
selection from raw MPEG-TS PMT sections.

## FeDevice

Frontend tuning uses the high-level `TuneRequest` enum, which lowers
per-delivery-system parameters to a DVBv5 property command sequence.

Example DVB-S2 tune:

```rust
use libdvb::{
    DvbS2Tune,
    FeDevice,
    TuneRequest,
    fe::sys::{
        SecTone,
        SecVoltage,
    },
};

let fe = FeDevice::open_rw(0, 0)?;

// Optional: drive the SEC/DiSEqC switch and translate the transponder
// frequency to the intermediate frequency (11044 MHz transponder,
// 9750 MHz LNB local oscillator).
let frequency_khz = fe.use_diseqc(11044, DiseqcConfig::Dsl("t v".to_owned()))?;

let request = TuneRequest::DvbS2(DvbS2Tune {
    frequency_khz,
    symbolrate: 27500 * 1000,
    voltage: SecVoltage::V13,
    tone: SecTone::Off,
    ..Default::default()
});

fe.tune(&request)?;
```

The low-level interface is still available: `TuneRequest::properties()`
builds the typed `Vec<DtvProperty>` command sequence, which can be applied
with `FeDevice::set_properties()`.

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

## DVR

`DvrDevice` opens `/dev/dvb/adapterN/dvrM` in blocking read-only mode.
It implements `Read` and can resize the DVR buffer through the DVB
`DMX_SET_BUFFER_SIZE` ioctl:

```rust
use std::io::Read;

use libdvb::DvrDevice;

let mut dvr = DvrDevice::open(0, 0)?;
dvr.set_buffer_size(100 * 188 * 1024)?;

let mut buf = vec![0; 188 * 1024];
let size = dvr.read(&mut buf)?;
println!("Read {} bytes", size);
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

## External CI (DigitalDevices / TBS)

`SecDevice` opens the CI adapter TS pipe (`ciN` node on DigitalDevices,
`secN` on TBS) in non-blocking mode. It is control plane only: the TS
data path uses the exposed file descriptors.

```rust
use libdvb::SecDevice;

let sec = SecDevice::open(1, 0)?;
sec.set_ci_bitrate(70)?; // MBit/s; TBS only, no-op for other vendors

let fd_in = sec.fd_in();   // write scrambled TS into the CAM
let fd_out = sec.fd_out(); // read descrambled TS from the CAM
```

## CI

`CiController` manages multi-slot CAM insertion/removal, reset,
`CREATE_TC`, transport polling, `RCV` and timeout recovery. It does not
create a thread or own an event loop: integrate its file descriptor into
the application runtime, drain `poll_event()` when readable and call
`tick()` from a monotonic timer. A CAM reaches `CamStatus::Ready` after
valid Application Information and CA Information replies; use `caids()`
for the deduplicated slot list or `session_caids()` for one CA application:

```rust,no_run
use std::time::Instant;

use libdvb::{CaEvent, CiController};

let mut ci = CiController::open(0, 0)?;

// Call periodically (for example, every 100 ms).
ci.tick(Instant::now())?;

// Drain after each tick and from the CA descriptor readable callback.
while let Some(event) = ci.poll_event()? {
    match event {
        CaEvent::SlotStatusChanged { slot_id, new, .. } => {
            println!("CI slot {slot_id}: {new:?}");
        }
        CaEvent::CaInfo { slot_id, session_id, caids } => {
            println!("CI slot {slot_id}, CA session {session_id}: {caids:X?}");
        }
        event => println!("CI: {event:?}"),
    }
}

// A complete raw PMT section, including CRC32. The controller copies all
// data it needs, so the input buffer may be reused after this call.
let raw_pmt: &[u8] = get_raw_pmt_section();
let program_number = ci.set_program(raw_pmt)?;

// Later, withdraw the service by its PMT program_number.
ci.remove_program(program_number)?;

# Ok::<(), libdvb::error::Error>(())
```

## File Descriptors

Demux, DVR, frontend, and network device handles open in blocking mode by default.
The CA device opens in non-blocking mode as required by the CI transport.
All device handles implement `AsFd` and `AsRawFd`, so callers can pass them to APIs
that operate on borrowed or raw file descriptors.

## Code Formatting

```
rustfmt --config "group_imports=StdExternalCrate,imports_granularity=Crate,imports_layout=Vertical,newline_style=Unix,spaces_around_ranges=true,struct_lit_single_line=true,use_field_init_shorthand=true"
```
