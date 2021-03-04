pub mod sys;


use {
    std::{
        io::{
            Read,
        },
        fs::{
            File,
            OpenOptions,
        },
        os::unix::{
            fs::{
                OpenOptionsExt,
            },
            io::{
                AsRawFd,
                RawFd,
            },
        },
        path::Path,
    },

    anyhow::{
        Context,
        Result,
    },

    nix::{
        fcntl::{
            readlink,
        },
        sys::{
            stat::{
                fstat,
                major,
                minor,
            },
        },

        ioctl_readwrite,
        ioctl_write_int_bad,
        request_code_none,
    },

    sys::*,
};


pub const EMPTY_MAC: &str = "00:00:00:00:00:00";


/// A reference to the network device
#[derive(Debug)]
pub struct NetDevice {
    file: File,

    /// Interface name
    name: String,

    /// MAC address
    mac: String,
}


impl AsRawFd for NetDevice {
    #[inline]
    fn as_raw_fd(&self) -> RawFd { self.file.as_raw_fd() }
}


fn read_interface_name<T: AsRawFd>(file: &T) -> Result<String> {
    let s = fstat(file.as_raw_fd())?;

    let path = format!("/sys/dev/char/{}:{}", major(s.st_rdev), minor(s.st_rdev));
    let name = readlink(path.as_str())?
        .to_str()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split(".net")
        .collect::<Vec<&str>>()
        .join("_");

    Ok(name)
}


fn read_mac_address(name: &str) -> Result<String> {
    ensure!(name.starts_with("dvb"), "incorrect interface name");

    let len = 2 * 6 + 5;

    let mut mac = String::with_capacity(len);
    let path = format!("/sys/class/net/{}/address", name);
    let file = File::open(&path)?;
    file.take(len as u64).read_to_string(&mut mac)?;

    Ok(mac)
}


impl NetDevice {
    /// Attempts to open a network device in read-write mode
    pub fn open<P: AsRef<Path>>(path: P) -> Result<NetDevice> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(::nix::libc::O_NONBLOCK)
            .open(path)
            .context("NET: open")?;

        let name = read_interface_name(&file).context("NET: read interface name")?;
        let mac = read_mac_address(&name).unwrap_or_else(|_| EMPTY_MAC.to_owned());

        let net = NetDevice {
            file,
            name,
            mac,
        };

        Ok(net)
    }

    /// Returns interface name in format `dvb{0}_{1}` where `{0}` is adapter number
    /// and `{1}` is a device number
    pub fn get_name(&self) -> &str { self.name.as_str() }

    /// Returns interface MAC address
    pub fn get_mac(&self) -> &str { self.mac.as_str() }

    /// Creates a new network interface
    pub fn add_if(&self, data: &mut DvbNetIf) -> Result<()> {
        // NET_ADD_IF
        ioctl_readwrite!(#[inline] ioctl_call, b'o', 52, DvbNetIf);
        unsafe {
            ioctl_call(self.as_raw_fd(), data as *mut _)
        }.context("NET: add if")?;

        Ok(())
    }

    /// Removes a network interface
    pub fn remove_if(&self, data: &DvbNetIf) -> Result<()> {
        // NET_REMOVE_IF
        ioctl_write_int_bad!(#[inline] ioctl_call, request_code_none!(b'o', 53));
        unsafe {
            ioctl_call(self.as_raw_fd(), data.if_num as _)
        }.context("NET: remove if")?;

        Ok(())
    }
}
