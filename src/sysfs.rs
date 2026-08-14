//! sysfs attributes of DVB adapter devices

/// PCI vendor and device IDs of the adapter backing the given DVB device
/// node, if reported via sysfs
///
/// `kind` is the device node kind: `"frontend"`, `"ca"`, `"ci"`, `"sec"`
/// and so on. Resolves through the sysfs class tree only, so it works
/// without opening the device node and while the device is busy.
pub fn pci_ids(kind: &str, adapter: u32, device: u32) -> (Option<u32>, Option<u32>) {
    (
        read_hex_attr(kind, adapter, device, "vendor"),
        read_hex_attr(kind, adapter, device, "device"),
    )
}

/// Reads a hexadecimal sysfs attribute (for example `vendor` or `device`)
/// of the PCI device backing the given DVB device node.
fn read_hex_attr(kind: &str, adapter: u32, device: u32, attr: &str) -> Option<u32> {
    let path = format!("/sys/class/dvb/dvb{adapter}.{kind}{device}/device/{attr}");
    let value = &std::fs::read_to_string(path).ok()?;
    u32::from_str_radix(value.trim().strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_ids_absent_device() {
        assert_eq!(pci_ids("frontend", u32::MAX, u32::MAX), (None, None));
    }
}
