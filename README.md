# libdvb

libdvb is an interface library for DVB API v5 devices in Linux.

Supports three types of delivery systems:

- Satellite: DVB-S, DVB-S2
- Terrestrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C (Annex A, B, C)
- DiSEqC 1.0
- DiSEqC 1.1
- EN 50494 - Unicable I
- EN 50607 - Unicable II

DVB-CI (EN 50221) support includes a runtime-neutral `CiController`, the
link, transport and session layers, and Resource Manager, Application
Information, Conditional Access Support, Host Control, Date-Time and
high-level MMI resources, including CA PMT program selection from raw
MPEG-TS PMT sections. The optional `tokio` feature adds `CiDriver` - an
async event loop that owns a `CiController` and exposes a thread-safe
command handle, an event stream and a CA_PMT readiness watch.

## FeDevice

Frontend tuning uses the high-level `TuneRequest` enum, which lowers
per-delivery-system parameters to a DVBv5 property command sequence.

Example DVB-S2 tune:

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

// Convert the transponder frequency through the LNB, set the polarization
// voltage and the band tone, drive the DiSEqC equipment if there is any,
// and get the frontend frequency back. This step always comes before the
// tune: the tune request itself carries no SEC state.
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

`Lnb::auto` picks the LNB from the transponder frequency itself for a
configuration that does not name one: an L band frequency passes through, the
C and S bands invert around a single oscillator, and the Ku band gets the
universal LNB above.

`DTV_STREAM_ID` belongs to the two delivery systems that have a stream to
select: `DvbS2Tune::mis` carries it for a multistream transponder, together
with the PLS the stream is scrambled with, and `DvbT2Tune::stream_id` is the
PLP of a T2 multiplex. A root PLS code is resolved to the Gold scrambling
sequence index, `DTV_SCRAMBLING_SEQUENCE_INDEX` is left alone for the default
root code 0, and it is dropped altogether on a DVB API older than 5.11.

The stream id reaches the property unchanged, so a driver-specific value can
be passed through it as well - such as the bit that switches a DVB-S2
frontend to delivering BBFrames instead of a transport stream:

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

The low-level interface is still available: `TuneRequest::properties()`
builds the typed `Vec<DtvProperty>` command sequence, which can be applied
with `FeDevice::set_properties()`. The SEC step splits the same way -
`sec_sequence()` builds the `Vec<SecCommand>` without touching the device,
taking the wait times as an argument, and `FeDevice::run_sec_sequence()`
applies it.

An application that needs full control over the command sequence - property
groups without a `DtvProperty` variant such as `DTV_ISDBT_LAYER*`, or its own
API-version gating - builds `DtvPropertyRaw` values and submits them verbatim
with `FeDevice::set_properties_raw()`. `FeDevice::drain_events()` discards the
events a tune leaves queued without touching the SEC state, which
`FeDevice::clear()` switches off.

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

`CiTsDevice` opens the CI adapter TS pipe (`ciN` node on DigitalDevices,
`secN` on TBS) in non-blocking mode. It is the data path of the adapter
whose control path is `CaDevice` and the en50221 stack above it, and it is
control plane only: the TS itself moves through the exposed file
descriptors.

```rust
use libdvb::CiTsDevice;

let ci = CiTsDevice::open(1, 0)?;
ci.set_input_bitrate(70)?; // MBit/s; TBS only, no-op for other vendors

let fd_in = ci.fd_in();   // write scrambled TS into the CAM
let fd_out = ci.fd_out(); // read descrambled TS from the CAM
```

## CI

`CiController` manages multi-slot CAM insertion/removal, reset,
`CREATE_TC`, transport polling, `RCV` and timeout recovery. It does not
create a thread or own an event loop: integrate its file descriptor into
the application runtime, drain `poll_event()` when readable and call
`tick()` from a monotonic timer. A CAM reaches `CamStatus::Ready` after
valid Application Information and CA Information replies; use `caids()`
for the deduplicated slot list or `session_caids()` for one CA application.

Program changes are paced: `set_program()` and `remove_program()` queue
the change and `tick()` applies at most one per
`CiControllerConfig::ca_pmt_interval` (20 s by default) once the CAM has
confirmed the handshake at least one interval ago - many CAMs ignore or
reject CA_PMT sent too early or too often. `ca_pmt_ready()` reports when
the gate is open:

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
// data it needs, so the input buffer may be reused after this call. The
// change is queued and applied from tick() at the configured pace.
let raw_pmt: &[u8] = get_raw_pmt_section();
let program_number = ci.set_program(raw_pmt)?;

// Later, withdraw the service by its PMT program_number.
ci.remove_program(program_number)?;

# Ok::<(), libdvb::error::Error>(())
```

Two runnable examples cover the CI stack: `examples/cainfo.rs` waits for
the inserted CAMs to identify themselves, prints everything found and
leaves; `examples/camenu.rs` gives interactive line-oriented access to the
CAM menu.

### Async driver (feature `tokio`)

With the `tokio` feature, `CiDriver` owns the event loop: it waits for CA
link frames, computes its own `tick` deadlines, sits out a suspended link
without polling the descriptor, and retries a failed global `CA_RESET`
internally until it succeeds. Spawn the future on your runtime; the
library never spawns tasks or owns a runtime. Commands may be sent from
any thread through the cloneable handle; CA_PMT pacing and the readiness
gate behave exactly as in the externally driven mode. Dropping every
handle (or calling `shutdown()`) stops the loop and closes the device.

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

Demux, DVR, frontend, and network device handles open in blocking mode by default.
The CA device opens in non-blocking mode as required by the CI transport.
All device handles implement `AsFd` and `AsRawFd`, so callers can pass them to APIs
that operate on borrowed or raw file descriptors.

## Code Formatting

```
rustfmt --config "group_imports=StdExternalCrate,imports_granularity=Crate,imports_layout=Vertical,newline_style=Unix,spaces_around_ranges=true,struct_lit_single_line=true,use_field_init_shorthand=true"
```
