//! en50221 8: the resource layer
//!
//! A resource is a host-side application-layer service the module opens a
//! session to. Every host-supported resource is registered in the
//! [`ResourceRegistry`].

use std::fmt;

/// en50221 8.4.1: resource identifier (4 bytes big-endian on the wire)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(u32);

impl ResourceId {
    pub const RESOURCE_MANAGER: Self = Self(0x0001_0041);
    pub const APPLICATION_INFORMATION: Self = Self(0x0002_0041);
    /// Conditional Access support (future)
    pub const CONDITIONAL_ACCESS_SUPPORT: Self = Self(0x0003_0041);
    /// Host Control (future)
    pub const HOST_CONTROL: Self = Self(0x0020_0041);
    /// Date-Time (future)
    pub const DATE_TIME: Self = Self(0x0024_0041);
    pub const MMI: Self = Self(0x0040_0041);

    /// Wraps a raw resource id
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Raw resource id
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Resource class and type with the version bits masked out
    pub const fn base(self) -> u32 {
        self.0 >> 6
    }

    /// Resource version (the low 6 bits)
    pub const fn version(self) -> u8 {
        (self.0 & 0x3F) as u8
    }
}

impl fmt::Debug for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::RESOURCE_MANAGER => "RESOURCE_MANAGER",
            Self::APPLICATION_INFORMATION => "APPLICATION_INFORMATION",
            Self::CONDITIONAL_ACCESS_SUPPORT => "CONDITIONAL_ACCESS_SUPPORT",
            Self::HOST_CONTROL => "HOST_CONTROL",
            Self::DATE_TIME => "DATE_TIME",
            Self::MMI => "MMI",
            _ => return write!(f, "ResourceId(0x{:08X})", self.0),
        };

        write!(f, "ResourceId({})", name)
    }
}
