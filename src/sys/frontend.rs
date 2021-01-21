use {
    std::{
        fmt,
        mem,
    },

    libc,

    super::{
        IoctlInt,
        io_none,
        io_read,
        io_write,
    },
};


pub use {
    fe_caps::*,
    fe_type::*,
    fe_sec_voltage::*,
    fe_sec_tone_mode::*,
    fe_sec_mini_cmd::*,
    fe_status::*,
    fe_spectral_inversion::*,
    fe_code_rate::*,
    fe_modulation::*,
    fe_transmit_mode::*,
    fe_guard_interval::*,
    fe_hierarchy::*,
    fe_interleaving::*,
    fe_pilot::*,
    fe_rolloff::*,
    fe_delivery_system::*,
    fecap_scale_params::*,
    dtv_property_cmd::*,
};


/// Frontend capabilities
mod fe_caps {
    /// There's something wrong at the frontend, and it can't report its capabilities
    pub const FE_IS_STUPID: u32                     = 0;
    /// Can auto-detect frequency spectral band inversion
    pub const FE_CAN_INVERSION_AUTO: u32            = 0x1;
    /// Supports FEC 1/2
    pub const FE_CAN_FEC_1_2: u32                   = 0x2;
    /// Supports FEC 2/3
    pub const FE_CAN_FEC_2_3: u32                   = 0x4;
    /// Supports FEC 3/4
    pub const FE_CAN_FEC_3_4: u32                   = 0x8;
    /// Supports FEC 4/5
    pub const FE_CAN_FEC_4_5: u32                   = 0x10;
    /// Supports FEC 5/6
    pub const FE_CAN_FEC_5_6: u32                   = 0x20;
    /// Supports FEC 6/7
    pub const FE_CAN_FEC_6_7: u32                   = 0x40;
    /// Supports FEC 7/8
    pub const FE_CAN_FEC_7_8: u32                   = 0x80;
    /// Supports FEC 8/9
    pub const FE_CAN_FEC_8_9: u32                   = 0x100;
    /// Can auto-detect FEC
    pub const FE_CAN_FEC_AUTO: u32                  = 0x200;
    /// Supports QPSK modulation
    pub const FE_CAN_QPSK: u32                      = 0x400;
    /// Supports 16-QAM modulation
    pub const FE_CAN_QAM_16: u32                    = 0x800;
    /// Supports 32-QAM modulation
    pub const FE_CAN_QAM_32: u32                    = 0x1000;
    /// Supports 64-QAM modulation
    pub const FE_CAN_QAM_64: u32                    = 0x2000;
    /// Supports 128-QAM modulation
    pub const FE_CAN_QAM_128: u32                   = 0x4000;
    /// Supports 256-QAM modulation
    pub const FE_CAN_QAM_256: u32                   = 0x8000;
    /// Can auto-detect QAM modulation
    pub const FE_CAN_QAM_AUTO: u32                  = 0x10000;
    /// Can auto-detect transmission mode
    pub const FE_CAN_TRANSMISSION_MODE_AUTO: u32    = 0x20000;
    /// Can auto-detect bandwidth
    pub const FE_CAN_BANDWIDTH_AUTO: u32            = 0x40000;
    /// Can auto-detect guard interval
    pub const FE_CAN_GUARD_INTERVAL_AUTO: u32       = 0x80000;
    /// Can auto-detect hierarchy
    pub const FE_CAN_HIERARCHY_AUTO: u32            = 0x100000;
    /// Supports 8-VSB modulation
    pub const FE_CAN_8VSB: u32                      = 0x200000;
    /// Supports 16-VSB modulation
    pub const FE_CAN_16VSB: u32                     = 0x400000;
    /// Unused
    pub const FE_HAS_EXTENDED_CAPS: u32             = 0x800000;
    /// Supports multistream filtering
    pub const FE_CAN_MULTISTREAM: u32               = 0x4000000;
    /// Supports "turbo FEC" modulation
    pub const FE_CAN_TURBO_FEC: u32                 = 0x8000000;
    /// Supports "2nd generation" modulation, e. g. DVB-S2, DVB-T2, DVB-C2
    pub const FE_CAN_2G_MODULATION: u32             = 0x10000000;
    /// Unused
    pub const FE_NEEDS_BENDING: u32                 = 0x20000000;
    /// Can recover from a cable unplug automatically
    pub const FE_CAN_RECOVER: u32                   = 0x40000000;
    /// Can stop spurious TS data output
    pub const FE_CAN_MUTE_TS: u32                   = 0x80000000;
}


