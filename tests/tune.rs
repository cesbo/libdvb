use std::{time, thread};
use dvb::{frontend, tune};

#[test]
fn test_tune() {
    let opt = tune::DvbS2 {
        adapter: tune::Adapter {
            id: 9,
            device: 0,
            modulation: frontend::MODULATION_PSK_8,
        },
        transponder: tune::Transponder {
            frequency: 12732,
            polarization: frontend::SEC_VOLTAGE_13,
            symbolrate: 29950,
        },
        lnb: tune::Lnb {
            mode: tune::LnbMode::AUTO,
            lof1: 9750,
            lof2: 10600,
            slof: 11700,
        },
        fec: frontend::FEC_AUTO,
        rof: frontend::ROLLOFF_35,
        mis: 0,
    };

    let mut dvb = tune::DvbTune::new(&opt).unwrap();
    loop {
        thread::sleep(time::Duration::new(1, 0));
        dvb.status().unwrap();
        println!("status:{:?} signal:{} snr:{} ber:{} unc:{}",
            dvb.status,
            dvb.signal,
            dvb.snr,
            dvb.ber,
            dvb.unc);
    }
}
