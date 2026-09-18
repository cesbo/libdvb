//! Interface for HiDes UT-100 family USB DVB-T modulators built on the ITE
//! IT9507 (`/dev/usb-it950xN`, the it950x driver v16.11.10.1 or later; the
//! v13 driver has other struct sizes and is not supported). `N` is the lowest
//! free index at plug time, so the first device is `usb-it950x0`; a
//! `usb-it950x-rxN` node is the receiver of a UT-100A and is not a modulator.
//!
//! One read-write handle carries the ioctls and the TS data
//! ([`ModDevice::open`]). Opening is not exclusive: every open succeeds and
//! all openers share one ring buffer. The first opener powers the chip up
//! (RF stays off); the last close turns RF off and powers it down unless the
//! receiver node is open. Setup order is [`driver_info`] (the check that the
//! driver answers this ABI; [`chip_type`] is informational),
//! [`acquire_channel`] (tunes and runs the TX calibration), [`set_modulation`]
//! (also turns RF on, about 200 ms), optionally [`set_gain`] (the calibration
//! in `acquire_channel` resets it, so it goes after every retune), then
//! [`start_transfer`], the writes, [`stop_transfer`]. The driver masks
//! modulation errors, so the parameters are validated here. `set_modulation`
//! is allowed while streaming, but it drops RF for about 100 ms.
//!
//! A write copies the whole buffer into the driver ring of 16 x 32712 bytes
//! (174 TS packets each) or nothing: `write()` returns 0 when accepted and the
//! positive code 59 when the ring has no room for all of it, and never
//! blocks (a driver version ending in `w` - TSDuck builds - blocks instead of
//! returning 59). [`ModDevice::write`] maps that to `Ok(true)` / `Ok(false)`;
//! [`ModDevice::write_all`] retries every millisecond up to a timeout. The
//! driver checks neither packet alignment nor size, but data leaves the host
//! only in whole 32712-byte URBs, so writes of a multiple of 188 bytes up to
//! 32712 keep URB boundaries on packet boundaries. A trailing partial URB
//! stays queued until more data arrives or [`ModDevice::flush`]. Data written
//! before `start_transfer` is discarded by its ring reset. A failed URB
//! submit surfaces as an errno from `write` and stops the streaming; a
//! `start_transfer` restarts it.
//!
//! The driver has no rate control, null insertion or PCR restamping. The chip
//! stuffs null packets when the input runs slower than the channel, so the
//! stream must be CBR at or below the net rate of the modulation parameters
//! ([`super::dvbt_ts_bitrate`]) and written at a steady pace; an input above
//! the channel rate fills the ring and every write reports no room. No
//! source quantifies the PCR jitter of the chip's stuffing.
//!
//! Neither `stop_transfer` nor closing the device cancels in-flight URBs, and
//! the last close turns RF off, so a session that ends with data in the ring
//! (a killed writer) leaves up to 16 URBs pending until the next
//! `set_modulation` turns RF on and the chip drains them at the channel rate:
//! the 523392-byte ring takes about 130 ms at 31.7 Mbit/s and 840 ms at the
//! lowest DVB-T rate. A `start_transfer` before they complete resets the URB
//! counters under them, the first write fails with `EBUSY` and the driver
//! stops streaming (seen on a UT-100C with driver v18.04.16.2w; the vendor
//! sample sleeps 3 s instead). So wait `ring bytes * 8 / bitrate` after
//! `set_modulation` before `start_transfer` whenever the previous session may
//! have been aborted, and end a session by pausing the writes for that long
//! before `stop_transfer`.
//!
//! The 32712-byte URB is not a multiple of the 512-byte bulk packet, which
//! trips an xHCI bounce-buffer bug of Linux before 5.9 (`Wrong bounce buffer
//! write length` in dmesg): every few seconds three or four consecutive
//! packets reach the air corrupted. Neither the driver nor this module can
//! work around it; use a fixed kernel.
//!
//! The START/STOP request numbers carry `sizeof(unsigned long)`, so a 32-bit
//! process on a 64-bit kernel gets `ENOTTY` (as a positive `ioctl()` return)
//! from them; there is no compat path.
//!
//! Driver `error` codes: 0x19 invalid guard interval, 0x1D invalid bandwidth,
//! 0x27 frequency outside the driver IQ table (50000..=1500000 kHz - the RF
//! capability of a given board is narrower and undocumented), 0x49 unknown
//! chip. The `SETDCCALIBRATIONVALUE` I/Q DC offset ioctl is not exposed: the
//! EEPROM calibration applied by `acquire_channel` covers it, and the hardware
//! meaning of its 9-bit values is not documented.
//!
//! [`driver_info`]: ModDevice::driver_info
//! [`chip_type`]: ModDevice::chip_type
//! [`acquire_channel`]: ModDevice::acquire_channel
//! [`set_modulation`]: ModDevice::set_modulation
//! [`set_gain`]: ModDevice::set_gain
//! [`start_transfer`]: ModDevice::start_transfer
//! [`stop_transfer`]: ModDevice::stop_transfer

