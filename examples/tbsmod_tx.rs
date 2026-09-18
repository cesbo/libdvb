//! Transmits the TS read from stdin through one channel of a TBS DVB-C
//! modulator (tbsmod driver).
//!
//! `tbsmod_tx [adapter] [device] [frequency_hz] [modulation 16|32|64|128|256]
//! [symbol_rate] [gain 0..120] [input_bitrate bit/s]`
//!
//! Card-level properties (frequency, modulation, symbol rate, gain) are
//! honored on channel 0 only; the input bitrate defaults to the net TS rate
//! of the modulation.

use std::io::{
    self,
    Read,
    Write,
};

use libdvb::{
    fe::sys::Modulation,
    modulator::{
        dvbc_ts_bitrate,
        tbs::{
            ModDevice,
            sys::*,
        },
    },
};

fn main() {
    let mut args = std::env::args().skip(1);

    let adapter: u32 = args.next().map(|v| v.parse().unwrap()).unwrap_or(0);
    let device: u32 = args.next().map(|v| v.parse().unwrap()).unwrap_or(0);
    let frequency_hz: u32 = args
        .next()
        .map(|v| v.parse().unwrap())
        .unwrap_or(474_000_000);
    let modulation = match args.next().as_deref().unwrap_or("64") {
        "16" => Modulation::Qam16,
        "32" => Modulation::Qam32,
        "64" => Modulation::Qam64,
        "128" => Modulation::Qam128,
        "256" => Modulation::Qam256,
        v => panic!("modulation 16|32|64|128|256, got {}", v),
    };
    let symbol_rate: u32 = args.next().map(|v| v.parse().unwrap()).unwrap_or(6_900_000);
    let gain: Option<u32> = args.next().map(|v| v.parse().unwrap());
    let net = dvbc_ts_bitrate(symbol_rate, modulation).unwrap();
    let input_bitrate: u64 = args.next().map(|v| v.parse().unwrap()).unwrap_or(net);

    let mut dev = ModDevice::open(adapter, device).unwrap();
    eprintln!("card 0x{:04x}", dev.card().unwrap());

    dev.set_property(MODULATOR_FREQUENCY, frequency_hz).unwrap();
    dev.set_property(MODULATOR_MODULATION, modulation.as_u32())
        .unwrap();
    dev.set_property(MODULATOR_SYMBOL_RATE, symbol_rate)
        .unwrap();
    if let Some(gain) = gain {
        dev.set_property(MODULATOR_GAIN, gain).unwrap();
    }
    // values above 210 are in units of 2^10 bit/s
    dev.set_property(MODULATOR_INPUT_BITRATE, (input_bitrate / 1024) as u32)
        .unwrap();
    eprintln!(
        "net bitrate {} bit/s, input bitrate {} bit/s",
        net, input_bitrate
    );

    let mut stdin = io::stdin().lock();
    let mut buf = vec![0u8; 188 * 96];
    let mut sent: u64 = 0;
    loop {
        let mut filled = 0;
        while filled < buf.len() {
            match stdin.read(&mut buf[filled ..]).unwrap() {
                0 => break,
                n => filled += n,
            }
        }
        let len = filled - filled % 188;
        if len == 0 {
            break;
        }
        if let Err(e) = dev.write_all(&buf[.. len]) {
            eprintln!("write failed after {} packets: {}", sent, e);
            break;
        }
        sent += (len / 188) as u64;
    }
    eprintln!("sent {} packets", sent);
}
