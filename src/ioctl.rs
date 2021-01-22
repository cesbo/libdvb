use {
    std::{
        io,
        mem,
        os::unix::io::RawFd,
    },

    libc,
};


#[cfg(target_env = "gnu")]
pub type IoctlInt = libc::c_ulong;

#[cfg(target_env = "musl")]
pub type IoctlInt = libc::c_int;


/// The number of bits used for the number field.
const IOC_NRBITS: IoctlInt = 8;
/// The number of bits used for the type field.
const IOC_TYPEBITS: IoctlInt = 8;


/// Architecture specific values
mod arch {
    use super::IoctlInt;

    /// The number of bits used for the size field.
    pub const IOC_SIZEBITS: IoctlInt = 14;

    // The number of bits used for the direction field.
    // pub const IOC_DIRBITS: IoctlInt = 2;

    /// Neither direction.
    pub const IOC_NONE: IoctlInt = 0;

    /// The write direction.
    pub const IOC_WRITE: IoctlInt = 1;

    /// The read direction.
    pub const IOC_READ: IoctlInt = 2;
}

use arch::*;

// Bitmask for the number field.
// const IOC_NRMASK: IoctlInt = (1 << IOC_NRBITS) - 1;

// Bitmask for the type field.
// const IOC_TYPEMASK: IoctlInt = (1 << IOC_TYPEBITS) - 1;

// Bitmask for the size field.
// const IOC_SIZEMASK: IoctlInt = (1 << IOC_SIZEBITS) - 1;

// Bitmask for the direction field.
// const IOC_DIRMASK: IoctlInt = (1 << IOC_DIRBITS) - 1;

/// Offset of the number field.
const IOC_NRSHIFT: IoctlInt = 0;

/// Offset of the type field.
const IOC_TYPESHIFT: IoctlInt = IOC_NRSHIFT + IOC_NRBITS;

/// Offset of the size field.
const IOC_SIZESHIFT: IoctlInt = IOC_TYPESHIFT + IOC_TYPEBITS;

/// Offset of the direction field.
const IOC_DIRSHIFT: IoctlInt = IOC_SIZESHIFT + IOC_SIZEBITS;


#[inline]
const fn ioc(dr: IoctlInt, ty: IoctlInt, nr: IoctlInt, sz: IoctlInt) -> IoctlInt {
    (dr << IOC_DIRSHIFT) | (ty << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (sz << IOC_SIZESHIFT)
}


#[inline]
pub const fn io_none(ty: u8, nr: u8) -> IoctlInt {
    ioc(
        IOC_NONE,
        ty as IoctlInt,
        nr as IoctlInt,
        0
    )
}


#[inline]
pub const fn io_read<T>(ty: u8, nr: u8) -> IoctlInt {
    ioc(
        IOC_READ,
        ty as IoctlInt,
        nr as IoctlInt,
        mem::size_of::<T>() as IoctlInt
    )
}


#[inline]
pub const fn io_write<T>(ty: u8, nr: u8) -> IoctlInt {
    ioc(
        IOC_WRITE,
        ty as IoctlInt,
        nr as IoctlInt,
        mem::size_of::<T>() as IoctlInt
    )
}


#[inline]
pub const fn io_rw<T>(ty: u8, nr: u8) -> IoctlInt {
    ioc(
        IOC_READ | IOC_WRITE,
        ty as IoctlInt,
        nr as IoctlInt,
        mem::size_of::<T>() as IoctlInt
    )
}


#[inline]
pub fn ioctl<T>(fd: RawFd, request: IoctlInt, argp: T) -> io::Result<i32> {
    let result = unsafe { libc::ioctl(fd, request, argp) };

    if result != -1 {
        Ok(result)
    } else {
        Err(io::Error::last_os_error())
    }
}
