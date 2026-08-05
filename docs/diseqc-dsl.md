# SEC/DiSEqC DSL

libdvb accepts SEC/DiSEqC control sequences written as a short text DSL. A
sequence is a list of single-letter commands, optionally separated by
whitespace. Pass it through `SecConfig::Dsl` to apply it to a frontend.

```text
t V W200 [E0 10 38 F3] W15 T
```

## Origin

The command language is derived from VDR's `diseqc.conf` format:

<https://github.com/vdr-projects/vdr/blob/master/diseqc.conf>

## Commands

| Token       | Meaning                                                |
|-------------|--------------------------------------------------------|
| `t`         | Tone off (22kHz continuous tone off)                   |
| `T`         | Tone on (22kHz continuous tone on)                     |
| `v`         | Voltage low — 13V (vertical / right circular)          |
| `V`         | Voltage high — 18V (horizontal / left circular)        |
| `A`         | Mini-DiSEqC tone burst A                               |
| `B`         | Mini-DiSEqC tone burst B                               |
| `W<number>` | Wait `<number>` milliseconds before the next command   |
| `[hex ...]` | DiSEqC master command, 3 to 6 bytes                    |

Whitespace between commands is ignored, so `tVW200` and `t V W200` parse
identically.

## Master command hex syntax

A DiSEqC master command is written as hex bytes inside square brackets:

```text
[E0 10 38 F3]
[E01038F3]
```

Rules:

- Each byte is exactly **two** adjacent hex digits (`0-9`, `a-f`, `A-F`).
- Whitespace may appear **between** bytes.
- The command must be **3 to 6 bytes** long, matching the valid Linux DVB
  `FE_DISEQC_SEND_MASTER_CMD` range.

A typical committed-switch command looks like `[E0 10 38 Fx]`:

- byte 1 `E0` — framing (master command, no response expected)
- byte 2 `10` — address (any LNB / switch / positioner)
- byte 3 `38` — command (write to port group 0, "committed")
- byte 4 `Fx` — data, where the low nibble encodes port, voltage and tone

## Using DSL

```rust
use libdvb::{FeDevice, Lnb, SecConfig};

let fe = FeDevice::open_rw(0, 0)?;
let frontend_frequency_khz = fe.setup_sec(
    12_320,
    Lnb::Single { lof_mhz: 10_600 },
    SecConfig::Dsl("t V W200 [E0 10 38 F3] W15 T".to_owned()),
)?;
```

`FeDevice::setup_sec` parses and validates the DSL internally, then runs the
sequence and blocks for the waits in it. It takes the transponder frequency
in MHz, converts it through the `Lnb`, and returns the frontend frequency in
kHz. DSL, LNB, switching, and toneburst configurations return that
intermediate frequency; Unicable configurations return the user-band
frequency.

Every configuration except `Dsl` also takes its band tone from the `Lnb` -
tone on above the switch frequency of a `Universal` LNB, off otherwise - and
those that encode the band in their command do so from the same value. A DSL
sequence spells its tone commands out itself, so the derived band is not
applied to it.

`setup_sec` places the default waits between the commands, and is a wrapper
over the two halves of the job:

```rust
use std::time::Duration;

use libdvb::{FeDevice, Lnb, SecConfig, SecTimings, sec_sequence};

let timings = SecTimings {
    switch_settle: Duration::from_millis(250),
    ..SecTimings::default()
};

let setup = sec_sequence(
    12_320,
    Lnb::Single { lof_mhz: 10_600 },
    SecConfig::Dsl("t V W200 [E0 10 38 F3] W15 T".to_owned()),
    timings,
)?;

let fe = FeDevice::open_rw(0, 0)?;
fe.run_sec_sequence(&setup.sec_sequence)?;
let frontend_frequency_khz = setup.frontend_frequency_khz;
```

`sec_sequence` is pure: it validates the configuration and returns the
commands without touching the device. Its `SecTimings` argument holds the
waits the built-in configurations place between commands - `switch_settle`,
`message_gap`, `unicable_settle`, `unicable_hold` and `lnb_settle`. A DSL
sequence carries its own waits and ignores the struct.

`FeDevice::run_sec_sequence` then applies the commands in order. It sleeps
for the waits on the calling thread, so an application on an event loop
either runs it on a blocking-work thread, or splits the sequence at its
`SecCommand::Wait` entries and drives the parts from its own timer.

## Built-in configurations

`SecConfig` also provides typed configurations for common commands:

- `Lnb { voltage }` - no DiSEqC equipment: polarization voltage and band
  tone only. Nothing is sent, but the voltage and tone still have to be set
  and to settle, so this goes through the same call as the rest.
- `Shared` - an LNB powered by another receiver on the same cable: the
  voltage and the tone are released instead of driven.
- `Switch1_0(DiseqcSwitchConfig)` - DiSEqC 1.0 committed switch, ports
  `1..=4`.
- `Switch1_1(DiseqcSwitchConfig)` - DiSEqC 1.1 uncommitted switch, ports
  `1..=16`.
- `Toneburst(ToneburstConfig)` - mini A/B tone burst.
- `Unicable1(UnicableConfig)` - EN 50494.
- `Unicable2(UnicableConfig)` - EN 50607.

```rust
use libdvb::{
    DiseqcSwitchConfig,
    FeDevice,
    Lnb,
    SecConfig,
};
use libdvb::fe::sys::SecVoltage;

let fe = FeDevice::open_rw(0, 0)?;
let frontend_frequency_khz = fe.setup_sec(
    12_320,
    Lnb::Universal {
        lof_low_mhz: 9_750,
        lof_high_mhz: 10_600,
        switch_mhz: 11_700,
    },
    SecConfig::Switch1_0(DiseqcSwitchConfig {
        port: 4,
        voltage: SecVoltage::V18,
    }),
)?;
```
