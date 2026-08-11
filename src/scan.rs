//! Enumeration of the frontends present in /dev/dvb.

use std::{
    fs::OpenOptions,
    path::Path,
};

use crate::{
    error::Error,
    fe::sys::FeInfo,
    net::{
        EMPTY_MAC,
        NetDevice,
        sys::DVB_NET_FEEDTYPE_MPE,
    },
};

/// What one /dev/dvb/adapterN/frontendM answered while being probed.
///
/// A probe stops at the first step that fails and reports it in `error`, so the fields tell how
/// far it got: `busy` is set once the device is open, `info` once the info block is read.
#[derive(Debug)]
pub struct FeProbe {
    pub adapter: u32,
    pub device: u32,
    /// Whether another process holds the frontend read-write. `None` when the frontend could not
    /// be opened at all.
    pub busy: Option<bool>,
    /// Frontend info block, `None` when `FE_GET_INFO` failed.
    pub info: Option<FeInfo>,
    /// Adapter MAC address. `None` when the driver reports none, or when the probe is skipped
    /// because the driver does not survive it.
    pub mac: Option<String>,
    /// Why the probe stopped early.
    pub error: Option<Error>,
}

/// Last run of digits in a directory entry name ("frontend12" -> 12).
fn last_int(name: &str) -> u32 {
    let mut start = None;
    for (i, c) in name.char_indices() {
        if c.is_ascii_digit() {
            start.get_or_insert(i);
        } else {
            start = None;
        }
    }

    start.and_then(|i| name[i ..].parse().ok()).unwrap_or(0)
}

/// Directory entries with the given name prefix, in readdir order.
fn entries_with_prefix(dir: &Path, prefix: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    rd.filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

/// Adapter MAC address, read through a dvbnet interface that exists for the duration of the read.
fn read_mac(adapter: u32, device: u32) -> Option<String> {
    let net = NetDevice::open(adapter, device).ok()?;
    let iface = net.add_if(0, DVB_NET_FEEDTYPE_MPE).ok()?;
    let mac = iface.mac();

    if mac.is_empty() || mac == EMPTY_MAC {
        return None;
    }

    Some(mac)
}

/// Opens one frontend and reports what it answers.
///
/// The frontend is opened read-write, and read-only when that fails, so a frontend another
/// process is using is reported as busy instead of as an error.
pub fn probe(adapter: u32, device: u32) -> FeProbe {
    let mut result = FeProbe {
        adapter,
        device,
        busy: None,
        info: None,
        mac: None,
        error: None,
    };

    let path = format!("/dev/dvb/adapter{}/frontend{}", adapter, device);

    let mut busy = false;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .or_else(|_| {
            busy = true;
            OpenOptions::new().read(true).open(&path)
        });

    let file = match file {
        Ok(file) => file,
        Err(e) => {
            result.error = Some(e.into());
            return result;
        }
    };

    result.busy = Some(busy);

    let info = match FeInfo::read(&file) {
        Ok(info) => info,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };
    drop(file);

    // NET_ADD_IF hangs or misbehaves on these drivers
    let net_supported = {
        let name = info.name_lossy();
        !name.starts_with("MXL5XX") && !name.starts_with("CXD2854")
    };

    if net_supported {
        result.mac = read_mac(adapter, device);
    }

    result.info = Some(info);

    result
}

/// Probes every frontend under /dev/dvb, in readdir order.
///
/// A frontend without demux and dvr nodes next to it is skipped: it cannot deliver a transport
/// stream, so no caller can use it.
pub fn scan() -> Vec<FeProbe> {
    let mut result = Vec::new();

    let root = Path::new("/dev/dvb");
    if !root.is_dir() {
        return result;
    }

    for adapter_name in entries_with_prefix(root, "adapter") {
        let adapter = last_int(&adapter_name);
        let adapter_dir = root.join(&adapter_name);

        for frontend_name in entries_with_prefix(&adapter_dir, "frontend") {
            let device = last_int(&frontend_name);

            let demux = adapter_dir.join(format!("demux{}", device));
            let dvr = adapter_dir.join(format!("dvr{}", device));
            if !demux.exists() || !dvr.exists() {
                continue;
            }

            result.push(probe(adapter, device));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_number_is_the_last_run_of_digits() {
        assert_eq!(last_int("frontend12"), 12);
        assert_eq!(last_int("adapter3"), 3);
        assert_eq!(last_int("frontend"), 0);
        assert_eq!(last_int("a1b2"), 2);
    }
}