pub mod sys;

use std::{
    borrow::Cow,
    ffi::c_int,
    fs::{
        File,
        OpenOptions,
    },
    io,
    io::Write,
    ops::RangeInclusive,
    os::fd::{
        AsFd,
        AsRawFd,
        BorrowedFd,
        RawFd,
    },
    thread,
    time::{
        Duration,
        Instant,
    },
};

use sys::*;

use super::{
    DvbtBandwidth,
    DvbtCodeRate,
    DvbtConstellation,
    DvbtGuard,
};
use crate::{
    error::{
        Error,
        Result,
    },
    fe::sys::{
        TransmitMode,
        cstr_lossy,
    },
};

impl DvbtConstellation {
    /// The matching `CONSTELLATION_*` of the SETMODULE request.
    pub fn ite(self) -> u8 {
        match self {
            DvbtConstellation::Qpsk => CONSTELLATION_QPSK,
            DvbtConstellation::Qam16 => CONSTELLATION_16QAM,
            DvbtConstellation::Qam64 => CONSTELLATION_64QAM,
        }
    }
}

impl DvbtCodeRate {
    /// The matching `CODE_RATE_*` of the SETMODULE request.
    pub fn ite(self) -> u8 {
        match self {
            DvbtCodeRate::Cr1_2 => CODE_RATE_1_OVER_2,
            DvbtCodeRate::Cr2_3 => CODE_RATE_2_OVER_3,
            DvbtCodeRate::Cr3_4 => CODE_RATE_3_OVER_4,
            DvbtCodeRate::Cr5_6 => CODE_RATE_5_OVER_6,
            DvbtCodeRate::Cr7_8 => CODE_RATE_7_OVER_8,
        }
    }
}

impl DvbtGuard {
    /// The matching `INTERVAL_*` of the SETMODULE request.
    pub fn ite(self) -> u8 {
        match self {
            DvbtGuard::G1_32 => INTERVAL_1_OVER_32,
            DvbtGuard::G1_16 => INTERVAL_1_OVER_16,
            DvbtGuard::G1_8 => INTERVAL_1_OVER_8,
            DvbtGuard::G1_4 => INTERVAL_1_OVER_4,
        }
    }
}

/// The `TRANSMISSION_MODE_*` of the SETMODULE request; the chip does only
/// 2K, 8K and 4K.
fn transmission_mode(mode: TransmitMode) -> Result<u8> {
    match mode {
        TransmitMode::Tm2K => Ok(TRANSMISSION_MODE_2K),
        TransmitMode::Tm8K => Ok(TRANSMISSION_MODE_8K),
        TransmitMode::Tm4K => Ok(TRANSMISSION_MODE_4K),
        _ => Err(Error::InvalidProperty(format!(
            "it950x transmission mode must be 2K, 4K or 8K, got {:?}",
            mode
        ))),
    }
}

impl TxModDriverInfo {
    /// Driver release, e.g. `v16.11.10.1`; a trailing `w` marks a blocking
    /// `write()`.
    pub fn driver_version(&self) -> Cow<'_, str> {
        cstr_lossy(&self.driver_version)
    }

    /// Modulator API, e.g. `1.3.20160929.0`.
    pub fn api_version(&self) -> Cow<'_, str> {
        cstr_lossy(&self.api_version)
    }

    /// Link firmware, read from the chip.
    pub fn fw_version_link(&self) -> Cow<'_, str> {
        cstr_lossy(&self.fw_version_link)
    }

    /// OFDM firmware, read from the chip.
    pub fn fw_version_ofdm(&self) -> Cow<'_, str> {
        cstr_lossy(&self.fw_version_ofdm)
    }
}

/// The two failure channels of a handled request: a nonzero `ioctl()` return
/// (an unknown request is a positive `ENOTTY`, not -1 with errno) and a
/// nonzero `error` field after a zero return.
fn check(name: &str, rc: c_int, error: u32) -> Result<()> {
    if rc != 0 {
        return Err(Error::InvalidData(format!(
            "{} returned {} (unsupported by this driver)",
            name, rc
        )));
    }
    if error != 0 {
        return Err(Error::InvalidData(format!(
            "{} failed with error 0x{:02x}",
            name, error
        )));
    }
    Ok(())
}

/// A HiDes it950x modulator device node.
#[derive(Debug)]
pub struct ModDevice {
    file: File,
    adapter: u32,
}

