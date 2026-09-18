//! UAPI of the ITE it950x USB DVB-T modulator (`/dev/usb-it950xN`) as
//! defined by `iocontrol.h` of the it950x driver v16.11.10.1 and later
//! (byte-identical through v18.04.16.2 and the TSDuck builds); the `Byte` /
//! `Word` / `Dword` types are `unsigned char` / `unsigned short` / `unsigned
//! long`.
//!
//! Request numbers are the header's unmasked `_IOW`/`_IOR` arithmetic and the
//! driver switches on the whole 32-bit value: the OTHER group (`0x500 + nr`)
//! overflows the 8-bit `nr` field into the type byte, so those requests are
//! type `'o'` with `nr` `0x07..0x09` (`nix` masks `nr` to 8 bits, so they must
//! be spelled that way here). Every `_IOW` request is copied back to user
//! space as well, so its `error` field (and any result field) is valid after
//! the call; the `_IOR` START/STOP requests never touch their argument. An
//! unknown request returns a positive `ENOTTY` (25) from `ioctl()` with errno
//! unset.

use std::ffi::{
    c_int,
    c_ulong,
};

// `TransmissionMode`: `transmissionMode` of the SETMODULE request.
pub const TRANSMISSION_MODE_2K: u8 = 0;
pub const TRANSMISSION_MODE_8K: u8 = 1;
pub const TRANSMISSION_MODE_4K: u8 = 2;

// `Constellation`: `constellation` of the SETMODULE request.
pub const CONSTELLATION_QPSK: u8 = 0;
pub const CONSTELLATION_16QAM: u8 = 1;
pub const CONSTELLATION_64QAM: u8 = 2;

// `Interval`: guard interval, `interval` of the SETMODULE request.
pub const INTERVAL_1_OVER_32: u8 = 0;
pub const INTERVAL_1_OVER_16: u8 = 1;
pub const INTERVAL_1_OVER_8: u8 = 2;
pub const INTERVAL_1_OVER_4: u8 = 3;

// `CodeRate`: `highCodeRate` of the SETMODULE request (`CodeRate_NONE` = 5
// is not valid for transmit).
pub const CODE_RATE_1_OVER_2: u8 = 0;
pub const CODE_RATE_2_OVER_3: u8 = 1;
pub const CODE_RATE_3_OVER_4: u8 = 2;
pub const CODE_RATE_5_OVER_6: u8 = 3;
pub const CODE_RATE_7_OVER_8: u8 = 4;

/// `Error_BUFFER_INSUFFICIENT`, the positive `write()` return when the ring
/// buffer has no room for the whole buffer (nothing is consumed).
pub const ERROR_BUFFER_INSUFFICIENT: usize = 0x3B;

/// Bytes per USB bulk transfer (174 TS packets); data leaves the host only in
/// whole URBs.
pub const URB_BUFSIZE_TX: usize = 32712;

/// URBs in the driver ring buffer.
pub const URB_COUNT_TX: usize = 16;

/// `TxModDriverInfo` - the `IOCTL_ITE_MOD_GETDRIVERINFO` result. The strings
/// are fixed-width and NUL-terminated by the driver except `date_time`, which
/// it never writes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TxModDriverInfo {
    /// `DRIVER_RELEASE_VERSION`, e.g. `v16.11.10.1`; a trailing `w` marks a
    /// TSDuck build with a blocking `write()`.
    pub driver_version: [u8; 16],
    pub api_version: [u8; 32],
    pub fw_version_link: [u8; 16],
    pub fw_version_ofdm: [u8; 16],
    pub date_time: [u8; 24],
    pub company: [u8; 8],
    pub support_hw_info: [u8; 32],
    pub error: u32,
    pub reserved: [u8; 128],
}

impl Default for TxModDriverInfo {
    fn default() -> Self {
        TxModDriverInfo {
            driver_version: [0; 16],
            api_version: [0; 32],
            fw_version_link: [0; 16],
            fw_version_ofdm: [0; 16],
            date_time: [0; 24],
            company: [0; 8],
            support_hw_info: [0; 32],
            error: 0,
            reserved: [0; 128],
        }
    }
}

/// `TxGetChipTypeRequest` - the `IOCTL_ITE_MOD_GETCHIPTYPE` argument.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxGetChipTypeRequest {
    /// `0x9507`, or `0x9503` when LINK register 0xD805 reads 1; anything
    /// else sets `error` 0x49.
    pub chip_type: u16,
    pub error: u32,
    pub reserved: [u8; 16],
}

/// `TxAcquireChannelRequest` - the `IOCTL_ITE_MOD_ACQUIRECHANNEL` argument.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxAcquireChannelRequest {
    /// Always 0.
    pub chip: u8,
    /// kHz: 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 7000 or 8000,
    /// anything else is `error` 0x1D.
    pub bandwidth: u16,
    /// kHz, inside the driver IQ table (50000..=1500000), outside is `error`
    /// 0x27.
    pub frequency: u32,
    pub error: u32,
    pub reserved: [u8; 16],
}

