mod status;
pub mod sys;


use {
    std::{
        ffi::CStr,
        fmt,
        fs::{
            File,
            OpenOptions,
        },
        ops::Range,
        os::unix::{
            fs::{
                OpenOptionsExt,
                FileTypeExt,
            },
            io::{
                AsRawFd,
                RawFd,
            },
        },
    },

    crate::error::{
        Error,
        Result,
    },

    self::sys::*,
};


pub use {
    status::FeStatus,
};


/// A reference to the frontend device and device information
#[derive(Debug)]
pub struct FeDevice {
    file: File,

    api_version: u16,

    name: String,
    delivery_system_list: Vec<u32>,
    frequency_range: Range<u32>,
    symbolrate_range: Range<u32>,
    caps: u32,
}


impl fmt::Display for FeDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "DVB API: {}.{}", self.api_version >> 8, self.api_version & 0xFF)?;
        writeln!(f, "Frontend: {}", self.name)?;

        write!(f, "Delivery system:")?;
        for v in &self.delivery_system_list {
            write!(f, " {}", &DeliverySystemDisplay(*v))?;
        }
        writeln!(f, "")?;

        writeln!(f, "Frequency range: {} .. {}",
            self.frequency_range.start / 1000,
            self.frequency_range.end / 1000)?;

        writeln!(f, "Symbolrate range: {} .. {}",
            self.symbolrate_range.start / 1000,
            self.symbolrate_range.end / 1000)?;

        write!(f, "Frontend capabilities: 0x{:08x}", self.caps)?;

        Ok(())
    }
}


impl AsRawFd for FeDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd { self.file.as_raw_fd() }
}


impl FeDevice {
    /// Clears frontend settings and event queue
    pub fn clear(&self) -> Result<()> {
        let cmdseq = [
            DtvProperty::new(DTV_VOLTAGE, SEC_VOLTAGE_OFF),
            DtvProperty::new(DTV_TONE, SEC_TONE_OFF),
            DtvProperty::new(DTV_CLEAR, 0),
        ];
        self.set_properties(&cmdseq)?;

        let mut event = FeEvent::default();

        for _ in 0 .. FE_MAX_EVENT {
            if self.get_event(&mut event).is_err() {
                break;
            }
        }

        Ok(())
    }