/// DEPRECATED: Should be kept just due to backward compatibility
mod fe_type {
    pub const FE_QPSK: u32                      = 0;
    pub const FE_QAM: u32                       = 1;
    pub const FE_OFDM: u32                      = 2;
    pub const FE_ATSC: u32                      = 3;
}


/// Frontend properties and capabilities
/// The frequencies are specified in Hz for Terrestrial and Cable systems.
/// The frequencies are specified in kHz for Satellite systems.
#[repr(C)]
#[derive(Debug)]
pub struct FeInfo {
    /// Name of the frontend
    pub name: [u8; 128],
    /// DEPRECATED: frontend delivery system
    pub fe_type: u32,
    /// Minimal frequency supported by the frontend
    pub frequency_min: u32,
    /// Maximal frequency supported by the frontend
    pub frequency_max: u32,
    /// All frequencies are multiple of this value
    pub frequency_stepsize: u32,
    /// Frequency tolerance
    pub frequency_tolerance: u32,
    /// Minimal symbol rate, in bauds (for Cable/Satellite systems)
    pub symbol_rate_min: u32,
    /// Maximal symbol rate, in bauds (for Cable/Satellite systems)
    pub symbol_rate_max: u32,
    /// Maximal symbol rate tolerance, in ppm (for Cable/Satellite systems)
    pub symbol_rate_tolerance: u32,
    /// DEPRECATED
    pub notifier_delay: u32,
    /// Capabilities supported by the frontend
    pub caps: u32,
}


impl Default for FeInfo {
    #[inline]
    fn default() -> Self { unsafe { mem::zeroed::<Self>() } }
}


impl FeInfo {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut FeInfo { self as *mut _ }
}


/// DiSEqC master command
/// Check out the DiSEqC bus spec available on http://www.eutelsat.org/ for
/// the possible messages that can be used.
#[repr(C)]
#[derive(Debug)]
pub struct DiseqcMasterCmd {
    /// DiSEqC message to be sent. It contains a 3 bytes header with:
    /// framing + address + command, and an optional argument
    /// of up to 3 bytes of data.
    pub msg: [u8; 6],
    /// Length of the DiSEqC message. Valid values are 3 to 6.
    pub len: u8,
}


impl Default for DiseqcMasterCmd {
    #[inline]
    fn default() -> Self { unsafe { mem::zeroed::<Self>() } }
}


impl DiseqcMasterCmd {
    #[inline]
    pub fn as_ptr(&self) -> *const DiseqcMasterCmd { self as *const _ }
}


/// DiSEqC received data
#[repr(C)]
#[derive(Debug)]
pub struct DiseqcSlaveReply {
    /// DiSEqC message buffer to store a message received via DiSEqC.
    /// It contains one byte header with: framing and
    /// an optional argument of up to 3 bytes of data.
    pub msg: [u8; 4],
    /// Length of the DiSEqC message. Valid values are 0 to 4,
    /// where 0 means no message.
    pub len: u8,
    /// Return from ioctl after timeout ms with errorcode when
    /// no message was received.
    pub timeout: u32,
}


impl Default for DiseqcSlaveReply {
    #[inline]
    fn default() -> Self { unsafe { mem::zeroed::<Self>() } }
}


/// DC Voltage used to feed the LNBf
mod fe_sec_voltage {
    /// Output 13V to the LNB. Vertical linear. Right circular.
    pub const SEC_VOLTAGE_13: u32               = 0;
    /// Output 18V to the LNB. Horizontal linear. Left circular.
    pub const SEC_VOLTAGE_18: u32               = 1;
    /// Don't feed the LNB with a DC voltage
    pub const SEC_VOLTAGE_OFF: u32              = 2;
}


mod fe_sec_tone_mode {
    /// Sends a 22kHz tone burst to the antenna
    pub const SEC_TONE_ON: u32                  = 0;
    /// Don't send a 22kHz tone to the antenna (except if the FE_DISEQC_* ioctl are called)
    pub const SEC_TONE_OFF: u32                 = 1;
}


