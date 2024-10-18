pub use {
    feed_type::*,
};


mod feed_type {
    /// Multi Protocol Encapsulation (MPE) encoding
    pub const DVB_NET_FEEDTYPE_MPE: u8 = 0;
    /// Ultra Lightweight Encapsulation (ULE) encoding
    pub const DVB_NET_FEEDTYPE_ULE: u8 = 1;
}


/// Describes a DVB network interface
/// Configures adapter to decapsulate IP packets from MPEG-TS stream
#[repr(C)]
#[derive(Debug)]
pub struct DvbNetIf {
    /// Packet ID (PID) of the MPEG-TS that contains data
    pub pid: u16,
    /// Number of the Digital TV interface
    pub if_num: u16,
    /// Encapsulation type of the feed
    pub feedtype: u8,
}