    fn get_info(&mut self) -> Result<()> {
        let mut feinfo = FeInfo::default();

        // FE_GET_INFO
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 61, FeInfo);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut feinfo as *mut _)
        }?;

        if let Some(len) = feinfo.name.iter().position(|&b| b == 0) {
            let name = unsafe { CStr::from_ptr(feinfo.name[.. len + 1].as_ptr()) };
            if let Ok(name) = name.to_str() {
                self.name = name.to_owned();
            }
        }

        self.frequency_range = feinfo.frequency_min .. feinfo.frequency_max;
        self.symbolrate_range = feinfo.symbol_rate_min .. feinfo.symbol_rate_max;

        self.caps = feinfo.caps;

        // DVB v5 properties

        let mut cmdseq = [
            DtvProperty::new(DTV_API_VERSION, 0),
            DtvProperty::new(DTV_ENUM_DELSYS, 0),
        ];
        self.get_properties(&mut cmdseq)?;

        // DVB API Version

        self.api_version = cmdseq[0].get_data() as u16;

        // Suppoerted delivery systems

        let u_buffer = unsafe { cmdseq[1].u.buffer };
        let u_buffer_len = ::std::cmp::min(u_buffer.len as usize, u_buffer.data.len());
        u_buffer.data[.. u_buffer_len]
            .iter()
            .for_each(|v| self.delivery_system_list.push(*v as u32));

        // dev-file metadata

        let metadata = self.file.metadata()?;

        if !metadata.file_type().is_char_device() {
            return Err(Error::InvalidProperty(
                "invalid frontend device path".to_owned()
            ));
        }

        Ok(())
    }

    fn open(adapter: u32, device: u32, is_write: bool) -> Result<FeDevice> {
        let path = format!("/dev/dvb/adapter{}/frontend{}", adapter, device);
        let file = OpenOptions::new()
            .read(true)
            .write(is_write)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(&path)?;

        let mut fe = FeDevice {
            file,

            api_version: 0,

            name: String::default(),
            delivery_system_list: Vec::default(),
            frequency_range: 0 .. 0,
            symbolrate_range: 0 .. 0,
            caps: 0,
        };

        fe.get_info()?;

        Ok(fe)
    }

    /// Attempts to open frontend device in read-only mode
    #[inline]
    pub fn open_ro(adapter: u32, device: u32) -> Result<FeDevice>
    {
        Self::open(adapter, device, false)
    }

    /// Attempts to open frontend device in read-write mode
    #[inline]
    pub fn open_rw(adapter: u32, device: u32) -> Result<FeDevice>
    {
        Self::open(adapter, device, true)
    }

    fn check_properties(&self, cmdseq: &[DtvProperty]) -> Result<()> {
        for p in cmdseq {
            match p.cmd {
                DTV_FREQUENCY => {
                    let v = unsafe { p.u.data };
                    if !self.frequency_range.contains(&v) {
                        return Err(Error::InvalidProperty(
                            "frequency out of range".to_owned()
                        ));
                    }
                }
                DTV_SYMBOL_RATE => {
                    let v = unsafe { p.u.data };
                    if !self.symbolrate_range.contains(&v) {
                        return Err(Error::InvalidProperty(
                            "symbolrate out of range".to_owned()
                        ));
                    }
                }
                DTV_INVERSION => {
                    let v = unsafe { p.u.data };
                    if v == INVERSION_AUTO && (self.caps & FE_CAN_INVERSION_AUTO) == 0 {
                        return Err(Error::InvalidProperty(
                            "frontend does not support auto inversion".to_owned()
                        ));
                    }
                }
                DTV_TRANSMISSION_MODE => {
                    let v = unsafe { p.u.data };
                    if v == TRANSMISSION_MODE_AUTO && (self.caps & FE_CAN_TRANSMISSION_MODE_AUTO) == 0 {
                        return Err(Error::InvalidProperty(
                            "frontend does not support auto transmission mode".to_owned()
                        ));
                    }
                }
                DTV_GUARD_INTERVAL => {
                    let v = unsafe { p.u.data };
                    if v == GUARD_INTERVAL_AUTO && (self.caps & FE_CAN_GUARD_INTERVAL_AUTO) == 0 {
                        return Err(Error::InvalidProperty(
                            "frontend does not support auto guard interval".to_owned()
                        ));
                    }
                }
                DTV_HIERARCHY => {
                    let v = unsafe { p.u.data };
                    if v == HIERARCHY_AUTO && (self.caps & FE_CAN_HIERARCHY_AUTO) == 0 {
                        return Err(Error::InvalidProperty(
                            "frontend does not support auto hierarchy".to_owned()
                        ));
                    }
                }
                DTV_STREAM_ID => {
                    if (self.caps & FE_CAN_MULTISTREAM) == 0 {
                        return Err(Error::InvalidProperty(
                            "frontend does not support multistream".to_owned()
                        ));
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Sets properties on frontend device
    pub fn set_properties(&self, cmdseq: &[DtvProperty]) -> Result<()> {
        self.check_properties(cmdseq)?;

        #[repr(C)]
        pub struct DtvProperties {
            num: u32,
            props: *const DtvProperty,
        }

        let cmd = DtvProperties {
            num: cmdseq.len() as u32,
            props: cmdseq.as_ptr(),
        };

        // FE_SET_PROPERTY
        nix::ioctl_write_ptr!(#[inline] ioctl_call, b'o', 82, DtvProperties);
        unsafe {
            ioctl_call(self.as_raw_fd(), &cmd as *const _)
        }?;

        Ok(())
    }

    /// Gets properties from frontend device
    pub fn get_properties(&self, cmdseq: &mut [DtvProperty]) -> Result<()> {
        #[repr(C)]
        pub struct DtvProperties {
            num: u32,
            props: *mut DtvProperty,
        }

        let mut cmd = DtvProperties {
            num: cmdseq.len() as u32,
            props: cmdseq.as_mut_ptr(),
        };

        // FE_GET_PROPERTY
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 83, DtvProperties);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut cmd as *mut _)
        }?;

        Ok(())
    }

    /// Returns a frontend events if available
    pub fn get_event(&self, event: &mut FeEvent) -> Result<()> {
        // FE_GET_EVENT
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 78, FeEvent);
        unsafe {
            ioctl_call(self.as_raw_fd(), event as *mut _)
        }?;

        Ok(())
    }

    /// Returns frontend status
    /// - [`FE_NONE`]
    /// - [`FE_HAS_SIGNAL`]
    /// - [`FE_HAS_CARRIER`]
    /// - [`FE_HAS_VITERBI`]
    /// - [`FE_HAS_SYNC`]
    /// - [`FE_HAS_LOCK`]
    /// - [`FE_TIMEDOUT`]
    /// - [`FE_REINIT`]
    pub fn read_status(&self) -> Result<u32> {
        let mut result: u32 = FE_NONE;

        // FE_READ_STATUS
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 69, u32);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut result as *mut _)
        }?;

        Ok(result)
    }

    /// Reads and returns a signal strength relative value (DVBv3 API)
    pub fn read_signal_strength(&self) -> Result<u16> {
        let mut result: u16 = 0;

        // FE_READ_SIGNAL_STRENGTH
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 71, u16);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut result as *mut _)
        }?;

        Ok(result)
    }

    /// Reads and returns a signal-to-noise ratio, relative value (DVBv3 API)
    pub fn read_snr(&self) -> Result<u16> {
        let mut result: u16 = 0;

        // FE_READ_SNR
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 72, u16);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut result as *mut _)
        }?;

        Ok(result)
    }

    /// Reads and returns a bit error counter (DVBv3 API)
    pub fn read_ber(&self) -> Result<u32> {
        let mut result: u32 = 0;

        // FE_READ_BER
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 70, u32);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut result as *mut _)
        }?;

        Ok(result)
    }

    /// Reads and returns an uncorrected blocks counter (DVBv3 API)
    pub fn read_unc(&self) -> Result<u32> {
        let mut result: u32 = 0;

        // FE_READ_UNCORRECTED_BLOCKS
        nix::ioctl_read!(#[inline] ioctl_call, b'o', 73, u32);
        unsafe {
            ioctl_call(self.as_raw_fd(), &mut result as *mut _)
        }?;

        Ok(result)
    }

    /// Turns on/off generation of the continuous 22kHz tone
    ///
    /// allowed `value`'s:
    ///
    /// - SEC_TONE_ON - turn 22kHz on
    /// - SEC_TONE_OFF - turn 22kHz off
    pub fn set_tone(&self, value: u32) -> Result<()> {
        // FE_SET_TONE
        nix::ioctl_write_int_bad!(#[inline] ioctl_call, nix::request_code_none!(b'o', 66));

        unsafe {
            ioctl_call(self.as_raw_fd(), value as _)
        }?;

        Ok(())
    }

    /// Sets the DC voltage level for LNB
    ///
    /// allowed `value`'s:
    ///
    /// - SEC_VOLTAGE_13 for 13V
    /// - SEC_VOLTAGE_18 for 18V
    /// - SEC_VOLTAGE_OFF turns LNB power supply off
    ///
    /// Different power levels used to select internal antennas for different polarizations:
    ///
    /// - 13V:
    ///     - Vertical in linear LNB
    ///     - Right in circular LNB
    /// - 18V:
    ///     - Horizontal in linear LNB
    ///     - Left in circular LNB
    /// - OFF is needed with external power supply, for example
    ///   to use same LNB with several receivers.
    pub fn set_voltage(&self, value: u32) -> Result<()> {
        // FE_SET_VOLTAGE
        nix::ioctl_write_int_bad!(#[inline] ioctl_call, nix::request_code_none!(b'o', 67));

        unsafe {
            ioctl_call(self.as_raw_fd(), value as _)
        }?;

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
    ///     - 00x0 - bit is set on SEC_VOLTAGE_18
    ///     - 000x - bit is set on SEC_TONE_ON
    ///
    pub fn diseqc_master_cmd(&self, msg: &[u8]) -> Result<()> {
        let mut cmd = DiseqcMasterCmd::default();
        debug_assert!(msg.len() <= cmd.msg.len());

        cmd.msg[0 .. msg.len()].copy_from_slice(msg);
        cmd.len = msg.len() as u8;

        // FE_DISEQC_SEND_MASTER_CMD
        nix::ioctl_write_ptr!(ioctl_call, b'o', 63, DiseqcMasterCmd);
        unsafe {
            ioctl_call(self.as_raw_fd(), &cmd as *const _)
        }?;

        Ok(())
    }

    /// Returns the current API version
    /// major - first byte
    /// minor - second byte
    #[inline]
    pub fn get_api_version(&self) -> u16 {
        self.api_version
    }
}