/// `TxSetModuleRequest` - the `IOCTL_ITE_MOD_SETMODULE` argument. The four
/// parameters are written to the OFDM registers unchecked (only `interval > 3`
/// is refused), and the result of the RF enable that follows overwrites
/// `error`, so a bad value is never reported.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxSetModuleRequest {
    /// Always 0.
    pub chip: u8,
    /// `TRANSMISSION_MODE_*`.
    pub transmission_mode: u8,
    /// `CONSTELLATION_*`.
    pub constellation: u8,
    /// `INTERVAL_*`.
    pub interval: u8,
    /// `CODE_RATE_*`.
    pub high_code_rate: u8,
    pub error: u32,
    pub reserved: [u8; 16],
}

/// `TxGetGainRangeRequest` - the `IOCTL_ITE_MOD_GETGAINRANGE` argument. The
/// range depends on `frequency` only; `bandwidth` is merely checked nonzero.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxGetGainRangeRequest {
    pub error: u32,
    /// kHz.
    pub frequency: u32,
    /// kHz, nonzero.
    pub bandwidth: u16,
    /// dB, at least 0.
    pub max_gain: c_int,
    /// dB, at most 0.
    pub min_gain: c_int,
    pub reserved: [u8; 16],
}

/// `TxSetGainRequest` - the `IOCTL_ITE_MOD_ADJUSTOUTPUTGAIN` argument, the
/// only request without a reserved tail.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxSetGainRequest {
    /// dB in 1 dB steps relative to the calibrated point, negative for
    /// attenuation; on return the value actually applied.
    pub gain_value: c_int,
    pub error: u32,
}

/// `TxStartTransferRequest` - the `IOCTL_ITE_MOD_STARTTRANSFER` argument. The
/// driver never reads or writes it; only its size matters, and `error` is a
/// `Dword` (`unsigned long`), so the request number differs between 32- and
/// 64-bit user space.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxStartTransferRequest {
    pub chip: u8,
    pub error: c_ulong,
    pub reserved: [u8; 16],
}

/// `TxStopTransferRequest` - the `IOCTL_ITE_MOD_STOPTRANSFER` argument; see
/// [`TxStartTransferRequest`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxStopTransferRequest {
    pub chip: u8,
    pub error: c_ulong,
    pub reserved: [u8; 16],
}

// IOCTL_ITE_MOD_GETDRIVERINFO: _IOR('k', 0x500 + 0x09, TxModDriverInfo)
nix::ioctl_read!(
    /// Driver, API and firmware versions; the liveness check of a fresh
    /// handle.
    #[inline]
    it950x_get_driver_info,
    b'o',
    0x09,
    TxModDriverInfo
);

// IOCTL_ITE_MOD_GETCHIPTYPE: _IOW('k', 0x3B, TxGetChipTypeRequest)
nix::ioctl_write_ptr!(
    /// Reads the chip id; `chip_type` and `error` are written back.
    #[inline]
    it950x_get_chip_type,
    b'k',
    0x3B,
    TxGetChipTypeRequest
);

// IOCTL_ITE_MOD_ACQUIRECHANNEL: _IOW('k', 0x22, TxAcquireChannelRequest)
nix::ioctl_write_ptr!(
    /// Tunes the LO and runs the TX calibration, which resets the output
    /// gain and the DC calibration; `error` is written back.
    #[inline]
    it950x_acquire_channel,
    b'k',
    0x22,
    TxAcquireChannelRequest
);

// IOCTL_ITE_MOD_SETMODULE: _IOW('k', 0x21, TxSetModuleRequest)
nix::ioctl_write_ptr!(
    /// Sets the OFDM parameters and turns the RF output on (about 200 ms);
    /// `error` is written back.
    #[inline]
    it950x_set_module,
    b'k',
    0x21,
    TxSetModuleRequest
);

// IOCTL_ITE_MOD_GETGAINRANGE: _IOW('k', 0x2C, TxGetGainRangeRequest)
nix::ioctl_write_ptr!(
    /// Gain range at a frequency; `max_gain`, `min_gain` and `error` are
    /// written back.
    #[inline]
    it950x_get_gain_range,
    b'k',
    0x2C,
    TxGetGainRangeRequest
);

// IOCTL_ITE_MOD_ADJUSTOUTPUTGAIN: _IOW('k', 0x2B, TxSetGainRequest)
nix::ioctl_write_ptr!(
    /// Scales the calibrated I/Q coefficients; the applied `gain_value` and
    /// `error` are written back.
    #[inline]
    it950x_adjust_output_gain,
    b'k',
    0x2B,
    TxSetGainRequest
);

// IOCTL_ITE_MOD_STARTTRANSFER: _IOR('k', 0x500 + 0x07, TxStartTransferRequest)
nix::ioctl_read!(
    /// Resets the ring buffer pointers and starts submitting URBs; the
    /// argument is not touched.
    #[inline]
    it950x_start_transfer,
    b'o',
    0x07,
    TxStartTransferRequest
);

