//! en50221 8.4.3: Conditional Access Support resource
//!
//! A Conditional Access application reports the CA systems it supports
//! after the host sends `ca_info_enq`. The information belongs to the
//! resource session: a module may expose more than one CA application in
//! the same slot, each with a different CAID list.

use std::collections::{
    BTreeMap,
    HashMap,
};

use crate::{
    ca::{
        ApduTag,
        capmt::{
            CaPmtCommand,
            CaPmtListManagement,
            Program,
        },
        resource::{
            Resource,
            ResourceContext,
            ResourceId,
        },
        session::CaEvent,
        transport::CiTransport,
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
    /// Last program state sent to this resource session.
    selected: BTreeMap<u16, Program>,
}

/// Conditional Access Support resource
pub struct ConditionalAccessResource {
    sessions: HashMap<u16, ConditionalAccessSession>,
    /// Desired programs outlive resource sessions so CAM reconnection can
    /// restore the complete selection after the next CA_INFO.
    programs: BTreeMap<u16, Program>,
}

impl ConditionalAccessResource {
    pub fn new() -> Self {
        ConditionalAccessResource {
            sessions: HashMap::new(),
            programs: BTreeMap::new(),
        }
    }

    /// Confirmed CAID list of one live resource session
    pub fn session_caids(&self, slot_id: u8, session_id: u16) -> Option<&[u16]> {
        self.sessions
            .get(&session_id)
            .filter(|session| session.slot_id == slot_id)
            .and_then(|session| session.caids.as_deref())
    }

    /// Adds or replaces a desired program and updates every CA application
    /// whose confirmed CAID list matches at least one descriptor in its PMT.
    pub fn set_program(
        &mut self,
        transport: &mut CiTransport,
        program: Program,
    ) -> Result<Vec<u8>> {
        let program_number = program.program_number();
        if self.programs.get(&program_number) == Some(&program) {
            return Ok(Vec::new());
        }
        self.programs.insert(program_number, program.clone());

        let mut session_ids: Vec<u16> = self.sessions.keys().copied().collect();
        session_ids.sort_unstable();
        let mut touched_slots = Vec::new();

        for session_id in session_ids {
            let session = self.sessions.get_mut(&session_id).expect("known session");
            let Some(caids) = session.caids.as_deref() else {
                continue;
            };
            let previous = session.selected.get(&program_number).cloned();
            let list_management = if previous.is_some() {
                CaPmtListManagement::Update
            } else if session.selected.is_empty() {
                CaPmtListManagement::Only
            } else {
                CaPmtListManagement::Add
            };

            if let Some(body) =
                program.build_ca_pmt(caids, list_management, CaPmtCommand::OkDescrambling)?
            {
                transport.send_apdu(session.slot_id, session_id, ApduTag::CA_PMT, &body)?;
                session.selected.insert(program_number, program.clone());
                touched_slots.push(session.slot_id);
            } else if let Some(previous) = previous {
                let body = previous
                    .build_ca_pmt(
                        caids,
                        CaPmtListManagement::Update,
                        CaPmtCommand::NotSelected,
                    )?
                    .ok_or_else(|| {
                        Error::InvalidData(format!(
                            "ca slot {}: selected program {} no longer has a matching CA descriptor",
                            session.slot_id, program_number
                        ))
                    })?;
                transport.send_apdu(session.slot_id, session_id, ApduTag::CA_PMT, &body)?;
                session.selected.remove(&program_number);
                touched_slots.push(session.slot_id);
            }
        }

        touched_slots.sort_unstable();
        touched_slots.dedup();
        Ok(touched_slots)
    }

    /// Removes a desired program and sends NOT_SELECTED to every CA
    /// application to which it had previously been selected.
    pub fn remove_program(
        &mut self,
        transport: &mut CiTransport,
        program_number: u16,
    ) -> Result<Vec<u8>> {
        if self.programs.remove(&program_number).is_none() {
            return Ok(Vec::new());
        }

        let mut session_ids: Vec<u16> = self.sessions.keys().copied().collect();
        session_ids.sort_unstable();
        let mut touched_slots = Vec::new();

        for session_id in session_ids {
            let session = self.sessions.get_mut(&session_id).expect("known session");
            let Some(program) = session.selected.get(&program_number).cloned() else {
                continue;
            };
            let caids = session.caids.as_deref().ok_or_else(|| {
                Error::InvalidData(format!(
                    "ca slot {}: selected program without confirmed CA_INFO",
                    session.slot_id
                ))
            })?;
            let body = program
                .build_ca_pmt(
                    caids,
                    CaPmtListManagement::Update,
                    CaPmtCommand::NotSelected,
                )?
                .ok_or_else(|| {
                    Error::InvalidData(format!(
                        "ca slot {}: selected program {} has no matching CA descriptor",
                        session.slot_id, program_number
                    ))
                })?;
            transport.send_apdu(session.slot_id, session_id, ApduTag::CA_PMT, &body)?;
            session.selected.remove(&program_number);
            touched_slots.push(session.slot_id);
        }

        touched_slots.sort_unstable();
        touched_slots.dedup();
        Ok(touched_slots)
    }

    fn synchronize_session(
        programs: &BTreeMap<u16, Program>,
        session: &mut ConditionalAccessSession,
        ctx: &mut ResourceContext<'_>,
        caids: Vec<u16>,
    ) -> Result<()> {
        let old_caids = session.caids.replace(caids.clone());
        let previous = std::mem::take(&mut session.selected);
        let mut selected = BTreeMap::new();

        for (&program_number, program) in programs {
            let list_management = if selected.is_empty() {
                CaPmtListManagement::Only
            } else {
                CaPmtListManagement::Add
            };
            if let Some(body) =
                program.build_ca_pmt(&caids, list_management, CaPmtCommand::OkDescrambling)?
            {
                ctx.send_apdu(ApduTag::CA_PMT, &body)?;
                selected.insert(program_number, program.clone());
            }
        }

        // A changed CA_INFO can invalidate programs selected using the old
        // CAID list. Explicitly withdraw them if no replacement selection
        // was possible.
        if selected.is_empty()
            && let Some(old_caids) = old_caids
        {
            for (_, program) in previous {
                if let Some(body) = program.build_ca_pmt(
                    &old_caids,
                    CaPmtListManagement::Update,
                    CaPmtCommand::NotSelected,
                )? {
                    ctx.send_apdu(ApduTag::CA_PMT, &body)?;
                }
            }
        }

        session.selected = selected;
        Ok(())
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
                selected: BTreeMap::new(),
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
                Self::synchronize_session(&self.programs, session, ctx, caids.clone())?;
                ctx.event(CaEvent::CaInfo {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    caids,
                });

                Ok(())
            }
            ApduTag::CA_PMT_REPLY | ApduTag::CA_UPDATE => Ok(()),
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
