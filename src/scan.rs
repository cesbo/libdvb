//! Enumeration of the DVB devices present under /dev: the frontends of /dev/dvb are probed, the
//! modulator nodes of every supported family are listed by name and never opened.

use std::{
    collections::HashSet,
    fs::File,
    path::{
        Path,
        PathBuf,
    },
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
    /// Whether a process holds the frontend open for writing, this one included. A process the
    /// caller may not inspect (another user's, without root or `CAP_SYS_PTRACE`) is not seen.
    /// `None` when the frontend could not be opened at all.
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

/// Device nodes below `dir` that a process holds open for writing, from a walk over
/// /proc/*/fd as fuser does it.
fn held_for_write(dir: &Path) -> HashSet<PathBuf> {
    let mut result = HashSet::new();

    let Ok(procs) = std::fs::read_dir("/proc") else {
        return result;
    };

    for proc_entry in procs.filter_map(|e| e.ok()) {
        let name = proc_entry.file_name();
        if !name.as_encoded_bytes().iter().all(u8::is_ascii_digit) {
            continue;
        }

        let proc_dir = proc_entry.path();
        let Ok(fds) = std::fs::read_dir(proc_dir.join("fd")) else {
            continue;
        };

        for fd in fds.filter_map(|e| e.ok()) {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };

            if target.starts_with(dir)
                && opened_for_write(&proc_dir.join("fdinfo").join(fd.file_name()))
            {
                result.insert(target);
            }
        }
    }

    result
}

/// Whether a /proc/<pid>/fdinfo/<fd> entry reports a writable access mode.
fn opened_for_write(fdinfo: &Path) -> bool {
    const O_ACCMODE: u32 = 0o3;

    std::fs::read_to_string(fdinfo)
        .ok()
        .and_then(|info| {
            let flags = info.lines().find_map(|line| line.strip_prefix("flags:"))?;
            u32::from_str_radix(flags.trim(), 8).ok()
        })
        .is_some_and(|flags| flags & O_ACCMODE != 0)
}

/// Opens one frontend and reports what it answers.
///
/// The frontend is opened read-only: a read-write open would start the frontend thread and wake
/// the demodulator. `busy` comes from the open file descriptors in /proc instead.
pub fn probe(adapter: u32, device: u32) -> FeProbe {
    let root = Path::new("/dev/dvb");
    let path = root.join(format!("adapter{}/frontend{}", adapter, device));
    let busy = held_for_write(root).contains(&path);

    probe_path(adapter, device, &path, busy)
}

fn probe_path(adapter: u32, device: u32, path: &Path, busy: bool) -> FeProbe {
    let mut result = FeProbe {
        adapter,
        device,
        busy: None,
        info: None,
        mac: None,
        error: None,
    };

    let file = match File::open(path) {
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

/// Probes every frontend below `root`, in readdir order.
///
/// A frontend without demux and dvr nodes next to it is skipped: it cannot deliver a transport
/// stream, so no caller can use it.
fn frontends_in(root: &Path) -> Vec<FeProbe> {
    let mut result = Vec::new();

    if !root.is_dir() {
        return result;
    }

    let held = held_for_write(root);

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

            let path = adapter_dir.join(&frontend_name);
            let busy = held.contains(&path);
            result.push(probe_path(adapter, device, &path, busy));
        }
    }

    result
}

/// Probes every frontend under /dev/dvb, in readdir order.
pub fn scan() -> Vec<FeProbe> {
    frontends_in(Path::new("/dev/dvb"))
}

/// Vendor family of a modulator node; each one has its own UAPI under [`crate::modulator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModVendor {
    /// DigitalDevices, `/dev/dvb/adapterN/modM`.
    Dd,
    /// TBS, `/dev/tbsmodN/modM`.
    Tbs,
    /// HiDes / ITE, `/dev/usb-it950xN`; `device` is always 0.
    It950x,
}

/// One modulator device node, found by its name and never opened.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModNode {
    pub vendor: ModVendor,
    pub adapter: u32,
    pub device: u32,
}

