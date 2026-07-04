//! en50221 8.4.1: Resource Manager resource
//!
//! The module opens a session to the resource manager right after the
//! transport connection is created. The host interrogates the module
//! profile and announces its own resource list:
//!
//! - session open: the host sends profile_enq
//! - profile (the module resource list): the host replies profile_change
//! - profile_enq from the module: the host replies profile with the host
//!   resource list
//! - profile_change from the module: the host re-enquires with
//!   profile_enq

use super::{
    super::apdu::ApduTag,
    Resource,
    ResourceContext,
    ResourceId,
};
use crate::error::{
    Error,
    Result,
};

/// Resource Manager resource
pub struct ResourceManager {
    /// encoded host resource ids for the profile reply
    profile: Vec<u8>,
}

impl ResourceManager {
    pub fn new(host_profile: &[ResourceId]) -> Self {
        let mut profile = Vec::with_capacity(host_profile.len() * 4);
        for resource_id in host_profile {
            let raw = resource_id.raw();
            profile.push((raw >> 24) as u8);
            profile.push((raw >> 16) as u8);
            profile.push((raw >> 8) as u8);
            profile.push(raw as u8);
        }

        ResourceManager { profile }
    }
}

impl Resource for ResourceManager {
    fn resource_id(&self) -> ResourceId {
        ResourceId::RESOURCE_MANAGER
    }

    fn on_open(&mut self, ctx: &mut ResourceContext<'_>) -> Result<()> {
        ctx.send_apdu(ApduTag::PROFILE_ENQ, &[])
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, _body: &[u8]) -> Result<()> {
        match tag {
            // the module profile content is not used by the host
            ApduTag::PROFILE => ctx.send_apdu(ApduTag::PROFILE_CHANGE, &[]),
            ApduTag::PROFILE_ENQ => ctx.send_apdu(ApduTag::PROFILE, &self.profile),
            // the module profile changed: re-enquire
            ApduTag::PROFILE_CHANGE => ctx.send_apdu(ApduTag::PROFILE_ENQ, &[]),
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected resource manager apdu tag {:?}",
                ctx.slot_id, tag
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_coding() {
        let rm = ResourceManager::new(&[ResourceId::RESOURCE_MANAGER, ResourceId::MMI]);
        assert_eq!(
            rm.profile,
            vec![0x00, 0x01, 0x00, 0x41, 0x00, 0x40, 0x00, 0x41]
        );
    }
}
