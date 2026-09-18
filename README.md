# libdvb

Rust interface to the Linux DVB API v5.

Delivery systems:

- Satellite: DVB-S, DVB-S2
- Terrestrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C (Annex A, B, C)

SEC: DiSEqC 1.0/1.1, Unicable I (EN 50494), Unicable II (EN 50607).

Modulators (transmit): DigitalDevices Octopus MOD / RESI / SDR (DVB-C,
DVB-T), TBS tbsmod PCIe cards (DVB-C, DVB-T, J.83B, ATSC, ISDB-T, ASI),
HiDes UT-100 USB sticks (DVB-T).

DVB-CI (EN 50221): runtime-neutral `CiController` with link, transport and
session layers and the Resource Manager, Application Information,
Conditional Access Support, Host Control, Date-Time and MMI resources.
CA PMT is built from raw MPEG-TS PMT sections. The `tokio` feature adds
`CiDriver`, an async event loop around `CiController`.

## FeDevice

`TuneRequest` describes a tune per delivery system and is lowered to a
DVBv5 property sequence. SEC setup comes first: `FeDevice::setup_sec()`
takes the transponder frequency, the `Lnb` and the `SecConfig` (voltage,
tone, DiSEqC, Unicable), runs the sequence and returns the frontend
frequency the request must carry; the tune request itself carries no SEC
state.

`Lnb::auto` picks the LNB from the transponder frequency: L band passes
through, C and S bands use a single oscillator, Ku band gets the universal
LNB.

`DTV_STREAM_ID` is set by `DvbS2Tune::mis` (multistream ISI plus PLS) and
`DvbT2Tune::stream_id` (PLP). A root PLS code is converted to the Gold
sequence index; `DTV_SCRAMBLING_SEQUENCE_INDEX` is skipped for root code 0
and on DVB API older than 5.11. The stream id is passed through unchanged,
so driver-specific values work too, such as the BBFrames bit
(`stream_id: 0x8000_0000`) of some DVB-S2 frontends.

Low-level access: `TuneRequest::properties()` returns the `Vec<DtvProperty>`
for `FeDevice::set_properties()`; `sec_sequence()` returns the
`Vec<SecCommand>` for `FeDevice::run_sec_sequence()`. Properties without a
`DtvProperty` variant (`DTV_ISDBT_LAYER*`, custom API-version gating) go
through `DtvPropertyRaw` and `FeDevice::set_properties_raw()`.
`FeDevice::drain_events()` discards queued tune events and keeps the SEC
state; `FeDevice::clear()` switches SEC off.

Frontend information: `api_version()`, `name()`, `delivery_systems()`,
`frequency_range()`, `symbolrate_range()`, `caps()`, `vendor_id()`,
`device_id()`. Frontend status: `get_stats()` returns the DVBv5 `FeStats`
(`status()`, `has_lock()`, `delivery_system()`, `modulation()`, `signal()`
and `cnr()` as `FeLevel` with `decibel()` / `relative()`, `ber()`, `unc()`,
`to_status_string()`); `read_status()`, `read_signal_strength()`,
`read_snr()`, `read_ber()`, `read_unc()` are the DVBv3 ioctls.

## Demux

`DmxDevice` opens `/dev/dvb/adapterN/demuxM`: `set_pes_filter()` with the
`DmxPesFilterParams` of `dmx::sys`, `set_ts_tap()` / `open_ts_tap()` for a
TS tap on one PID (8192 for the whole stream), buffer size, start/stop.

## DVR

`DvrDevice` opens `/dev/dvb/adapterN/dvrM` read-only and blocking. It
implements `Read`; `set_buffer_size()` wraps `DMX_SET_BUFFER_SIZE`.

## NetDevice

`NetDevice::add_if()` creates a dvbnet interface for a PID and feed type and
returns a `NetInterface`, removed on drop; `mac()` returns the interface MAC
address.

## External CI (DigitalDevices / TBS)

`CiTsDevice` opens the CI adapter TS pipe (`ciN` on DigitalDevices, `secN`
on TBS) in non-blocking mode. It only exposes the descriptors: `fd_in()`
takes the scrambled TS into the CAM, `fd_out()` returns the descrambled TS;
`set_input_bitrate()` sets the TBS CI bitrate (a no-op for other vendors).
The control path is `CaDevice` and the en50221 stack.

## BBFrame (DigitalDevices)

With the BBFrames bit in `DvbS2Tune::mis` DigitalDevices frontends deliver
raw DVB-S2 base band frames fragmented into TS packets on PID 270.
`BbFrameDecoder::push()` reassembles them, extracts the user packets of one
ISI and restores the `0x47` sync byte; `push_frame()` decodes a complete
BBFRAME from any transport; `take_foreign_isi()` reports each other input
stream once.

## T2-MI