impl AsRawFd for ModDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for ModDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl ModDevice {
    /// Opens `/dev/usb-it950x{adapter}` read-write. The first opener powers
    /// the chip up; RF stays off until [`set_modulation`](Self::set_modulation).
    pub fn open(adapter: u32) -> Result<ModDevice> {
        let path = format!("/dev/usb-it950x{}", adapter);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        Ok(ModDevice { file, adapter })
    }

    /// Index `N` of `/dev/usb-it950xN`.
    pub fn adapter(&self) -> u32 {
        self.adapter
    }

    /// Driver, API and firmware versions (`GETDRIVERINFO`); fails on a
    /// driver that does not answer the request.
    pub fn driver_info(&self) -> Result<TxModDriverInfo> {
        let mut info = TxModDriverInfo::default();
        let rc = unsafe { it950x_get_driver_info(self.as_raw_fd(), &mut info) }?;
        check("IOCTL_ITE_MOD_GETDRIVERINFO", rc, info.error)?;
        Ok(info)
    }

    /// Chip id (`GETCHIPTYPE`): `0x9507` or `0x9503`; the driver reports any
    /// other chip as an error.
    pub fn chip_type(&self) -> Result<u16> {
        let mut req = TxGetChipTypeRequest::default();
        // `_IOW` in the header, but the driver copies the result back
        let rc = unsafe { it950x_get_chip_type(self.as_raw_fd(), &raw mut req as *const _) }?;
        check("IOCTL_ITE_MOD_GETCHIPTYPE", rc, req.error)?;
        Ok(req.chip_type)
    }

    /// Tunes to `frequency_khz` in `bandwidth` (`ACQUIRECHANNEL`) and runs the
    /// TX calibration, which resets the output gain. Does not touch RF or the
    /// streaming. The driver refuses a frequency outside its IQ table.
    pub fn acquire_channel(&self, frequency_khz: u32, bandwidth: DvbtBandwidth) -> Result<()> {
        let mut req = TxAcquireChannelRequest {
            bandwidth: (bandwidth.hz() / 1000) as u16,
            frequency: frequency_khz,
            ..Default::default()
        };
        let rc = unsafe { it950x_acquire_channel(self.as_raw_fd(), &raw mut req as *const _) }?;
        check("IOCTL_ITE_MOD_ACQUIRECHANNEL", rc, req.error)
    }

    /// Sets the OFDM parameters (`SETMODULE`) and turns the RF output on;
    /// blocks for about 200 ms. `mode` must be 2K, 8K or 4K.
    pub fn set_modulation(
        &self,
        mode: TransmitMode,
        constellation: DvbtConstellation,
        code_rate: DvbtCodeRate,
        guard: DvbtGuard,
    ) -> Result<()> {
        let mut req = TxSetModuleRequest {
            transmission_mode: transmission_mode(mode)?,
            constellation: constellation.ite(),
            interval: guard.ite(),
            high_code_rate: code_rate.ite(),
            ..Default::default()
        };
        let rc = unsafe { it950x_set_module(self.as_raw_fd(), &raw mut req as *const _) }?;
        check("IOCTL_ITE_MOD_SETMODULE", rc, req.error)
    }

    /// Output gain range in dB at `frequency_khz` (`GETGAINRANGE`), e.g.
    /// `-52..=6` at 474 MHz. Depends on the frequency only and may be asked
    /// before tuning.
    pub fn gain_range(
        &self,
        frequency_khz: u32,
        bandwidth: DvbtBandwidth,
    ) -> Result<RangeInclusive<i32>> {
        let mut req = TxGetGainRangeRequest {
            frequency: frequency_khz,
            bandwidth: (bandwidth.hz() / 1000) as u16,
            ..Default::default()
        };
        let rc = unsafe { it950x_get_gain_range(self.as_raw_fd(), &raw mut req as *const _) }?;
        check("IOCTL_ITE_MOD_GETGAINRANGE", rc, req.error)?;
        Ok(req.min_gain ..= req.max_gain)
    }

    /// Sets the output gain in dB relative to the calibrated point
    /// (`ADJUSTOUTPUTGAIN`), negative for attenuation, and returns the value
    /// the driver could apply. Goes after `acquire_channel`, which resets it.
    /// The gain is digital (it scales the I/Q coefficients), so deep
    /// attenuation costs MER: a UT-100C measured 23 dB CNR at -30 dB against
    /// 36 dB at -10 dB. Attenuate externally instead.
    pub fn set_gain(&self, gain: i32) -> Result<i32> {
        let mut req = TxSetGainRequest {
            gain_value: gain,
            error: 0,
        };
        let rc = unsafe { it950x_adjust_output_gain(self.as_raw_fd(), &raw mut req as *const _) }?;
        check("IOCTL_ITE_MOD_ADJUSTOUTPUTGAIN", rc, req.error)?;
        Ok(req.gain_value)
    }

