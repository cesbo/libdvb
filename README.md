# libdvb

libdvb is an interface library for DVB-API v5 devices in Linux.

Supports three types of delivery systems:

- Satellite: DVB-S, DVB-S2
- Terretrial: DVB-T, DVB-T2, ATSC, ISDB-T
- Cable: DVB-C

TODO:

- Cenelec EN 50221 - Common Interface Specification for Conditional Access and
  other Digital Video BroadcastingDecoder Applications
- DiSEqC 1.0
- DiSEqC 1.1
- EN 50494 - Unicable I
- EN 50607 - Unicable II

## FeDevice

Example DVB-S2 tune:

```rust
let cmdseq = vec![
    DtvProperty::new(DTV_DELIVERY_SYSTEM, SYS_DVBS2),
    DtvProperty::new(DTV_FREQUENCY, (11044 - 9750) * 1000),
    DtvProperty::new(DTV_MODULATION, PSK_8),
    DtvProperty::new(DTV_VOLTAGE, SEC_VOLTAGE_13),
    DtvProperty::new(DTV_TONE, SEC_TONE_OFF),
    DtvProperty::new(DTV_INVERSION, INVERSION_AUTO),
    DtvProperty::new(DTV_SYMBOL_RATE, 27500 * 1000),
    DtvProperty::new(DTV_INNER_FEC, FEC_AUTO),
    DtvProperty::new(DTV_PILOT, PILOT_AUTO),
    DtvProperty::new(DTV_ROLLOFF, ROLLOFF_35),
    DtvProperty::new(DTV_TUNE, 0),
];

let fe = FeDevice::open_rw("/dev/dvb/adapter0/frontend0")?;
fe.ioctl_set_property(&cmdseq)?;
```

Frontend information:

```rust
let fe = FeDevice::open_rd("/dev/dvb/adapter0/frontend0")?;
println!("{}", &fe);
```

Frontend status:

```rust
let fe = FeDevice::open_rd("/dev/dvb/adapter0/frontend0")?;
let mut status = FeStatus::default();
status.read(&fe)?;
println!("{}", &status.display(1));
```