/// Type of mini burst to be sent
mod fe_sec_mini_cmd {
    /// Sends a mini-DiSEqC 22kHz '0' Tone Burst to select satellite-A
    pub const SEC_MINI_A: u32                   = 0;
    /// Sends a mini-DiSEqC 22kHz '1' Data Burst to select satellite-B
    pub const SEC_MINI_B: u32                   = 1;
}


/// Enumerates the possible frontend status
mod fe_status {
    /// The frontend doesn't have any kind of lock. That's the initial frontend status
    pub const FE_NONE: u32                      = 0x00;
    /// Has found something above the noise level
    pub const FE_HAS_SIGNAL: u32                = 0x01;
    /// Has found a signal
    pub const FE_HAS_CARRIER: u32               = 0x02;
    /// FEC inner coding (Viterbi, LDPC or other inner code) is stable.
    pub const FE_HAS_VITERBI: u32               = 0x04;
    /// Synchronization bytes was found
    pub const FE_HAS_SYNC: u32                  = 0x08;
    /// Digital TV were locked and everything is working
    pub const FE_HAS_LOCK: u32                  = 0x10;
    /// Fo lock within the last about 2 seconds
    pub const FE_TIMEDOUT: u32                  = 0x20;
    /// Frontend was reinitialized, application is recommended
    /// to reset DiSEqC, tone and parameters
    pub const FE_REINIT: u32                    = 0x40;
}


/// Spectral band inversion
mod fe_spectral_inversion {
    pub const INVERSION_OFF: u32                = 0;
    pub const INVERSION_ON: u32                 = 1;
    pub const INVERSION_AUTO: u32               = 2;
}


mod fe_code_rate {
    pub const FEC_NONE: u32                     = 0;
    pub const FEC_1_2: u32                      = 1;
    pub const FEC_2_3: u32                      = 2;
    pub const FEC_3_4: u32                      = 3;
    pub const FEC_4_5: u32                      = 4;
    pub const FEC_5_6: u32                      = 5;
    pub const FEC_6_7: u32                      = 6;
    pub const FEC_7_8: u32                      = 7;
    pub const FEC_8_9: u32                      = 8;
    pub const FEC_AUTO: u32                     = 9;
    pub const FEC_3_5: u32                      = 10;
    pub const FEC_9_10: u32                     = 11;
    pub const FEC_2_5: u32                      = 12;
    pub const FEC_1_4: u32                      = 13;
    pub const FEC_1_3: u32                      = 14;
}


/// Type of modulation/constellation
mod fe_modulation {
    pub const QPSK: u32                         = 0;
    pub const QAM_16: u32                       = 1;
    pub const QAM_32: u32                       = 2;
    pub const QAM_64: u32                       = 3;
    pub const QAM_128: u32                      = 4;
    pub const QAM_256: u32                      = 5;
    pub const QAM_AUTO: u32                     = 6;
    pub const VSB_8: u32                        = 7;
    pub const VSB_16: u32                       = 8;
    pub const PSK_8: u32                        = 9;
    pub const APSK_16: u32                      = 10;
    pub const APSK_32: u32                      = 11;
    pub const DQPSK: u32                        = 12;
    pub const QAM_4_NR: u32                     = 13;
    pub const APSK_64: u32                      = 14;
    pub const APSK_128: u32                     = 15;
    pub const APSK_256: u32                     = 16;
}


mod fe_transmit_mode {
    pub const TRANSMISSION_MODE_2K: u32         = 0;
    pub const TRANSMISSION_MODE_8K: u32         = 1;
    pub const TRANSMISSION_MODE_AUTO: u32       = 2;
    pub const TRANSMISSION_MODE_4K: u32         = 3;
    pub const TRANSMISSION_MODE_1K: u32         = 4;
    pub const TRANSMISSION_MODE_16K: u32        = 5;
    pub const TRANSMISSION_MODE_32K: u32        = 6;
    pub const TRANSMISSION_MODE_C1: u32         = 7;
    pub const TRANSMISSION_MODE_C3780: u32      = 8;
}