`T2miDecoder` turns the T2-MI packets (TS 102 773) of one PID back into the
transport stream of one PLP: feed `push()` the TS packets of the T2-MI PID.
The BBFRAMEs inside go through the same extractor as `BbFrameDecoder`, in
normal and high efficiency mode; `take_foreign_plp()` reports each other
PLP once.

## Modulators

Transmit support lives in `modulator`. The vendor-neutral part is the
DVB-T channel parameter types (`DvbtBandwidth`, `DvbtConstellation`,
`DvbtCodeRate`, `DvbtGuard`) and the net TS bitrate math
(`dvbt_ts_bitrate()`, `dvbc_ts_bitrate()`). Device access is one submodule
per vendor: every family has its own UAPI, and the same numeric property
code can mean different things, so there are no cross-vendor device
constants.

`modulator::dd` - DigitalDevices (`/dev/dvb/adapterN/modM`): the classic
DVB-C cards through `DVB_MOD_SET` / `DVB_MOD_CHANNEL_SET` and the
`MODULATOR_*` properties, the SDR cards through the MCI command interface
(`setup_channels_cmd()`, `setup_output_cmd()`, `setup_stream_cmd()`). A
read-only handle configures, the write-only one (`open_wr()`) carries the
TS and starts the RF output. The classic cards pad a slow input with null
packets; the SDR DVB-T path needs an exact CBR stream.

`modulator::tbs` - TBS tbsmod cards (`/dev/tbsmodN/modM`): 6004/6008
DVB-C, 6104 DVB-T, 6014 J.83B, 6034 ATSC, 6214 ISDB-T, 690b ASI. One
write-only handle per channel carries the properties and the TS; opening
enables the RF channel. Card-level properties (frequency, modulation,
symbol rate, gain) are honored on channel 0 only, `MODULATOR_INPUT_BITRATE`
is per channel and a ceiling on the DMA pace: the card pads a slower input
with null packets and holds back a faster one.

`modulator::it950x` - HiDes UT-100 (ITE IT9507, `/dev/usb-it950xN`, driver
v16.11.10.1 or later): `driver_info()`, `acquire_channel()`,
`set_modulation()`, `gain_range()` / `set_gain()`, `start_transfer()`,
`write()` / `write_all()`, `stop_transfer()`. The chip pads a slower input
with null packets, so the stream must be CBR at or below the channel rate.
The gain is digital and deep attenuation costs MER. After an aborted
session the URBs left in flight drain only once RF is on again, so wait
the ring drain time after `set_modulation()` before `start_transfer()`.

## CI

`CiController` handles CAM insertion/removal, reset, `CREATE_TC`, transport
polling, `RCV` and timeout recovery for all slots. It owns no thread or
event loop: poll its file descriptor from the application runtime, drain
`poll_event()` when readable and call `tick()` from a timer. A CAM is
`CamStatus::Ready` after the Application Information and CA Information
replies; `caids()` returns the deduplicated slot list, `session_caids()` a
single CA application.

`set_program()` takes one complete raw PMT section including CRC32 (copied)
and returns the program number `remove_program()` takes back. `tick()`
holds the changes for `CiControllerConfig::ca_pmt_delay` (1 s by default)
after the CAM handshake and then applies at most one per `ca_pmt_interval`
(1 s, adjustable with `set_ca_pmt_interval()`); a long hold breaks some
Irdeto CAMs. `ca_pmt_ready()` reports whether the gate is open. CA_PMT
activity is reported as `CaEvent::CaPmt` (dispatched),
`CaEvent::CaPmtSkipped` (no matching CA descriptor) and
`CaEvent::CaPmtReply` (module verdict).

### Async driver (feature `tokio`)

`CiDriver` owns the event loop: it waits for CA link frames, schedules
`tick()`, idles while the link is suspended and retries a failed `CA_RESET`.
Spawn `run()` on your runtime - the library spawns nothing itself. The
cloneable `CiDriverHandle` sends commands from any thread and exposes
`ready_watch()`; events arrive as `CiDriverEvent` on the returned channel.
CA_PMT pacing is the same as in the manual mode. Dropping all handles or
calling `shutdown()` stops the loop and closes the device.

## Examples

Compiled examples in `examples/`: `feinfo` and `femon` (frontend
information and status), `netinfo` (dvbnet), `cainfo` (prints the inserted
CAMs and exits), `camenu` (interactive CAM menu), `it950x_tx` and
`tbsmod_tx` (transmit the TS from stdin through a HiDes or TBS modulator).

## File Descriptors

All devices open in blocking mode except the CA device, which is
non-blocking as required by the CI transport. Every handle implements
`AsFd` and `AsRawFd`.

## Code Formatting

```
rustfmt --config "group_imports=StdExternalCrate,imports_granularity=Crate,imports_layout=Vertical,newline_style=Unix,spaces_around_ranges=true,struct_lit_single_line=true,use_field_init_shorthand=true"
```
