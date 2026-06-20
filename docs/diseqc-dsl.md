# DiSEqC DSL

libdvb accepts SEC/DiSEqC control sequences written as a short text DSL. A
sequence is a list of single-letter commands, optionally separated by
whitespace. Pass it through `DiseqcConfig::Dsl` to apply it to a frontend.

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
use libdvb::{DiseqcConfig, FeDevice};

let fe = FeDevice::open_rw(0, 0)?;
let _frontend_frequency_khz = fe.use_diseqc(DiseqcConfig::Dsl(
    "t V W200 [E0 10 38 F3] W15 T".to_owned(),
))?;
```

`FeDevice::use_diseqc` parses and validates the DSL internally. A DSL sequence
does not change the frontend frequency, so it returns `None`. Unicable
configurations return the calculated frontend frequency in kHz.

## Built-in configurations

`DiseqcConfig` also provides typed configurations for common commands:

- `Switch1_0(DiseqcSwitchConfig)` - DiSEqC 1.0 committed switch, ports
  `1..=4`.
- `Switch1_1(DiseqcSwitchConfig)` - DiSEqC 1.1 uncommitted switch, ports
  `1..=16`.
- `Toneburst(ToneburstConfig)` - mini A/B tone burst.
- `Unicable1(UnicableConfig)` - EN 50494.
- `Unicable2(UnicableConfig)` - EN 50607.

```rust
use libdvb::{
    DiseqcConfig,
    DiseqcSwitchConfig,
    FeDevice,
};
use libdvb::fe::sys::{SecTone, SecVoltage};

let fe = FeDevice::open_rw(0, 0)?;
let _frontend_frequency_khz = fe.use_diseqc(DiseqcConfig::Switch1_0(DiseqcSwitchConfig {
    port: 4,
    voltage: SecVoltage::V18,
    tone: SecTone::On,
}))?;
```
