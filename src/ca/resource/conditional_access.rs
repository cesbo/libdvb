//! en50221 8.4.3: Conditional Access Support resource
//!
//! A Conditional Access application reports the CA systems it supports
//! after the host sends `ca_info_enq`. The information belongs to the
//! resource session: a module may expose more than one CA application in
//! the same slot, each with a different CAID list.

use std::collections::HashMap;

use crate::{
    ca::{
        ApduTag,
        resource::{
            Resource,
            ResourceContext,
            ResourceId,
        },
        session::CaEvent,
    },
    error::{
        Error,
        Result,
    },
};

struct ConditionalAccessSession {
    slot_id: u8,
    /// `None` means the session is open but CA_INFO has not arrived yet.
    /// An empty CA_INFO is a valid, confirmed list.
    caids: Option<Vec<u16>>,
}

/// Conditional Access Support resource
pub struct ConditionalAccessResource {
    sessions: HashMap<u16, ConditionalAccessSession>,
}

impl ConditionalAccessResource {
    pub fn new() -> Self {
        ConditionalAccessResource {
            sessions: HashMap::new(),
        }
    }

    /// Confirmed CAID list of one live resource session
    pub fn session_caids(&self, slot_id: u8, session_id: u16) -> Option<&[u16]> {
        self.sessions
            .get(&session_id)
            .filter(|session| session.slot_id == slot_id)
            .and_then(|session| session.caids.as_deref())
    }
}

fn parse_ca_info(slot_id: u8, body: &[u8]) -> Result<Vec<u16>> {
    if !body.len().is_multiple_of(2) {
        return Err(Error::InvalidData(format!(
            "ca slot {}: ca_info has an odd body length {}",
            slot_id,
            body.len()
        )));
    }

    Ok(body
        .chunks_exact(2)
        .map(|bytes| (u16::from(bytes[0]) << 8) | u16::from(bytes[1]))
        .collect())
}

impl Resource for ConditionalAccessResource {
    fn resource_id(&self) -> ResourceId {
        ResourceId::CONDITIONAL_ACCESS_SUPPORT
    }

    fn on_open(&mut self, ctx: &mut ResourceContext<'_>) -> Result<()> {
        self.sessions.insert(
            ctx.session_id,
            ConditionalAccessSession {
                slot_id: ctx.slot_id,
                caids: None,
            },
        );

        ctx.send_apdu(ApduTag::CA_INFO_ENQ, &[])
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()> {
        match tag {
            ApduTag::CA_INFO => {
                let caids = parse_ca_info(ctx.slot_id, body)?;
                let session = self.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
                    Error::InvalidData(format!(
                        "ca slot {}: ca_info on unknown resource session {}",
                        ctx.slot_id, ctx.session_id
                    ))
                })?;
                session.caids = Some(caids.clone());
                ctx.event(CaEvent::CaInfo {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    caids,
                });

                Ok(())
            }
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected conditional access apdu tag {:?}",
                ctx.slot_id, tag
            ))),
        }
    }

    fn on_close(&mut self, _slot_id: u8, session_id: u16) {
        self.sessions.remove(&session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ca_info() {
        assert_eq!(
            parse_ca_info(0, &[0x01, 0x00, 0x05, 0x00, 0x0B, 0x00]).unwrap(),
            [0x0100, 0x0500, 0x0B00]
        );
        assert_eq!(parse_ca_info(0, &[]).unwrap(), []);
    }

    #[test]
    fn test_parse_ca_info_rejects_odd_length() {
        assert!(parse_ca_info(2, &[0x01]).is_err());
        assert!(parse_ca_info(2, &[0x01, 0x00, 0xFF]).is_err());
    }
}