/// Modulator nodes below `dev`, sorted by vendor, adapter and device.
fn modulators_in(dev: &Path) -> Vec<ModNode> {
    let mut result = Vec::new();

    let dvb = dev.join("dvb");
    for adapter_name in entries_with_prefix(&dvb, "adapter") {
        let adapter = last_int(&adapter_name);
        for mod_name in entries_with_prefix(&dvb.join(&adapter_name), "mod") {
            result.push(ModNode {
                vendor: ModVendor::Dd,
                adapter,
                device: last_int(&mod_name),
            });
        }
    }

    for card_name in entries_with_prefix(dev, "tbsmod") {
        let adapter = last_int(&card_name);
        for mod_name in entries_with_prefix(&dev.join(&card_name), "mod") {
            result.push(ModNode {
                vendor: ModVendor::Tbs,
                adapter,
                device: last_int(&mod_name),
            });
        }
    }

    for name in entries_with_prefix(dev, "usb-it950x") {
        result.push(ModNode {
            vendor: ModVendor::It950x,
            adapter: last_int(&name),
            device: 0,
        });
    }

    result.sort();
    result
}

/// Lists the modulator nodes under /dev without opening them.
pub fn modulators() -> Vec<ModNode> {
    modulators_in(Path::new("/dev"))
}

/// The DVB devices of one host: the probed receivers and the modulator nodes, each list sorted by
/// adapter and device.
#[derive(Debug, Default)]
pub struct Inventory {
    pub receivers: Vec<FeProbe>,
    pub modulators: Vec<ModNode>,
}

impl Inventory {
    /// Probes the frontends of /dev/dvb and lists the modulator nodes under /dev.
    pub fn scan() -> Self {
        Self::scan_in(Path::new("/dev"))
    }

    fn scan_in(dev: &Path) -> Self {
        let mut receivers = frontends_in(&dev.join("dvb"));
        receivers.sort_by_key(|probe| (probe.adapter, probe.device));

        Self {
            receivers,
            modulators: modulators_in(dev),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("libdvb-scan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path).unwrap();
    }

    #[test]
    fn device_number_is_the_last_run_of_digits() {
        assert_eq!(last_int("frontend12"), 12);
        assert_eq!(last_int("adapter3"), 3);
        assert_eq!(last_int("frontend"), 0);
        assert_eq!(last_int("a1b2"), 2);
    }

    #[test]
    fn modulator_nodes_by_name() {
        let dev = temp_tree("mods");
        touch(dev.join("dvb/adapter2/mod1"));
        touch(dev.join("dvb/adapter2/mod0"));
        touch(dev.join("dvb/adapter0/frontend0"));
        touch(dev.join("tbsmod1/mod0"));
        touch(dev.join("tbsmod0/mod3"));
        touch(dev.join("usb-it950x4"));
        touch(dev.join("video0"));

        let node = |vendor, adapter, device| ModNode {
            vendor,
            adapter,
            device,
        };
        assert_eq!(
            modulators_in(&dev),
            vec![
                node(ModVendor::Dd, 2, 0),
                node(ModVendor::Dd, 2, 1),
                node(ModVendor::Tbs, 0, 3),
                node(ModVendor::Tbs, 1, 0),
                node(ModVendor::It950x, 4, 0),
            ]
        );

        let _ = std::fs::remove_dir_all(&dev);
    }

    #[test]
    fn held_for_write_reads_the_access_mode() {
        let dir = temp_tree("held").canonicalize().unwrap();
        let rw = dir.join("rw");
        let ro = dir.join("ro");
        touch(rw.clone());
        touch(ro.clone());

        let _rw = std::fs::OpenOptions::new().write(true).open(&rw).unwrap();
        let _ro = File::open(&ro).unwrap();

        let held = held_for_write(&dir);
        assert!(held.contains(&rw));
        assert!(!held.contains(&ro));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inventory_without_dev_is_empty() {
        let inventory = Inventory::scan_in(Path::new("/nonexistent/libdvb-scan"));
        assert!(inventory.receivers.is_empty());
        assert!(inventory.modulators.is_empty());
    }
}