mod fe_guard_interval {
    pub const GUARD_INTERVAL_1_32: u32          = 0;
    pub const GUARD_INTERVAL_1_16: u32          = 1;
    pub const GUARD_INTERVAL_1_8: u32           = 2;
    pub const GUARD_INTERVAL_1_4: u32           = 3;
    pub const GUARD_INTERVAL_AUTO: u32          = 4;
    pub const GUARD_INTERVAL_1_128: u32         = 5;
    pub const GUARD_INTERVAL_19_128: u32        = 6;
    pub const GUARD_INTERVAL_19_256: u32        = 7;
    pub const GUARD_INTERVAL_PN420: u32         = 8;
    pub const GUARD_INTERVAL_PN595: u32         = 9;
    pub const GUARD_INTERVAL_PN945: u32         = 10;
}


mod fe_hierarchy {
    pub const HIERARCHY_NONE: u32               = 0;
    pub const HIERARCHY_1: u32                  = 1;
    pub const HIERARCHY_2: u32                  = 2;
    pub const HIERARCHY_4: u32                  = 3;
    pub const HIERARCHY_AUTO: u32               = 4;
}


mod fe_interleaving {
    pub const INTERLEAVING_NONE: u32            = 0;
    pub const INTERLEAVING_AUTO: u32            = 1;
    pub const INTERLEAVING_240: u32             = 2;
    pub const INTERLEAVING_720: u32             = 3;
}


mod fe_pilot {
    pub const PILOT_ON: u32                     = 0;
    pub const PILOT_OFF: u32                    = 1;
    pub const PILOT_AUTO: u32                   = 2;
}


mod fe_rolloff {
    pub const ROLLOFF_35: u32                   = 0;
    pub const ROLLOFF_20: u32                   = 1;
    pub const ROLLOFF_25: u32                   = 2;
    pub const ROLLOFF_AUTO: u32                 = 3;
    pub const ROLLOFF_15: u32                   = 4;
    pub const ROLLOFF_10: u32                   = 5;
    pub const ROLLOFF_5: u32                    = 6;
}


mod fe_delivery_system {
    use std::fmt;

    pub const SYS_UNDEFINED: u32                = 0;
    pub const SYS_DVBC_ANNEX_A: u32             = 1;
    pub const SYS_DVBC_ANNEX_B: u32             = 2;
    pub const SYS_DVBT: u32                     = 3;
    pub const SYS_DSS: u32                      = 4;
    pub const SYS_DVBS: u32                     = 5;
    pub const SYS_DVBS2: u32                    = 6;
    pub const SYS_DVBH: u32                     = 7;
    pub const SYS_ISDBT: u32                    = 8;
    pub const SYS_ISDBS: u32                    = 9;
    pub const SYS_ISDBC: u32                    = 10;
    pub const SYS_ATSC: u32                     = 11;
    pub const SYS_ATSCMH: u32                   = 12;
    pub const SYS_DTMB: u32                     = 13;
    pub const SYS_CMMB: u32                     = 14;
    pub const SYS_DAB: u32                      = 15;
    pub const SYS_DVBT2: u32                    = 16;
    pub const SYS_TURBO: u32                    = 17;
    pub const SYS_DVBC_ANNEX_C: u32             = 18;
    pub const SYS_DVBC2: u32                    = 19;

    pub struct DeliverySystemDisplay(pub u32);

    impl fmt::Display for DeliverySystemDisplay {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let v = match self.0 {
                SYS_UNDEFINED => "none",
                SYS_DVBC_ANNEX_A => "dvb-c",
                SYS_DVBC_ANNEX_B => "dvb-c/b",
                SYS_DVBT => "dvb-t",
                SYS_DSS => "dss",
                SYS_DVBS => "dvb-s",
                SYS_DVBS2 => "dvb-s2",
                SYS_DVBH => "dvb-h",
                SYS_ISDBT => "isdb-t",
                SYS_ISDBS => "isdb-s",
                SYS_ISDBC => "isdb-c",
                SYS_ATSC => "atsc",
                SYS_ATSCMH => "atsc-m/h",
                SYS_DTMB => "dtmb",
                SYS_CMMB => "cmmb",
                SYS_DAB => "dab",
                SYS_DVBT2 => "dvb-t2",
                SYS_TURBO => "dvb-s/turbo",
                SYS_DVBC_ANNEX_C => "dvb-c/c",
                SYS_DVBC2 => "dvb-c2",
                _ => "unknown",
            };

            write!(f, "{}", v)
        }
    }
}


