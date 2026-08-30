# libdvb

Rust interface to the Linux DVB API v5.

Delivery systems:

- Satellite: DVB-S, DVB-S2
- Terrestrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C (Annex A, B, C)

SEC: DiSEqC 1.0/1.1, Unicable I (EN 50494), Unicable II (EN 50607).

DVB-CI (EN 50221): runtime-neutral `CiController` with link, transport and
session layers and the Resource Manager, Application Information,
Conditional Access Support, Host Control, Date-Time and MMI resources.
CA PMT is built from raw MPEG-TS PMT sections. The `tokio` feature adds
`CiDriver`, an async event loop around `CiController`.

## FeDevice

`TuneRequest` describes a tune per delivery system and is lowered to a
DVBv5 property sequence.

DVB-S2 example:

```rust
use libdvb::{
    DvbS2Tune,
    FeDevice,
    Lnb,
    SecConfig,
    TuneRequest,
    fe::sys::SecVoltage,
};

let fe = FeDevice::open_rw(0, 0)?;

// SEC setup comes first: LNB conversion, voltage, tone, DiSEqC.
// Returns the frontend frequency; the tune request carries no SEC state.
let frequency_khz = fe.setup_sec(
    11044,
    Lnb::Universal {
        lof_low_mhz: 9750,
        lof_high_mhz: 10600,
        switch_mhz: 11700,
    },
    SecConfig::Lnb {
        voltage: SecVoltage::V13,
    },
)?;

let request = TuneRequest::DvbS2(DvbS2Tune {
    frequency_khz,
    symbolrate: 27500 * 1000,
    ..Default::default()
});

fe.tune(&request)?;
```

`Lnb::auto` picks the LNB from the transponder frequency: L band passes
through, C and S bands use a single oscillator, Ku band gets the universal
LNB.

`DTV_STREAM_ID` is set by `DvbS2Tune::mis` (multistream ISI plus PLS) and
`DvbT2Tune::stream_id` (PLP). A root PLS code is converted to the Gold
sequence index; `DTV_SCRAMBLING_SEQUENCE_INDEX` is skipped for root code 0
and on DVB API older than 5.11.

The stream id is passed through unchanged, so driver-specific values work
too, such as the BBFrames bit of some DVB-S2 frontends:

```rust
let request = TuneRequest::DvbS2(DvbS2Tune {
    frequency_khz,
    symbolrate: 27500 * 1000,
    mis: Some(Mis {
        mode: PlsMode::Root,
        code: 0,
        stream_id: 0x8000_0000,
    }),
    ..Default::default()
});
```

Low-level access: `TuneRequest::properties()` returns the `Vec<DtvProperty>`
for `FeDevice::set_properties()`; `sec_sequence()` returns the
`Vec<SecCommand>` for `FeDevice::run_sec_sequence()`. Properties without a
`DtvProperty` variant (`DTV_ISDBT_LAYER*`, custom API-version gating) go
through `DtvPropertyRaw` and `FeDevice::set_properties_raw()`.
`FeDevice::drain_events()` discards queued tune events and keeps the SEC
state; `FeDevice::clear()` switches SEC off.

Frontend information:

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

`FeStatus` also exposes parsed values: `delivery_system()`, `modulation()`,
`signal_strength()`, `signal_strength_decibel()`, `snr()`, `snr_decibel()`,
`ber()`, `unc()`.

## Demux

`DmxDevice` opens `/dev/dvb/adapterN/demuxM`: PES filters, buffer size,
start/stop.

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

`DvrDevice` opens `/dev/dvb/adapterN/dvrM` read-only and blocking. It
implements `Read`; `set_buffer_size()` wraps `DMX_SET_BUFFER_SIZE`.

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

`NetInterface` is removed on drop; `mac()` returns the interface MAC address.

```rust
use libdvb::NetDevice;

let dev = NetDevice::open(0, 0)?;
let interface = dev.add_if(0, libdvb::net::sys::DVB_NET_FEEDTYPE_MPE)?;
println!("Interface: {}", interface);
println!("MAC: {}", interface.mac());
```

## External CI (DigitalDevices / TBS)

`CiTsDevice` opens the CI adapter TS pipe (`ciN` on DigitalDevices, `secN`
on TBS) in non-blocking mode. It only exposes the descriptors; the TS moves
through them directly. The control path is `CaDevice` and the en50221 stack.

```rust
use libdvb::CiTsDevice;

let ci = CiTsDevice::open(1, 0)?;
ci.set_input_bitrate(70)?; // MBit/s; TBS only, no-op for other vendors

let fd_in = ci.fd_in();   // write scrambled TS into the CAM
let fd_out = ci.fd_out(); // read descrambled TS from the CAM
```

## CI

`CiController` handles CAM insertion/removal, reset, `CREATE_TC`, transport
polling, `RCV` and timeout recovery for all slots. It owns no thread or
event loop: poll its file descriptor from the application runtime, drain
`poll_event()` when readable and call `tick()` from a timer. A CAM is
`CamStatus::Ready` after the Application Information and CA Information
replies; `caids()` returns the deduplicated slot list, `session_caids()` a
single CA application.

`set_program()` and `remove_program()` queue changes; `tick()` applies at
most one per `CiControllerConfig::ca_pmt_interval` (20 s by default),
starting one interval after the CAM handshake - many CAMs reject CA_PMT sent
too early or too often. `ca_pmt_ready()` reports whether the gate is open.

```rust,no_run
use std::time::Instant;

use libdvb::{CaEvent, CiController};

let mut ci = CiController::open(0, 0)?;

// Call periodically (for example, every 100 ms).
ci.tick(Instant::now())?;

// Drain after each tick and when the CA descriptor is readable.
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

// A complete raw PMT section, including CRC32. The data is copied, so the
// buffer may be reused. The change is applied from tick().
let raw_pmt: &[u8] = get_raw_pmt_section();
let program_number = ci.set_program(raw_pmt)?;

// Later, withdraw the service by its program_number.
ci.remove_program(program_number)?;

# Ok::<(), libdvb::error::Error>(())
```

Examples: `examples/cainfo.rs` prints the inserted CAMs and exits;
`examples/camenu.rs` is an interactive CAM menu.

### Async driver (feature `tokio`)

`CiDriver` owns the event loop: it waits for CA link frames, schedules
`tick()`, idles while the link is suspended and retries a failed `CA_RESET`.
Spawn `run()` on your runtime - the library spawns nothing itself. The
cloneable handle sends commands from any thread; CA_PMT pacing is the same
as in the manual mode. Dropping all handles or calling `shutdown()` stops
the loop and closes the device.

```rust,no_run
use libdvb::{CiController, CiDriver, CiDriverEvent};

let controller = CiController::open(0, 0)?;
let (driver, handle, mut events) = CiDriver::new(controller);
let ready = handle.ready_watch();   // watch::Receiver<bool>

runtime.spawn(driver.run());

// Program changes from any thread; validation is synchronous.
let program_number = handle.set_program(get_raw_pmt_section())?;

while let Some(event) = events.recv().await {
    match event {
        CiDriverEvent::Ca(event) => println!("CI: {event:?}"),
        event => println!("CI driver: {event:?}"),
    }
}
```

## File Descriptors

All devices open in blocking mode except the CA device, which is
non-blocking as required by the CI transport. Every handle implements
`AsFd` and `AsRawFd`.

## Code Formatting

```
rustfmt --config "group_imports=StdExternalCrate,imports_granularity=Crate,imports_layout=Vertical,newline_style=Unix,spaces_around_ranges=true,struct_lit_single_line=true,use_field_init_shorthand=true"
```
