pub mod sec;
mod stats;
pub mod sys;
mod tune;

use std::{
    ffi::CStr,
    fmt,
    fs::{
        File,
        OpenOptions,
    },
    ops::Range,
    os::{
        fd::{
            AsFd,
            BorrowedFd,
        },
        unix::io::{
            AsRawFd,
            RawFd,
        },
    },
};

pub use sec::{
    DiseqcSwitchConfig,
    Lnb,
    SecCommand,
    SecConfig,
    SecSetup,
    SecTimings,
    ToneburstConfig,
    UnicableConfig,
    sec_sequence,
};
pub use stats::{
    FeLevel,
    FeStats,
};
pub use tune::{
    AtscTune,
    DvbCAnnex,
    DvbCTune,
    DvbS2Tune,
    DvbSTune,
    DvbT2Tune,
    DvbTTune,
    IsdbTTune,
    Mis,
    PlsMode,
    TuneRequest,
};

use self::sys::*;
use crate::{
    error::{
        Error,
        Result,
    },
    fd::{
        file_status_flags,
        set_file_status_flags,
    },
    sysfs,
};

/// Typed DVBv5 property used to build a frontend command sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtvProperty {
    Frequency(u32),
    Modulation(Modulation),
    BandwidthHz(u32),
    Inversion(Inversion),
    SymbolRate(u32),
    InnerFec(Fec),
    Voltage(SecVoltage),
    Tone(SecTone),
    Pilot(Pilot),
    Rolloff(Rolloff),
    DeliverySystem(DeliverySystem),
    CodeRateHp(Fec),
    CodeRateLp(Fec),
    GuardInterval(GuardInterval),
    TransmissionMode(TransmitMode),
    Hierarchy(Hierarchy),
    StreamId(u32),
    ScramblingSequenceIndex(u32),
    Tune,
    Clear,
}

impl DtvProperty {
    /// Lower the typed property to its on-wire `DtvPropertyRaw` form.
    pub fn to_raw(&self) -> DtvPropertyRaw {
        match *self {
            DtvProperty::Frequency(v) => DtvPropertyRaw::new(DTV_FREQUENCY, v),
            DtvProperty::Modulation(v) => DtvPropertyRaw::new(DTV_MODULATION, v as u32),
            DtvProperty::BandwidthHz(v) => DtvPropertyRaw::new(DTV_BANDWIDTH_HZ, v),
            DtvProperty::Inversion(v) => DtvPropertyRaw::new(DTV_INVERSION, v as u32),
            DtvProperty::SymbolRate(v) => DtvPropertyRaw::new(DTV_SYMBOL_RATE, v),
            DtvProperty::InnerFec(v) => DtvPropertyRaw::new(DTV_INNER_FEC, v as u32),
            DtvProperty::Voltage(v) => DtvPropertyRaw::new(DTV_VOLTAGE, v as u32),
            DtvProperty::Tone(v) => DtvPropertyRaw::new(DTV_TONE, v as u32),
            DtvProperty::Pilot(v) => DtvPropertyRaw::new(DTV_PILOT, v as u32),
            DtvProperty::Rolloff(v) => DtvPropertyRaw::new(DTV_ROLLOFF, v as u32),
            DtvProperty::DeliverySystem(v) => DtvPropertyRaw::new(DTV_DELIVERY_SYSTEM, v as u32),
            DtvProperty::CodeRateHp(v) => DtvPropertyRaw::new(DTV_CODE_RATE_HP, v as u32),
            DtvProperty::CodeRateLp(v) => DtvPropertyRaw::new(DTV_CODE_RATE_LP, v as u32),
            DtvProperty::GuardInterval(v) => DtvPropertyRaw::new(DTV_GUARD_INTERVAL, v as u32),
            DtvProperty::TransmissionMode(v) => {
                DtvPropertyRaw::new(DTV_TRANSMISSION_MODE, v as u32)
            }
            DtvProperty::Hierarchy(v) => DtvPropertyRaw::new(DTV_HIERARCHY, v as u32),
            DtvProperty::StreamId(v) => DtvPropertyRaw::new(DTV_STREAM_ID, v),
            DtvProperty::ScramblingSequenceIndex(v) => {
                DtvPropertyRaw::new(DTV_SCRAMBLING_SEQUENCE_INDEX, v)
            }
            DtvProperty::Tune => DtvPropertyRaw::new(DTV_TUNE, 0),
            DtvProperty::Clear => DtvPropertyRaw::new(DTV_CLEAR, 0),
        }
    }
}