/// scale types for the quality parameters
mod fecap_scale_params {
    /// That QoS measure is not available. That could indicate
    /// a temporary or a permanent condition.
    pub const FE_SCALE_NOT_AVAILABLE: u8       = 0;
    /// The scale is measured in 0.001 dB steps, typically used on signal measures.
    pub const FE_SCALE_DECIBEL: u8             = 1;
    /// The scale is a relative percentual measure,
    /// ranging from 0 (0%) to 0xffff (100%).
    pub const FE_SCALE_RELATIVE: u8            = 2;
    /// The scale counts the occurrence of an event, like
    /// bit error, block error, lapsed time.
    pub const FE_SCALE_COUNTER: u8             = 3;
}


/// Used for reading a DTV status property
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct DtvStats {
    pub scale: u8, // fecap_scale_params
    pub value: i64,
}


impl fmt::Debug for DtvStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("DtvStats");
        s.field("scale", &{self.scale});
        match self.scale {
            FE_SCALE_NOT_AVAILABLE => s.field("value", &"not available"),
            FE_SCALE_DECIBEL => s.field("value", &{self.value}),
            FE_SCALE_RELATIVE => s.field("value", &{self.value as u64}),
            FE_SCALE_COUNTER => s.field("value", &{self.value as u64}),
            _ => s.field("value", &"invalid scale format"),
        };
        s.finish()
    }
}


pub const MAX_DTV_STATS: usize = 4;


/// Store Digital TV frontend statistics
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct DtvFrontendStats {
    pub len: u8,
    pub stat: [DtvStats; MAX_DTV_STATS],
}


impl DtvFrontendStats {
    pub fn get_counter(&self) -> Option<u64> {
        for i in 0 .. ::std::cmp::min(self.len as usize, self.stat.len()) {
            let s = &self.stat[i];
            if s.scale == FE_SCALE_COUNTER {
                return Some(s.value as u64);
            }
        }

        None
    }

    pub fn get_decibel(&self) -> Option<f64> {
        for i in 0 .. ::std::cmp::min(self.len as usize, self.stat.len()) {
            let s = &self.stat[i];
            if s.scale == FE_SCALE_DECIBEL {
                return Some((s.value as f64) / 1000.0);
            }
        }

        None
    }
}


#[repr(C)]
#[derive(Copy, Clone)]
pub struct DtvPropertyBuffer {
    pub data: [u8; 32],
    pub len: u32,
    __reserved_1: [u32; 3],
    __reserved_2: *mut libc::c_void,
}


#[repr(C)]
pub union DtvPropertyData {
    pub data: u32,
    pub st: DtvFrontendStats,
    pub buffer: DtvPropertyBuffer,
    __align: [u8; 56],
}


/// DVBv5 property Commands
mod dtv_property_cmd {
    pub const DTV_UNDEFINED: u32                        = 0;
    pub const DTV_TUNE: u32                             = 1;
    pub const DTV_CLEAR: u32                            = 2;
    pub const DTV_FREQUENCY: u32                        = 3;
    pub const DTV_MODULATION: u32                       = 4;
    pub const DTV_BANDWIDTH_HZ: u32                     = 5;
    pub const DTV_INVERSION: u32                        = 6;
    pub const DTV_DISEQC_MASTER: u32                    = 7;
    pub const DTV_SYMBOL_RATE: u32                      = 8;
    pub const DTV_INNER_FEC: u32                        = 9;
    pub const DTV_VOLTAGE: u32                          = 10;
    pub const DTV_TONE: u32                             = 11;
    pub const DTV_PILOT: u32                            = 12;
    pub const DTV_ROLLOFF: u32                          = 13;
    pub const DTV_DISEQC_SLAVE_REPLY: u32               = 14;

    /* Basic enumeration set for querying unlimited capabilities */

    pub const DTV_FE_CAPABILITY_COUNT: u32              = 15;
    pub const DTV_FE_CAPABILITY: u32                    = 16;
    pub const DTV_DELIVERY_SYSTEM: u32                  = 17;

    /* ISDB-T and ISDB-Tsb */

    pub const DTV_ISDBT_PARTIAL_RECEPTION: u32          = 18;
    pub const DTV_ISDBT_SOUND_BROADCASTING: u32         = 19;

    pub const DTV_ISDBT_SB_SUBCHANNEL_ID: u32           = 20;
    pub const DTV_ISDBT_SB_SEGMENT_IDX: u32             = 21;
    pub const DTV_ISDBT_SB_SEGMENT_COUNT: u32           = 22;

