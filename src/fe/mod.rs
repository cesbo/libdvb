mod status;


use {
    std::{
        ffi::CStr,
        fmt,
        fs::{
            File,
            OpenOptions,
            read_to_string,
        },
        ops::Range,
        os::linux::fs::MetadataExt,
        os::unix::{
            fs::OpenOptionsExt,
            io::{
                AsRawFd,
                RawFd,
            },
        },
        path::{
            Path,
            PathBuf,
        },
    },

    anyhow::{
        Context,
        Result,
    },
    libc,
    thiserror::Error,

    crate::{
        sys::{
            ioctl,
            IoctlInt,
            frontend::*,
        },
    },
};


pub use {
    status::FeStatus,
};


#[derive(Debug, Error)]
pub enum FeError {
    #[error("frontend is not char device")]
    InvalidDeviceFormat,
    #[error("frequency out of range")]
    InvalidFrequency,
    #[error("symbolrate out of range")]
    InvalidSymbolrate,
    #[error("unknown subsystem")]
    InvalidSubsystem,
    #[error("no auto inversion")]
    NoAutoInversion,
    #[error("no auto transmission mode")]
    NoAutoTransmitMode,
    #[error("no auto guard interval")]
    NoAutoGuardInterval,
    #[error("no auto hierarchy")]
    NoAutoHierarchy,
    #[error("multistream not supported")]
    NoMultistream,
}


#[derive(Debug)]
pub struct FeDevice {
    file: File,

    api_version: u16,

    name: String,
    delivery_system_list: Vec<u32>,
    frequency_range: Range<u32>,
    symbolrate_range: Range<u32>,
    caps: u32,

    vendor_id: u16,
    model_id: u16,
}


