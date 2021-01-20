use {
    super::{
        IoctlInt,
        io_none,
        io_write,
    },
};


pub use {
    dmx_output::*,
    dmx_input::*,
    dmx_ts_pes::*,
    dmx_filter_flags::*,
};


/// Output for the demux
mod dmx_output {
    /// Streaming directly to decoder
    pub const DMX_OUT_DECODER: u32              = 0;
    /// Output going to a memory buffer (to be retrieved via the read command).
    /// Delivers the stream output to the demux device on which the ioctl
    /// is called.
    pub const DMX_OUT_TAP: u32                  = 1;
    /// Output multiplexed into a new TS (to be retrieved by reading from the
    /// logical DVR device). Routes output to the logical DVR device
    /// `/dev/dvb/adapter?/dvr?`, which delivers a TS multiplexed from all
    /// filters for which DMX_OUT_TS_TAP was specified.
    pub const DMX_OUT_TS_TAP: u32               = 2;
    /// Like DMX_OUT_TS_TAP but retrieved from the DMX device.
    pub const DMX_OUT_TSDEMUX_TAP: u32          = 3;
}


/// Input from the demux
mod dmx_input {
    /// Input from a front-end device
    pub const DMX_IN_FRONTEND: u32              = 0;
    /// Input from the logical DVR device
    pub const DMX_IN_DVR: u32                   = 1;
}


/// type of the PES filter
mod dmx_ts_pes {
    /// first audio PID
    pub const DMX_PES_AUDIO0: u32               = 0;
    /// first video PID
    pub const DMX_PES_VIDEO0: u32               = 1;
    /// first teletext PID
    pub const DMX_PES_TELETEXT0: u32            = 2;
    /// first subtitle PID
    pub const DMX_PES_SUBTITLE0: u32            = 3;
    /// first Program Clock Reference PID
    pub const DMX_PES_PCR0: u32                 = 4;

    /// second audio PID.
    pub const DMX_PES_AUDIO1: u32               = 5;
    /// second video PID.
    pub const DMX_PES_VIDEO1: u32               = 6;
    /// second teletext PID.
    pub const DMX_PES_TELETEXT1: u32            = 7;
    /// second subtitle PID.
    pub const DMX_PES_SUBTITLE1: u32            = 8;
    /// second Program Clock Reference PID.
    pub const DMX_PES_PCR1: u32                 = 9;

    /// third audio PID.
    pub const DMX_PES_AUDIO2: u32               = 10;
    /// third video PID.
    pub const DMX_PES_VIDEO2: u32               = 11;
    /// third teletext PID.
    pub const DMX_PES_TELETEXT2: u32            = 12;
    /// third subtitle PID.
    pub const DMX_PES_SUBTITLE2: u32            = 13;
    /// third Program Clock Reference PID.
    pub const DMX_PES_PCR2: u32                 = 14;

    /// fourth audio PID.
    pub const DMX_PES_AUDIO3: u32               = 15;
    /// fourth video PID.
    pub const DMX_PES_VIDEO3: u32               = 16;
    /// fourth teletext PID.
    pub const DMX_PES_TELETEXT3: u32            = 17;
    /// fourth subtitle PID.
    pub const DMX_PES_SUBTITLE3: u32            = 18;
    /// fourth Program Clock Reference PID.
    pub const DMX_PES_PCR3: u32                 = 19;

    /// any other PID.
    pub const DMX_PES_OTHER: u32                = 20;
}


/// Flags for the demux filter
mod dmx_filter_flags {
    /// Only deliver sections where the CRC check succeeded
    pub const DMX_CHECK_CRC: u32                = 1;
    /// Disable the section filter after one section has been delivered
    pub const DMX_ONESHOT: u32                  = 2;
    /// Start filter immediately without requiring a `DMX_START`
    pub const DMX_IMMEDIATE_START: u32          = 4;
}


/// Specifies Packetized Elementary Stream (PES) filter parameters
#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct DmxPesFilterParams {
    /// PID to be filtered. 8192 to pass all PID's
    pub pid: u16,
    /// Demux input, as specified by `DMX_IN_*`
    pub input: u32,
    /// Demux output, as specified by `DMX_OUT_*`
    pub output: u32,
    /// Type of the pes filter, as specified by `DMX_PES_*`
    pub pes_type: u32,
    /// Demux PES flags
    pub flags: u32,
}


pub const DMX_START: IoctlInt = io_none(b'o', 41);
pub const DMX_STOP: IoctlInt = io_none(b'o', 42);
pub const DMX_SET_PES_FILTER: IoctlInt = io_write::<DmxPesFilterParams>(b'o', 44);
pub const DMX_SET_BUFFER_SIZE: IoctlInt = io_none(b'o', 45);