    pub const DTV_ISDBT_LAYERA_FEC: u32                 = 23;
    pub const DTV_ISDBT_LAYERA_MODULATION: u32          = 24;
    pub const DTV_ISDBT_LAYERA_SEGMENT_COUNT: u32       = 25;
    pub const DTV_ISDBT_LAYERA_TIME_INTERLEAVING: u32   = 26;

    pub const DTV_ISDBT_LAYERB_FEC: u32                 = 27;
    pub const DTV_ISDBT_LAYERB_MODULATION: u32          = 28;
    pub const DTV_ISDBT_LAYERB_SEGMENT_COUNT: u32       = 29;
    pub const DTV_ISDBT_LAYERB_TIME_INTERLEAVING: u32   = 30;

    pub const DTV_ISDBT_LAYERC_FEC: u32                 = 31;
    pub const DTV_ISDBT_LAYERC_MODULATION: u32          = 32;
    pub const DTV_ISDBT_LAYERC_SEGMENT_COUNT: u32       = 33;
    pub const DTV_ISDBT_LAYERC_TIME_INTERLEAVING: u32   = 34;

    pub const DTV_API_VERSION: u32                      = 35;

    /* DVB-T/T2 */

    pub const DTV_CODE_RATE_HP: u32                     = 36;
    pub const DTV_CODE_RATE_LP: u32                     = 37;
    pub const DTV_GUARD_INTERVAL: u32                   = 38;
    pub const DTV_TRANSMISSION_MODE: u32                = 39;
    pub const DTV_HIERARCHY: u32                        = 40;

    pub const DTV_ISDBT_LAYER_ENABLED: u32              = 41;

    pub const DTV_STREAM_ID: u32                        = 42;
    pub const DTV_DVBT2_PLP_ID_LEGACY: u32              = 43;

    pub const DTV_ENUM_DELSYS: u32                      = 44;

    /* ATSC-MH */

    pub const DTV_ATSCMH_FIC_VER: u32                   = 45;
    pub const DTV_ATSCMH_PARADE_ID: u32                 = 46;
    pub const DTV_ATSCMH_NOG: u32                       = 47;
    pub const DTV_ATSCMH_TNOG: u32                      = 48;
    pub const DTV_ATSCMH_SGN: u32                       = 49;
    pub const DTV_ATSCMH_PRC: u32                       = 50;
    pub const DTV_ATSCMH_RS_FRAME_MODE: u32             = 51;
    pub const DTV_ATSCMH_RS_FRAME_ENSEMBLE: u32         = 52;
    pub const DTV_ATSCMH_RS_CODE_MODE_PRI: u32          = 53;
    pub const DTV_ATSCMH_RS_CODE_MODE_SEC: u32          = 54;
    pub const DTV_ATSCMH_SCCC_BLOCK_MODE: u32           = 55;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_A: u32          = 56;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_B: u32          = 57;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_C: u32          = 58;
    pub const DTV_ATSCMH_SCCC_CODE_MODE_D: u32          = 59;

    pub const DTV_INTERLEAVING: u32                     = 60;
    pub const DTV_LNA: u32                              = 61;

    /* Quality parameters */

    pub const DTV_STAT_SIGNAL_STRENGTH: u32             = 62;
    pub const DTV_STAT_CNR: u32                         = 63;
    pub const DTV_STAT_PRE_ERROR_BIT_COUNT: u32         = 64;
    pub const DTV_STAT_PRE_TOTAL_BIT_COUNT: u32         = 65;
    pub const DTV_STAT_POST_ERROR_BIT_COUNT: u32        = 66;
    pub const DTV_STAT_POST_TOTAL_BIT_COUNT: u32        = 67;
    pub const DTV_STAT_ERROR_BLOCK_COUNT: u32           = 68;
    pub const DTV_STAT_TOTAL_BLOCK_COUNT: u32           = 69;

    /* Physical layer scrambling */

    pub const DTV_SCRAMBLING_SEQUENCE_INDEX: u32        = 70;
    pub const DTV_INPUT: u32                            = 71;
}


/// Store one of frontend command and its value
#[repr(C, packed)]
pub struct DtvProperty {
    pub cmd: u32,
    __reserved_1: [u32; 3],
    pub u: DtvPropertyData,
    pub result: i32,
}