/// DVB API version (major.minor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiVersion {
    pub major: u8,
    pub minor: u8,
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// `dtv_properties` ioctl argument: a count plus a pointer to a property array.
#[repr(C)]
struct DtvProperties {
    num: u32,
    props: *mut DtvPropertyRaw,
}

/// A reference to the frontend device and device information
#[derive(Debug)]
pub struct FeDevice {
    file: File,

    api_version: ApiVersion,

    name: String,
    delivery_system_list: Vec<DeliverySystem>,
    frequency_range: Range<u32>,
    symbolrate_range: Range<u32>,
    caps: FeCaps,

    vendor_id: Option<u32>,
    device_id: Option<u32>,
}

impl AsRawFd for FeDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for FeDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl FeDevice {
    /// Clears frontend settings and event queue
    ///
    /// Switches the LNB voltage and the 22 kHz tone off before sending `DTV_CLEAR`.
    pub fn clear(&self) -> Result<()> {
        let cmdseq = [
            DtvProperty::Voltage(SecVoltage::Off),
            DtvProperty::Tone(SecTone::Off),
            DtvProperty::Clear,
        ];
        self.set_properties(&cmdseq)?;

        self.drain_events()
    }

    /// Reads the queued frontend events and discards them
    ///
    /// The kernel queues up to [`FE_MAX_EVENT`] events, so draining makes the next event report
    /// the current tune only. The device is switched to non-blocking mode for the duration of
    /// the drain and back afterwards, so this also works on a frontend opened in blocking mode;
    /// running two drains, or a drain and a blocking [`FeDevice::get_event`], on one device at
    /// the same time is unsafe.
    pub fn drain_events(&self) -> Result<()> {
        let original_flags = file_status_flags(self.as_raw_fd())?;
        set_file_status_flags(self.as_raw_fd(), original_flags | ::nix::libc::O_NONBLOCK)?;

        let mut event = FeEvent::default();
        // one read more than the queue holds: an overflowed queue reports the
        // overflow first, without dequeuing an event for it
        for _ in 0 ..= FE_MAX_EVENT {
            match self.get_event(&mut event) {
                Ok(()) => {}
                Err(Error::Nix(::nix::errno::Errno::EOVERFLOW)) => {}
                Err(_) => break,
            }
        }

        set_file_status_flags(self.as_raw_fd(), original_flags)?;

        Ok(())
    }

