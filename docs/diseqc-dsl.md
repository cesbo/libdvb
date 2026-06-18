# DiSEqC DSL

libdvb accepts SEC/DiSEqC control sequences written as a short text DSL. A
sequence is a list of single-letter commands, optionally separated by
whitespace, that is parsed into a `Vec<SecCommand>` and then executed against a
frontend.

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

## Parsing

```rust
use libdvb::fe::parse_sec_sequence;

let sequence = parse_sec_sequence("t V W200 [E0 10 38 F3] W15 T")?;
```

This yields:

```rust
use std::time::Duration;
use libdvb::fe::{
    SecCommand,
    sys::{SecTone, SecVoltage},
};

vec![
    SecCommand::SetTone(SecTone::Off),
    SecCommand::SetVoltage(SecVoltage::V18),
    SecCommand::Wait(Duration::from_millis(200)),
    SecCommand::SendMasterCommand(vec![0xE0, 0x10, 0x38, 0xF3]),
    SecCommand::Wait(Duration::from_millis(15)),
    SecCommand::SetTone(SecTone::On),
];
```

`SecCommand` variants:

- `SetTone(SecTone)` — `t` / `T`
- `SetVoltage(SecVoltage)` — `v` / `V`
- `SendBurst(SecMiniCmd)` — `A` / `B`
- `SendMasterCommand(Vec<u8>)` — `[hex ...]`
- `Wait(Duration)` — `W<number>`

## Executing

A parsed sequence is run against an open frontend with
[`FeDevice::execute_sec_sequence`]. Each command maps directly to a frontend
operation, and `Wait` sleeps the current thread for the given duration:

```rust
use libdvb::FeDevice;
use libdvb::fe::parse_sec_sequence;

let fe = FeDevice::open_rw(0, 0)?;
let sequence = parse_sec_sequence("t V W200 [E0 10 38 F3] W15 T")?;
fe.execute_sec_sequence(&sequence)?;
```

## Builders

For the common cases you do not need to hand-write the DSL. These helpers
return the same `Vec<SecCommand>` ready to pass to `execute_sec_sequence`:

- `diseqc_1_0_sequence(port, voltage, tone)` - DiSEqC 1.0 committed switch,
  ports `1..=4`.
- `diseqc_1_1_sequence(port, voltage, tone)` - DiSEqC 1.1 uncommitted switch,
  ports `1..=16`.
- `toneburst_sequence(burst, voltage, tone)` - mini A/B tone burst.

```rust
use libdvb::fe::{
    diseqc_1_0_sequence,
    sys::{SecTone, SecVoltage},
};

// Select committed port 4 at 18V with the tone left on afterwards.
let sequence = diseqc_1_0_sequence(4, SecVoltage::V18, SecTone::On)?;
```
