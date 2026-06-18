use std::{
    io,
    os::unix::io::RawFd,
};

pub fn file_status_flags(fd: RawFd) -> io::Result<i32> {
    let flags = unsafe { ::nix::libc::fcntl(fd, ::nix::libc::F_GETFL) };

    if flags == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(flags)
    }
}

pub fn set_file_status_flags(fd: RawFd, flags: i32) -> io::Result<()> {
    let result = unsafe { ::nix::libc::fcntl(fd, ::nix::libc::F_SETFL, flags) };

    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
