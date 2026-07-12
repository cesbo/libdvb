//! en50221 8: the resource layer
//!
//! A resource is a host-side application-layer service the module opens a
//! session to. Every host-supported resource is registered in the
//! [`ResourceRegistry`].

pub(super) mod application_info;
pub(super) mod conditional_access;
pub(super) mod date_time;
pub(super) mod host_control;
pub(super) mod mmi;
pub(super) mod rm;

use std::{
    collections::VecDeque,
    fmt,
    time::Instant,
};

pub use self::{
    application_info::ApplicationInfo,
    mmi::MmiMenu,
};
use super::{
    apdu::ApduTag,
    session::CaEvent,
    transport::CiTransport,
};
use crate::error::Result;

/// en50221 8.4.1: resource identifier (4 bytes big-endian on the wire)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(u32);

impl ResourceId {
    pub const RESOURCE_MANAGER: Self = Self(0x0001_0041);
    pub const APPLICATION_INFORMATION: Self = Self(0x0002_0041);
    pub const CONDITIONAL_ACCESS_SUPPORT: Self = Self(0x0003_0041);
    pub const HOST_CONTROL: Self = Self(0x0020_0041);
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

/// Sending and eventing context passed to resource callbacks, bound to
/// one session
pub(super) struct ResourceContext<'a> {
    pub transport: &'a mut CiTransport,
    pub events: &'a mut VecDeque<CaEvent>,
    pub slot_id: u8,
    pub session_id: u16,
    /// set by a resource to ask the session layer to close the session
    /// after the callback returns
    pub close_session: bool,
}

impl ResourceContext<'_> {
    /// Sends an APDU on the context session
    pub fn send_apdu(&mut self, tag: ApduTag, body: &[u8]) -> Result<()> {
        self.transport
            .send_apdu(self.slot_id, self.session_id, tag, body)
    }

    /// Queues an event for the application
    pub fn event(&mut self, event: CaEvent) {
        self.events.push_back(event);
    }
}

/// Host-side en50221 resource: session open/close and APDU callbacks
/// driven by the session layer
pub(super) trait Resource {
    /// Resource id offered to modules in the profile reply
    fn resource_id(&self) -> ResourceId;

    /// Called when the module opened a session to the resource
    fn on_open(&mut self, _ctx: &mut ResourceContext<'_>) -> Result<()> {
        Ok(())
    }

    /// Called for every APDU arriving on a session bound to the resource.
    /// An `InvalidData` error is turned into a [`CaEvent::Malformed`]
    /// event by the session layer, other errors are fatal.
    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()>;

    /// Called when the session is gone: module close request, host close
    /// completion or slot drop
    fn on_close(&mut self, _slot_id: u8, _session_id: u16) {}

    /// Called from the session layer tick for time-based work
    fn on_tick(&mut self, _transport: &mut CiTransport, _now: Instant) -> Result<()> {
        Ok(())
    }
}

/// Resource ids offered to modules in the profile reply
const HOST_PROFILE: [ResourceId; 6] = [
    ResourceId::RESOURCE_MANAGER,
    ResourceId::APPLICATION_INFORMATION,
    ResourceId::CONDITIONAL_ACCESS_SUPPORT,
    ResourceId::HOST_CONTROL,
    ResourceId::DATE_TIME,
    ResourceId::MMI,
];

/// All host-supported resources with dispatch by resource id
pub(super) struct ResourceRegistry {
    pub rm: rm::ResourceManager,
    pub application_info: application_info::ApplicationInfoResource,
    pub conditional_access: conditional_access::ConditionalAccessResource,
    pub host_control: host_control::HostControlResource,
    pub date_time: date_time::DateTimeResource,
    pub mmi: mmi::MmiResource,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        ResourceRegistry {
            rm: rm::ResourceManager::new(&HOST_PROFILE),
            application_info: application_info::ApplicationInfoResource::new(),
            conditional_access: conditional_access::ConditionalAccessResource::new(),
            host_control: host_control::HostControlResource,
            date_time: date_time::DateTimeResource::new(),
            mmi: mmi::MmiResource::new(),
        }
    }

    fn resources(&mut self) -> [&mut dyn Resource; 6] {
        [
            &mut self.rm,
            &mut self.application_info,
            &mut self.conditional_access,
            &mut self.host_control,
            &mut self.date_time,
            &mut self.mmi,
        ]
    }

    /// Finds the resource with the same class and type, version-agnostic
    pub fn lookup(&mut self, resource_id: ResourceId) -> Option<&mut dyn Resource> {
        self.resources()
            .into_iter()
            .find(|resource| resource.resource_id().base() == resource_id.base())
    }

    /// Runs time-based work of all resources
    pub fn tick(&mut self, transport: &mut CiTransport, now: Instant) -> Result<()> {
        for resource in self.resources() {
            resource.on_tick(transport, now)?;
        }

        Ok(())
    }
}
