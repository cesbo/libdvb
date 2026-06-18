pub mod sys;

use std::{
    fmt,
    fs::{
        File,
        OpenOptions,
    },
    io::Read,
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

use self::sys::*;
use crate::error::Result;

pub const EMPTY_MAC: &str = "00:00:00:00:00:00";
const MAC_SIZE: usize = EMPTY_MAC.len();

/// A reference to the network device
#[derive(Debug)]
pub struct NetDevice {
    adapter: u32,
    device: u32,

    file: File,
}

impl AsRawFd for NetDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl AsFd for NetDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl NetDevice {
    /// Attempts to open a network device in blocking read-write mode.
    pub fn open(adapter: u32, device: u32) -> Result<NetDevice> {
        let path = format!("/dev/dvb/adapter{}/net{}", adapter, device);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        let net = NetDevice {
            adapter,
            device,
            file,
        };

        Ok(net)
    }

    /// Creates a new network interface and returns interface number
    pub fn add_if(&self, pid: u16, feedtype: u8) -> Result<NetInterface<'_>> {
        let mut data = DvbNetIf {
            pid,
            if_num: 0,
            feedtype,
        };

        // NET_ADD_IF
        nix::ioctl_readwrite!(
            #[inline]
            ioctl_call,
            b'o',
            52,
            DvbNetIf
        );
        unsafe { ioctl_call(self.as_raw_fd(), &mut data as *mut _) }?;

        Ok(NetInterface {
            net: self,
            if_num: data.if_num,
        })
    }
}

pub struct NetInterface<'a> {
    net: &'a NetDevice,
    if_num: u16,
}

impl<'a> fmt::Display for NetInterface<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.net.device == 0 {
            write!(f, "dvb{}_{}", self.net.adapter, self.if_num)
        } else {
            write!(
                f,
                "dvb{}{}{}",
                self.net.adapter, self.net.device, self.if_num
            )
        }
    }
}

impl<'a> NetInterface<'a> {
    /// Returns interface mac address or empty mac on any error
    pub fn mac(&self) -> String {
        let path = format!("/sys/class/net/{}/address", self);
        let file = match File::open(&path) {
            Ok(v) => v,
            _ => return EMPTY_MAC.to_owned(),
        };

        let mut mac = String::with_capacity(MAC_SIZE);
        let result = file.take(MAC_SIZE as u64).read_to_string(&mut mac);

        match result {
            Ok(MAC_SIZE) => mac,
            _ => EMPTY_MAC.to_owned(),
        }
    }
}

impl Drop for NetInterface<'_> {
    fn drop(&mut self) {
        // NET_REMOVE_IF
        nix::ioctl_write_int_bad!(
            #[inline]
            ioctl_call,
            nix::request_code_none!(b'o', 53)
        );
        let _ = unsafe { ioctl_call(self.net.as_raw_fd(), self.if_num as _) };
    }
}
