use std::{
    fs::File,
    os::unix::fs::{
        FileTypeExt,
        MetadataExt,
    },
};

/// Reads a hexadecimal sysfs attribute (for example `vendor` or `device`)
/// of the PCI device backing the given character device file.
pub fn read_hex_attr(file: &File, attr: &str) -> Option<u32> {
    let metadata = file.metadata().ok()?;
    if !metadata.file_type().is_char_device() {
        return None;
    }

    let rdev = metadata.rdev();
    let major = ((rdev >> 8) & 0xFFF) | ((rdev >> 32) & !0xFFF);
    let minor = (rdev & 0xFF) | ((rdev >> 12) & !0xFF);

    let path = format!("/sys/dev/char/{major}:{minor}/device/{attr}");
    let content = std::fs::read_to_string(path).ok()?;

    u32::from_str_radix(content.trim().strip_prefix("0x")?, 16).ok()
}
