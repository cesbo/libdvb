//! Transmits the TS read from stdin through a HiDes it950x modulator.
//!
//! `it950x_tx [adapter] [frequency_khz] [bandwidth 6|7|8] [constellation qpsk|16|64]
//! [code_rate 1/2..7/8] [guard 1/32..1/4] [mode 2k|8k] [gain_db]`

use std::{
    io::{
        self,
        Read,
    },
    thread,
    time::Duration,
};

use libdvb::{
    fe::sys::TransmitMode,
    modulator::{
        DvbtBandwidth,
        DvbtCodeRate,
        DvbtConstellation,
        DvbtGuard,
        dvbt_ts_bitrate,
        it950x::{
            ModDevice,
            sys::{
                URB_BUFSIZE_TX,
                URB_COUNT_TX,
            },
        },
    },
};

fn main() {
    let mut args = std::env::args().skip(1);

    let adapter: u32 = args.next().map(|v| v.parse().unwrap()).unwrap_or(0);
    let frequency_khz: u32 = args.next().map(|v| v.parse().unwrap()).unwrap_or(474_000);

    let bandwidth = match args.next().as_deref().unwrap_or("8") {
        "6" => DvbtBandwidth::Mhz6,
        "7" => DvbtBandwidth::Mhz7,
        "8" => DvbtBandwidth::Mhz8,
        v => panic!("bandwidth 6|7|8, got {}", v),
    };
    let constellation = match args.next().as_deref().unwrap_or("64") {
        "qpsk" => DvbtConstellation::Qpsk,
        "16" => DvbtConstellation::Qam16,
        "64" => DvbtConstellation::Qam64,
        v => panic!("constellation qpsk|16|64, got {}", v),
    };
    let code_rate = match args.next().as_deref().unwrap_or("7/8") {
        "1/2" => DvbtCodeRate::Cr1_2,
        "2/3" => DvbtCodeRate::Cr2_3,
        "3/4" => DvbtCodeRate::Cr3_4,
        "5/6" => DvbtCodeRate::Cr5_6,
        "7/8" => DvbtCodeRate::Cr7_8,
        v => panic!("code rate 1/2|2/3|3/4|5/6|7/8, got {}", v),
    };
    let guard = match args.next().as_deref().unwrap_or("1/32") {
        "1/32" => DvbtGuard::G1_32,
        "1/16" => DvbtGuard::G1_16,
        "1/8" => DvbtGuard::G1_8,
        "1/4" => DvbtGuard::G1_4,
        v => panic!("guard 1/32|1/16|1/8|1/4, got {}", v),
    };
    let mode = match args.next().as_deref().unwrap_or("8k") {
        "2k" => TransmitMode::Tm2K,
        "8k" => TransmitMode::Tm8K,
        v => panic!("mode 2k|8k, got {}", v),
    };
    let gain: Option<i32> = args.next().map(|v| v.parse().unwrap());

    let dev = ModDevice::open(adapter).unwrap();

    let info = dev.driver_info().unwrap();
    eprintln!(
        "driver {} api {} fw {} / {}",
        info.driver_version(),
        info.api_version(),
        info.fw_version_link(),
        info.fw_version_ofdm()
    );
    eprintln!("chip 0x{:04x}", dev.chip_type().unwrap());

    dev.acquire_channel(frequency_khz, bandwidth).unwrap();
    dev.set_modulation(mode, constellation, code_rate, guard)
        .unwrap();
    eprintln!(
        "gain range {:?} dB",
        dev.gain_range(frequency_khz, bandwidth).unwrap()
    );
    if let Some(gain) = gain {
        let applied = dev.set_gain(gain).unwrap();
        eprintln!("gain {} dB (asked {})", applied, gain);
    }
    let bitrate = dvbt_ts_bitrate(bandwidth, constellation, code_rate, guard);
    eprintln!("net bitrate {} bit/s", bitrate);

    // URBs left pending by an aborted session drain at the channel rate once
    // RF is on; a START before they complete trips the driver ring accounting
    // and the first write fails with EBUSY
    let ring_bits = (URB_COUNT_TX * URB_BUFSIZE_TX) as u64 * 8;
    thread::sleep(Duration::from_millis(100 + ring_bits * 1000 / bitrate));
    dev.start_transfer().unwrap();

    let mut stdin = io::stdin().lock();
    let mut buf = vec![0u8; URB_BUFSIZE_TX];
    let mut sent: u64 = 0;
    let mut full: u64 = 0;
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
        while !dev.write(&buf[.. len]).unwrap() {
            full += 1;
            thread::sleep(Duration::from_millis(1));
        }
        sent += (len / 188) as u64;
    }

    dev.flush().unwrap();
    dev.stop_transfer().unwrap();
    eprintln!("sent {} packets, {} full-ring waits", sent, full);
}