    fn get_info(&mut self) -> Result<()> {
        let mut feinfo = FeInfo::default();

        // FE_GET_INFO
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            61,
            FeInfo
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut feinfo as *mut _) }?;

        if let Ok(name) = CStr::from_bytes_until_nul(&feinfo.name)
            && let Ok(name) = name.to_str()
        {
            self.name = name.to_owned();
        }

        self.frequency_range = feinfo.frequency_min .. feinfo.frequency_max;
        self.symbolrate_range = feinfo.symbol_rate_min .. feinfo.symbol_rate_max;

        self.caps = FeCaps::from_bits_retain(feinfo.caps);

        // DVB v5 properties

        let mut cmdseq = [
            DtvPropertyRaw::new(DTV_API_VERSION, 0),
            DtvPropertyRaw::new(DTV_ENUM_DELSYS, 0),
        ];
        self.get_properties(&mut cmdseq)?;

        // DVB API Version

        let v = cmdseq[0].data() as u16;
        self.api_version = ApiVersion {
            major: (v >> 8) as u8,
            minor: (v & 0xFF) as u8,
        };

        // Supported delivery systems

        let u_buffer = unsafe { cmdseq[1].u.buffer };
        let u_buffer_len = ::std::cmp::min(u_buffer.len as usize, u_buffer.data.len());
        for &v in &u_buffer.data[.. u_buffer_len] {
            if let Ok(ds) = DeliverySystem::try_from(v as u32) {
                self.delivery_system_list.push(ds);
            }
        }

        Ok(())
    }

    fn open(adapter: u32, device: u32, is_write: bool) -> Result<FeDevice> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", adapter, device);
        let file = OpenOptions::new().read(true).write(is_write).open(&path)?;

        let mut fe = FeDevice {
            file,

            api_version: ApiVersion { major: 0, minor: 0 },

            name: String::default(),
            delivery_system_list: Vec::default(),
            frequency_range: 0 .. 0,
            symbolrate_range: 0 .. 0,
            caps: FeCaps::empty(),

            vendor_id: None,
            device_id: None,
        };

        fe.get_info()?;

        fe.vendor_id = sysfs::read_hex_attr(&fe.file, "vendor");
        fe.device_id = sysfs::read_hex_attr(&fe.file, "device");

        Ok(fe)
    }

    /// Attempts to open a frontend device in blocking read-only mode.
    pub fn open_ro(adapter: u32, device: u32) -> Result<FeDevice> {
        Self::open(adapter, device, false)
    }

    /// Attempts to open a frontend device in blocking read-write mode.
    pub fn open_rw(adapter: u32, device: u32) -> Result<FeDevice> {
        Self::open(adapter, device, true)
    }

    /// Sets properties on frontend device
    ///
    /// A `DTV_SCRAMBLING_SEQUENCE_INDEX` is dropped on DVB API older than 5.11; no other
    /// property is validated. Use [`FeDevice::set_properties_raw`] to submit a command
    /// sequence verbatim.
    pub fn set_properties(&self, cmdseq: &[DtvProperty]) -> Result<()> {
        let mut raw: Vec<DtvPropertyRaw> = Vec::with_capacity(cmdseq.len());
        for p in cmdseq {
            // DTV_SCRAMBLING_SEQUENCE_INDEX requires DVB API 5.11 or later
            const API_5_11: ApiVersion = ApiVersion {
                major: 0x05,
                minor: 0x0B,
            };
            if matches!(p, DtvProperty::ScramblingSequenceIndex(_)) && self.api_version < API_5_11 {
                continue;
            }
            raw.push(p.to_raw());
        }

        self.set_properties_raw(&raw)
    }

    /// Sets properties on frontend device from an on-wire command sequence
    ///
    /// The sequence is submitted to `FE_SET_PROPERTY` as it is - nothing is filtered, reordered
    /// or added. This covers properties without a [`DtvProperty`] variant, like the
    /// `DTV_ISDBT_LAYER*` group, and callers doing their own API-version gating.
    ///
    /// `DTV_TUNE` acts on the whole per-frontend property cache, not only on the sequence it
    /// arrives in; send `DTV_CLEAR` first to start from the cache defaults. A sequence longer
    /// than `DTV_IOCTL_MAX_MSGS` properties, or an empty one, is rejected with `EINVAL`.
    pub fn set_properties_raw(&self, cmdseq: &[DtvPropertyRaw]) -> Result<()> {
        // the kernel copies the property array in and never writes back on the
        // set path, so a shared slice is enough for the `*mut` the struct holds
        let cmd = DtvProperties {
            num: cmdseq.len() as u32,
            props: cmdseq.as_ptr() as *mut _,
        };

        // FE_SET_PROPERTY
        nix::ioctl_write_ptr!(
            #[inline]
            ioctl_call,
            b'o',
            82,
            DtvProperties
        );
        unsafe { ioctl_call(self.as_raw_fd(), &cmd as *const _) }?;

        Ok(())
    }

    /// Tunes the frontend using a high-level [`TuneRequest`].
    ///
    /// The request is lowered to a DVBv5 property command sequence and
    /// applied with [`FeDevice::set_properties`].
    /// For satellite systems the SEC step runs first, with
    /// [`FeDevice::setup_sec`]: it drives the LNB and any DiSEqC equipment,
    /// and reports the frequency the request has to carry.
    pub fn tune(&self, request: &TuneRequest) -> Result<()> {
        self.set_properties(&request.properties())
    }

    /// Gets properties from frontend device (raw read path)
    pub(crate) fn get_properties(&self, cmdseq: &mut [DtvPropertyRaw]) -> Result<()> {
        let mut cmd = DtvProperties {
            num: cmdseq.len() as u32,
            props: cmdseq.as_mut_ptr(),
        };

        // FE_GET_PROPERTY
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            83,
            DtvProperties
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut cmd as *mut _) }?;

        Ok(())
    }

    /// Returns a frontend events if available
    pub fn get_event(&self, event: &mut FeEvent) -> Result<()> {
        // FE_GET_EVENT
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            78,
            FeEvent
        );
        unsafe { ioctl_call(self.as_raw_fd(), event as *mut _) }?;

        Ok(())
    }

    /// Returns a snapshot of the frontend statistics.
    ///
    /// Reads the status flags and all DVBv5 statistics properties at once,
    /// with fallback to the DVBv3 API for signal strength/SNR/BER/UNC, so
    /// all values in the returned [`FeStats`] belong to the same point in time.
    pub fn get_stats(&self) -> Result<FeStats> {
        FeStats::read(self)
    }

    /// Returns frontend status flags
    /// - [`FeStatusFlags::NONE`]
    /// - [`FeStatusFlags::HAS_SIGNAL`]
    /// - [`FeStatusFlags::HAS_CARRIER`]
    /// - [`FeStatusFlags::HAS_VITERBI`]
    /// - [`FeStatusFlags::HAS_SYNC`]
    /// - [`FeStatusFlags::HAS_LOCK`]
    /// - [`FeStatusFlags::TIMEDOUT`]
    /// - [`FeStatusFlags::REINIT`]
    pub fn read_status(&self) -> Result<FeStatusFlags> {
        let mut result: u32 = 0;

        // FE_READ_STATUS
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            69,
            u32
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut result as *mut _) }?;

        Ok(FeStatusFlags::from_bits_retain(result))
    }

    /// Reads and returns a signal strength relative value (DVBv3 API)
    pub fn read_signal_strength(&self) -> Result<u16> {
        let mut result: u16 = 0;

        // FE_READ_SIGNAL_STRENGTH
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            71,
            u16
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut result as *mut _) }?;

        Ok(result)
    }

    /// Reads and returns a signal-to-noise ratio, relative value (DVBv3 API)
    pub fn read_snr(&self) -> Result<u16> {
        let mut result: u16 = 0;

        // FE_READ_SNR
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            72,
            u16
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut result as *mut _) }?;

        Ok(result)
    }

    /// Reads and returns a bit error counter (DVBv3 API)
    pub fn read_ber(&self) -> Result<u32> {
        let mut result: u32 = 0;

        // FE_READ_BER
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            70,
            u32
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut result as *mut _) }?;

        Ok(result)
    }

    /// Reads and returns an uncorrected blocks counter (DVBv3 API)
    pub fn read_unc(&self) -> Result<u32> {
        let mut result: u32 = 0;

        // FE_READ_UNCORRECTED_BLOCKS
        nix::ioctl_read!(
            #[inline]
            ioctl_call,
            b'o',
            73,
            u32
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut result as *mut _) }?;

        Ok(result)
    }

    /// Turns on/off generation of the continuous 22kHz tone
    ///
    /// allowed `value`'s:
    ///
    /// - [`SecTone::On`] - turn 22kHz on
    /// - [`SecTone::Off`] - turn 22kHz off
    pub fn set_tone(&self, value: SecTone) -> Result<()> {
        // FE_SET_TONE
        nix::ioctl_write_int_bad!(
            #[inline]
            ioctl_call,
            nix::request_code_none!(b'o', 66)
        );
        unsafe { ioctl_call(self.as_raw_fd(), (value as u32) as _) }?;

        Ok(())
    }

    /// Sets the DC voltage level for LNB
    ///
    /// allowed `value`'s:
    ///
    /// - [`SecVoltage::V13`] for 13V
    /// - [`SecVoltage::V18`] for 18V
    /// - [`SecVoltage::Off`] turns LNB power supply off
    ///
    /// Different power levels used to select internal antennas for different polarizations:
    ///
    /// - 13V:
    ///     - Vertical in linear LNB
    ///     - Right in circular LNB
    /// - 18V:
    ///     - Horizontal in linear LNB
    ///     - Left in circular LNB
    /// - OFF is needed with external power supply, for example to use same LNB with several
    ///   receivers.
    pub fn set_voltage(&self, value: SecVoltage) -> Result<()> {
        // FE_SET_VOLTAGE
        nix::ioctl_write_int_bad!(
            #[inline]
            ioctl_call,
            nix::request_code_none!(b'o', 67)
        );
        unsafe { ioctl_call(self.as_raw_fd(), (value as u32) as _) }?;

        Ok(())
    }

    /// Sends a DiSEqC 22kHz mini-burst (tone burst A / data burst B)
    pub fn diseqc_send_burst(&self, cmd: SecMiniCmd) -> Result<()> {
        // FE_DISEQC_SEND_BURST  ==  _IO('o', 65)
        nix::ioctl_write_int_bad!(
            #[inline]
            ioctl_call,
            nix::request_code_none!(b'o', 65)
        );
        unsafe { ioctl_call(self.as_raw_fd(), (cmd as u32) as _) }?;

        Ok(())
    }

    /// Sets DiSEqC master command
    ///
    /// `msg` is a message no more 6 bytes length
    ///
    /// Example DiSEqC commited command:
    ///
    /// ```text
    /// [0xE0, 0x10, 0x38, 0xF0 | value]
    /// ```
    ///
    /// - byte 1 is a framing (master command without response)
    /// - byte 2 is an address (any LNB)
    /// - byte 3 is a command (commited)
    /// - last 4 bits of byte 4 is:
    ///     - xx00 - switch input
    ///     - 00x0 - bit is set on SecVoltage::V18
    ///     - 000x - bit is set on SecTone::On
    pub fn diseqc_master_cmd(&self, msg: &[u8]) -> Result<()> {
        if !(3 ..= 6).contains(&msg.len()) {
            return Err(Error::InvalidData(format!(
                "DiSEqC master command length must be 3..=6 bytes, got {}",
                msg.len()
            )));
        }

        let mut cmd = DiseqcMasterCmd::default();

        cmd.msg[0 .. msg.len()].copy_from_slice(msg);
        cmd.len = msg.len() as u8;

        // FE_DISEQC_SEND_MASTER_CMD
        nix::ioctl_write_ptr!(ioctl_call, b'o', 63, DiseqcMasterCmd);
        unsafe { ioctl_call(self.as_raw_fd(), &cmd as *const _) }?;

        Ok(())
    }

    /// Points the frontend at a transponder: converts the frequency through
    /// the LNB, sets the polarization voltage and the band tone, drives any
    /// DiSEqC equipment, and blocks for the waits the sequence asks for.
    ///
    /// This is the whole SEC step of a satellite tune, and it must run
    /// before the tune request: the voltage powers the switch and selects
    /// the polarization, the band tone must be silent while a DiSEqC
    /// command is on the wire, and each of those needs time to settle. For
    /// that reason [`TuneRequest`] carries no voltage or tone of its own.
    ///
    /// `transponder_mhz` is the broadcast frequency; returns the frequency
    /// to put in the tune request, in kHz. Use [`SecConfig::Lnb`] when there
    /// is no DiSEqC equipment at all - the voltage and the tone still have
    /// to be set.
    ///
    /// The waits are the [`SecTimings`] defaults. An installation that needs
    /// its own calls [`sec_sequence`] with them and hands the result to
    /// [`FeDevice::run_sec_sequence`], which is what this method does.
    pub fn setup_sec(&self, transponder_mhz: u32, lnb: Lnb, config: SecConfig) -> Result<u32> {
        let setup = sec_sequence(transponder_mhz, lnb, config, SecTimings::default())?;
        self.run_sec_sequence(&setup.sec_sequence)?;

        Ok(setup.frontend_frequency_khz)
    }

    /// Runs a SEC sequence on the frontend, in order.
    ///
    /// [`SecCommand::Wait`] is served by `std::thread::sleep`, so the call
    /// blocks for as long as the waits in the sequence add up to - a
    /// committed-switch sequence takes a little over 200 ms. On an event
    /// loop this belongs on a blocking-work thread, or split the sequence at
    /// its waits and drive it from the runtime timer.
    ///
    /// The sequence usually comes from [`sec_sequence`], which also reports
    /// the frequency to tune to once it has run.
    pub fn run_sec_sequence(&self, sequence: &[SecCommand]) -> Result<()> {
        for command in sequence {
            match command {
                SecCommand::SetTone(value) => self.set_tone(*value)?,
                SecCommand::SetVoltage(value) => self.set_voltage(*value)?,
                SecCommand::SendBurst(value) => self.diseqc_send_burst(*value)?,
                SecCommand::SendMasterCommand(msg) => self.diseqc_master_cmd(msg)?,
                SecCommand::Wait(duration) => std::thread::sleep(*duration),
            }
        }

        Ok(())
    }

    /// Returns the current API version
    pub fn api_version(&self) -> ApiVersion {
        self.api_version
    }

    /// Frontend name as reported by `FE_GET_INFO`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Delivery systems supported by the frontend.
    pub fn delivery_systems(&self) -> &[DeliverySystem] {
        &self.delivery_system_list
    }

    /// Tunable frequency range (kHz units as reported by the kernel).
    pub fn frequency_range(&self) -> Range<u32> {
        self.frequency_range.clone()
    }

    /// Supported symbol-rate range.
    pub fn symbolrate_range(&self) -> Range<u32> {
        self.symbolrate_range.clone()
    }

    /// Frontend capability flags.
    pub fn caps(&self) -> FeCaps {
        self.caps
    }

    /// PCI vendor ID of the frontend device, if reported via sysfs.
    pub fn vendor_id(&self) -> Option<u32> {
        self.vendor_id
    }

    /// PCI device ID of the frontend device, if reported via sysfs.
    pub fn device_id(&self) -> Option<u32> {
        self.device_id
    }
}