// IOCTL_ITE_MOD_STOPTRANSFER: _IOR('k', 0x500 + 0x08, TxStopTransferRequest)
nix::ioctl_read!(
    /// Clears the streaming flag; in-flight URBs complete, the ring is not
    /// reset and the argument is not touched.
    #[inline]
    it950x_stop_transfer,
    b'o',
    0x08,
    TxStopTransferRequest
);

#[cfg(test)]
mod tests {
    use std::mem::{
        offset_of,
        size_of,
    };

    use super::*;

    #[test]
    fn driver_info_layout() {
        assert_eq!(size_of::<TxModDriverInfo>(), 276);
        assert_eq!(offset_of!(TxModDriverInfo, api_version), 16);
        assert_eq!(offset_of!(TxModDriverInfo, support_hw_info), 112);
        assert_eq!(offset_of!(TxModDriverInfo, error), 144);
        assert_eq!(offset_of!(TxModDriverInfo, reserved), 148);
    }

    #[test]
    fn chip_type_layout() {
        assert_eq!(size_of::<TxGetChipTypeRequest>(), 24);
        assert_eq!(offset_of!(TxGetChipTypeRequest, error), 4);
        assert_eq!(offset_of!(TxGetChipTypeRequest, reserved), 8);
    }

    #[test]
    fn acquire_channel_layout() {
        assert_eq!(size_of::<TxAcquireChannelRequest>(), 28);
        assert_eq!(offset_of!(TxAcquireChannelRequest, bandwidth), 2);
        assert_eq!(offset_of!(TxAcquireChannelRequest, frequency), 4);
        assert_eq!(offset_of!(TxAcquireChannelRequest, error), 8);
        assert_eq!(offset_of!(TxAcquireChannelRequest, reserved), 12);
    }

    #[test]
    fn set_module_layout() {
        assert_eq!(size_of::<TxSetModuleRequest>(), 28);
        assert_eq!(offset_of!(TxSetModuleRequest, high_code_rate), 4);
        assert_eq!(offset_of!(TxSetModuleRequest, error), 8);
        assert_eq!(offset_of!(TxSetModuleRequest, reserved), 12);
    }

    #[test]
    fn gain_range_layout() {
        assert_eq!(size_of::<TxGetGainRangeRequest>(), 36);
        assert_eq!(offset_of!(TxGetGainRangeRequest, frequency), 4);
        assert_eq!(offset_of!(TxGetGainRangeRequest, bandwidth), 8);
        assert_eq!(offset_of!(TxGetGainRangeRequest, max_gain), 12);
        assert_eq!(offset_of!(TxGetGainRangeRequest, min_gain), 16);
        assert_eq!(offset_of!(TxGetGainRangeRequest, reserved), 20);
    }

    #[test]
    fn set_gain_layout() {
        assert_eq!(size_of::<TxSetGainRequest>(), 8);
        assert_eq!(offset_of!(TxSetGainRequest, error), 4);
    }

    #[test]
    fn transfer_layout() {
        // `error` is an `unsigned long`: 32 bytes on 64-bit, 24 on 32-bit
        let ul = size_of::<c_ulong>();
        assert_eq!(size_of::<TxStartTransferRequest>(), 2 * ul + 16);
        assert_eq!(offset_of!(TxStartTransferRequest, error), ul);
        assert_eq!(offset_of!(TxStartTransferRequest, reserved), 2 * ul);
        assert_eq!(size_of::<TxStopTransferRequest>(), 2 * ul + 16);
        assert_eq!(offset_of!(TxStopTransferRequest, error), ul);
        assert_eq!(offset_of!(TxStopTransferRequest, reserved), 2 * ul);
    }

    #[test]
    fn request_numbers() {
        // gcc x86_64 values of the header macros
        assert_eq!(
            nix::request_code_read!(b'o', 0x09, size_of::<TxModDriverInfo>()),
            0x8114_6F09_u32 as nix::sys::ioctl::ioctl_num_type
        );
        assert_eq!(
            nix::request_code_write!(b'k', 0x3B, size_of::<TxGetChipTypeRequest>()),
            0x4018_6B3B
        );
        assert_eq!(
            nix::request_code_write!(b'k', 0x22, size_of::<TxAcquireChannelRequest>()),
            0x401C_6B22
        );
        assert_eq!(
            nix::request_code_write!(b'k', 0x21, size_of::<TxSetModuleRequest>()),
            0x401C_6B21
        );
        assert_eq!(
            nix::request_code_write!(b'k', 0x2C, size_of::<TxGetGainRangeRequest>()),
            0x4024_6B2C
        );
        assert_eq!(
            nix::request_code_write!(b'k', 0x2B, size_of::<TxSetGainRequest>()),
            0x4008_6B2B
        );
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(
                nix::request_code_read!(b'o', 0x07, size_of::<TxStartTransferRequest>()),
                0x8020_6F07_u32 as nix::sys::ioctl::ioctl_num_type
            );
            assert_eq!(
                nix::request_code_read!(b'o', 0x08, size_of::<TxStopTransferRequest>()),
                0x8020_6F08_u32 as nix::sys::ioctl::ioctl_num_type
            );
        }
    }
}