    /// Starts streaming (`STARTTRANSFER`): resets the ring buffer, discarding
    /// anything written before, and begins submitting URBs. A no-op while
    /// streaming.
    pub fn start_transfer(&self) -> Result<()> {
        let mut req = TxStartTransferRequest::default();
        let rc = unsafe { it950x_start_transfer(self.as_raw_fd(), &mut req) }?;
        check("IOCTL_ITE_MOD_STARTTRANSFER", rc, 0)
    }

    /// Stops streaming (`STOPTRANSFER`): in-flight URBs complete, the ring is
    /// kept and RF stays on. A no-op when not streaming.
    pub fn stop_transfer(&self) -> Result<()> {
        let mut req = TxStopTransferRequest::default();
        let rc = unsafe { it950x_stop_transfer(self.as_raw_fd(), &mut req) }?;
        check("IOCTL_ITE_MOD_STOPTRANSFER", rc, 0)
    }

    /// Hands the whole buffer to the driver ring: `Ok(true)` when accepted,
    /// `Ok(false)` when the ring has no room for all of it (nothing is
    /// consumed; retry the same buffer after a pause). Never blocks on the
    /// vendor driver. A buffer larger than the ring can never be accepted.
    pub fn write(&self, buf: &[u8]) -> Result<bool> {
        if buf.len() > URB_COUNT_TX * URB_BUFSIZE_TX {
            return Err(Error::InvalidProperty(format!(
                "it950x write of {} bytes exceeds the {} byte ring",
                buf.len(),
                URB_COUNT_TX * URB_BUFSIZE_TX
            )));
        }

        loop {
            match (&self.file).write(buf) {
                Ok(0) => return Ok(true),
                Ok(ERROR_BUFFER_INSUFFICIENT) => return Ok(false),
                Ok(n) => {
                    return Err(Error::InvalidData(format!("it950x write returned {}", n)));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// [`write`](Self::write) retried every millisecond while the ring has
    /// no room, until accepted or `timeout` elapses (`TimedOut`). One
    /// 32712-byte URB frees in about 8 ms at 31.7 Mbit/s and 70 ms at the
    /// slowest DVB-T rate.
    pub fn write_all(&self, buf: &[u8], timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while !self.write(buf)? {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "it950x ring buffer stayed full",
                )
                .into());
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    /// Submits a pending partial URB (a zero-length `write()`), so the tail
    /// of a stream leaves the host. Same result contract as `write`.
    pub fn flush(&self) -> Result<bool> {
        self.write(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvbt_parameter_mappings() {
        assert_eq!(DvbtConstellation::Qpsk.ite(), 0);
        assert_eq!(DvbtConstellation::Qam64.ite(), 2);
        assert_eq!(DvbtCodeRate::Cr1_2.ite(), 0);
        assert_eq!(DvbtCodeRate::Cr7_8.ite(), 4);
        assert_eq!(DvbtGuard::G1_32.ite(), 0);
        assert_eq!(DvbtGuard::G1_4.ite(), 3);
        // 2K and 8K match `fe_transmit_mode` (0, 1); 4K is 2 here, 3 in the kernel enum
        assert_eq!(transmission_mode(TransmitMode::Tm2K).unwrap(), 0);
        assert_eq!(transmission_mode(TransmitMode::Tm8K).unwrap(), 1);
        assert_eq!(transmission_mode(TransmitMode::Tm4K).unwrap(), 2);
        assert!(transmission_mode(TransmitMode::Auto).is_err());
        assert!(transmission_mode(TransmitMode::Tm32K).is_err());
    }

    #[test]
    fn driver_strings() {
        let mut info = TxModDriverInfo::default();
        info.driver_version[.. 11].copy_from_slice(b"v16.11.10.1");
        info.api_version[.. 14].copy_from_slice(b"1.3.20160929.0");
        info.fw_version_link.fill(b'x');
        assert_eq!(info.driver_version(), "v16.11.10.1");
        assert_eq!(info.api_version(), "1.3.20160929.0");
        assert_eq!(info.fw_version_link(), "x".repeat(16));
        assert_eq!(info.fw_version_ofdm(), "");
    }

    #[test]
    fn result_check() {
        assert!(check("X", 0, 0).is_ok());
        // unknown request: positive ENOTTY from ioctl()
        assert!(matches!(check("X", 25, 0), Err(Error::InvalidData(_))));
        assert!(matches!(check("X", 0, 0x27), Err(Error::InvalidData(_))));
    }
}