impl fmt::Display for FeDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "DVB API: {}.{}", self.api_version >> 8, self.api_version & 0xFF)?;

        writeln!(f, "Device ID: {:04x}:{:04x}", self.vendor_id, self.model_id)?;
        writeln!(f, "Device Name: {}", self.name)?;

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
    #[inline]
    pub fn ioctl<T>(&self, request: IoctlInt, argp: T) -> Result<()> {
        ioctl(self.as_raw_fd(), request, argp).context("fe ioctl")?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut cmdseq = [
            DtvProperty::new(DTV_VOLTAGE, SEC_VOLTAGE_OFF),
            DtvProperty::new(DTV_TONE, SEC_TONE_OFF),
            DtvProperty::new(DTV_CLEAR, 0),
        ];
        self.ioctl_set_property(&mut cmdseq).context("fe clear")?;

        let mut event = FeEvent::default();

        for _ in 0 .. 100 {
            if self.ioctl(FE_GET_EVENT, event.as_mut_ptr()).is_err() {
                break;
            }
        }

        Ok(())
    }

    fn get_info_pci(&mut self, path: &mut PathBuf) -> Result<()> {
        path.push("vendor");
        let vendor = read_to_string(&path)?;
        path.pop();

        let value = vendor.trim_end();
        if value.starts_with("0x") {
            self.vendor_id = u16::from_str_radix(&value[2 ..], 16).unwrap_or(0);
        }

        path.push("device");
        let device = read_to_string(&path)?;
        path.pop();

        let value = device.trim_end();
        if value.starts_with("0x") {
            self.model_id = u16::from_str_radix(&value[2 ..], 16).unwrap_or(0);
        }

        Ok(())
    }

    fn get_info_usb(&mut self, path: &mut PathBuf) -> Result<()> {
        path.push("idVendor");
        let vendor = read_to_string(&path)?;
        path.pop();

        let value = vendor.trim_end();
        self.vendor_id = u16::from_str_radix(value, 16).unwrap_or(0);

        path.push("idProduct");
        let device = read_to_string(&path)?;
        path.pop();

        let value = device.trim_end();
        self.model_id = u16::from_str_radix(value, 16).unwrap_or(0);

        Ok(())
    }

    fn get_info(&mut self) -> Result<()> {
        let mut feinfo = FeInfo::default();
        self.ioctl(FE_GET_INFO, feinfo.as_mut_ptr()).context("fe get info")?;

        let len = unsafe { libc::strnlen(feinfo.name.as_ptr() as *const _, feinfo.name.len()) };
        if let Ok(name) = CStr::from_bytes_with_nul(&feinfo.name[.. len + 1]) {
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
        self.ioctl_get_property(&mut cmdseq).context("fe get api version (deprecated driver)")?;

        // DVB API Version

        self.api_version = unsafe { cmdseq[0].u.data as u16 };

        // Suppoerted delivery systems

        let u_buffer = unsafe { &cmdseq[1].u.buffer };
        let u_buffer_len = ::std::cmp::min(u_buffer.len as usize, u_buffer.data.len());
        u_buffer.data[.. u_buffer_len]
            .iter()
            .for_each(|v| self.delivery_system_list.push(*v as u32));

        // dev-file metadata

        let metadata = self.file.metadata().context("fe get device metadata")?;
        let mode = metadata.st_mode();

        ensure!(
            (mode & ::libc::S_IFMT) == ::libc::S_IFCHR,
            FeError::InvalidDeviceFormat);

        let rdev = metadata.st_rdev();
        let major = unsafe { ::libc::major(rdev) };
        let minor = unsafe { ::libc::minor(rdev) };

        let mut dev_path: PathBuf = format!("/sys/dev/char/{}:{}/device", major, minor).into();

        // USB/PCI subsystem

        dev_path.push("subsystem");
        let subsystem_path = dev_path.read_link().context("fe subsystem read link")?;
        dev_path.pop();

        let subsystem = subsystem_path.file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default();

        match subsystem {
            "pci" => self.get_info_pci(&mut dev_path).context("fe get pci info")?,
            "usb" => self.get_info_usb(&mut dev_path).context("fe get usb info")?,
            _ => bail!(FeError::InvalidSubsystem),
        };

        Ok(())
    }

    pub fn new(path: &Path, write: bool) -> Result<FeDevice> {
        let file = OpenOptions::new()
            .read(true)
            .write(write)
            .custom_flags(::libc::O_NONBLOCK)
            .open(path)
            .context("fe open")?;

        let mut fe = FeDevice {
            file,

            api_version: 0,

            name: String::default(),
            delivery_system_list: Vec::default(),
            frequency_range: 0 .. 0,
            symbolrate_range: 0 .. 0,
            caps: 0,

            vendor_id: 0,
            model_id: 0,
        };

        fe.get_info()?;

        Ok(fe)
    }

    fn check_cmdseq(&self, cmdseq: &[DtvProperty]) -> Result<()> {
        for p in cmdseq {
            match p.cmd {
                DTV_FREQUENCY => {
                    let v = p.get_data();
                    ensure!(
                        self.frequency_range.contains(&v),
                        FeError::InvalidFrequency);
                }
                DTV_SYMBOL_RATE => {
                    let v = p.get_data();
                    ensure!(
                        self.symbolrate_range.contains(&v),
                        FeError::InvalidSymbolrate);
                }
                DTV_INVERSION => {
                    if p.get_data() == INVERSION_AUTO {
                        ensure!(
                            self.caps & FE_CAN_INVERSION_AUTO != 0,
                            FeError::NoAutoInversion);
                    }
                }
                DTV_TRANSMISSION_MODE => {
                    if p.get_data() == TRANSMISSION_MODE_AUTO {
                        ensure!(
                            self.caps & FE_CAN_TRANSMISSION_MODE_AUTO != 0,
                            FeError::NoAutoTransmitMode);
                    }
                }
                DTV_GUARD_INTERVAL => {
                    if p.get_data() == GUARD_INTERVAL_AUTO {
                        ensure!(
                            self.caps & FE_CAN_GUARD_INTERVAL_AUTO != 0,
                            FeError::NoAutoGuardInterval);
                    }
                }
                DTV_HIERARCHY => {
                    if p.get_data() == HIERARCHY_AUTO {
                        ensure!(
                            self.caps & FE_CAN_HIERARCHY_AUTO != 0,
                            FeError::NoAutoHierarchy);
                    }
                }
                DTV_STREAM_ID => {
                    ensure!(
                        self.caps & FE_CAN_MULTISTREAM != 0,
                        FeError::NoMultistream);
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn ioctl_set_property(&self, cmdseq: &mut [DtvProperty]) -> Result<()> {
        self.check_cmdseq(cmdseq).context("fe property check")?;

        let cmd = DtvProperties::new(cmdseq);
        self.ioctl(FE_SET_PROPERTY, cmd.as_ptr())
    }

    pub fn ioctl_get_property(&self, cmdseq: &mut [DtvProperty]) -> Result<()> {
        let mut cmd = DtvProperties::new(cmdseq);
        self.ioctl(FE_GET_PROPERTY, cmd.as_mut_ptr())
    }

    #[inline]
    pub fn get_api_version(&self) -> u16 {
        self.api_version
    }
}