impl fmt::Debug for DtvProperty {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("DtvProperty");
        s.field("cmd", &{ self.cmd });

        const FNAME: &str = "value";

        match self.cmd {
            DTV_FREQUENCY => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_MODULATION => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_BANDWIDTH_HZ => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_INVERSION => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_SYMBOL_RATE => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_INNER_FEC => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_PILOT => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_ROLLOFF => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_DELIVERY_SYSTEM => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            DTV_API_VERSION => {
                s.field(FNAME, unsafe { &self.u.data });
            }
            // TODO: more values
            _ => {}
        }

        s.field("result", &{ self.result });
        s.finish()
    }
}


impl DtvProperty {
    #[inline]
    pub fn new(cmd: u32, data: u32) -> Self {
        Self {
            cmd,
            __reserved_1: [0, 0, 0],
            u: DtvPropertyData { data },
            result: 0,
        }
    }

    #[inline]
    pub fn get_data(&self) -> u32 {
        unsafe { self.u.data }
    }
}


pub const DTV_MAX_COMMAND: u32                          = DTV_INPUT;


/// num of properties cannot exceed DTV_IOCTL_MAX_MSGS per ioctl
pub const DTV_IOCTL_MAX_MSGS: usize                     = 64;


/// a set of command/value pairs for FE_SET_PROPERTY
#[repr(C)]
#[derive(Debug)]
pub struct DtvProperties {
    pub num: u32,
    pub props: *const DtvProperty,
}


impl DtvProperties {
    #[inline]
    pub fn new(props: &[DtvProperty]) -> DtvProperties {
        DtvProperties {
            num: props.len() as u32,
            props: props.as_ptr(),
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const DtvProperties { self as *const _ }
}


#[repr(C)]
#[derive(Debug)]
pub struct FeParameters {
    /// (absolute) frequency in Hz for DVB-C/DVB-T/ATSC
    /// intermediate frequency in kHz for DVB-S
    pub frequency: u32,
    pub inversion: u32,
    /// unimplemented frontend parameters data
    __reserved_1: [u8; 28],
}


pub const FE_MAX_EVENT: usize = 8;


#[repr(C)]
#[derive(Debug)]
pub struct FeEvent {
    pub status: u32,
    pub parameters: FeParameters,
}


impl Default for FeEvent {
    #[inline]
    fn default() -> Self { unsafe { mem::zeroed::<Self>() } }
}


impl FeEvent {
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut FeEvent { self as *mut _ }
}


pub const FE_GET_INFO: IoctlInt = io_read::<FeInfo>(b'o', 61);

pub const FE_DISEQC_RESET_OVERLOAD: IoctlInt = io_none(b'o', 62);
pub const FE_DISEQC_SEND_MASTER_CMD: IoctlInt = io_write::<DiseqcMasterCmd>(b'o', 63);
pub const FE_DISEQC_RECV_SLAVE_REPLY: IoctlInt = io_read::<DiseqcSlaveReply>(b'0', 64);
pub const FE_DISEQC_SEND_BURST: IoctlInt = io_none(b'o', 65);

pub const FE_SET_TONE: IoctlInt = io_none(b'o', 66);
pub const FE_SET_VOLTAGE: IoctlInt = io_none(b'o', 67);
pub const FE_ENABLE_HIGH_LNB_VOLTAGE: IoctlInt = io_none(b'o', 68);

pub const FE_READ_STATUS: IoctlInt = io_read::<u32>(b'o', 69);
pub const FE_READ_BER: IoctlInt = io_read::<u32>(b'o', 70);
pub const FE_READ_SIGNAL_STRENGTH: IoctlInt = io_read::<u16>(b'o', 71);
pub const FE_READ_SNR: IoctlInt = io_read::<u16>(b'o', 72);
pub const FE_READ_UNCORRECTED_BLOCKS: IoctlInt = io_read::<u32>(b'o', 73);

pub const FE_GET_EVENT: IoctlInt = io_read::<FeEvent>(b'o', 78);
pub const FE_SET_FRONTEND_TUNE_MODE: IoctlInt = io_none(b'o', 81);

pub const FE_SET_PROPERTY: IoctlInt = io_write::<DtvProperties>(b'o', 82);
pub const FE_GET_PROPERTY: IoctlInt = io_read::<DtvProperties>(b'o', 83);
