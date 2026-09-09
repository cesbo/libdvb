//! en50221 8.4.3: Conditional Access Support resource
//!
//! A Conditional Access application reports the CA systems it supports
//! after the host sends `ca_info_enq`. The information belongs to the
//! resource session: a module may expose more than one CA application in
//! the same slot, each with a different CAID list.

use std::collections::{
    BTreeMap,
    HashMap,
    VecDeque,
};

use crate::{
    ca::{
        ApduTag,
        capmt::{
            CaPmtCommand,
            CaPmtListManagement,
            CaPmtReply,
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
    /// Desired programs outlive resource sessions: after a CA_INFO the
    /// controller replays them through the CA_PMT pacer.
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

    /// Desired programs, in program number order
    pub fn programs(&self) -> impl Iterator<Item = &Program> {
        self.programs.values()
    }

    /// Adds or replaces a desired program and updates every CA application
    /// whose confirmed CAID list matches at least one descriptor in its PMT.
    /// A session that already holds this exact program is left alone, so a
    /// replayed or duplicate select reaches only the sessions missing it.
    pub fn set_program(
        &mut self,
        transport: &mut CiTransport,
        events: &mut VecDeque<CaEvent>,
        program: Program,
    ) -> Result<Vec<u8>> {
        let program_number = program.program_number();
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
            if previous.as_ref() == Some(&program) {
                continue;
            }
            let list_management = if previous.is_some() {
                CaPmtListManagement::Update
            } else if session.selected.is_empty() {
                CaPmtListManagement::Only
            } else {
                CaPmtListManagement::Add
            };

            let sent = if let Some(body) =
                program.build_ca_pmt(caids, list_management, CaPmtCommand::OkDescrambling)?
            {
                transport.send_apdu(session.slot_id, session_id, ApduTag::CA_PMT, &body)?;
                session.selected.insert(program_number, program.clone());
                Some((list_management, CaPmtCommand::OkDescrambling))
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
                Some((CaPmtListManagement::Update, CaPmtCommand::NotSelected))
            } else {
                None
            };

            match sent {
                Some((list_management, command)) => {
                    touched_slots.push(session.slot_id);
                    events.push_back(CaEvent::CaPmt {
                        slot_id: session.slot_id,
                        session_id,
                        program_number,
                        list_management,
                        command,
                    });
                }
                None => events.push_back(CaEvent::CaPmtSkipped {
                    slot_id: session.slot_id,
                    session_id,
                    program_number,
                    caids: caids.to_vec(),
                }),
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
        events: &mut VecDeque<CaEvent>,
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
            events.push_back(CaEvent::CaPmt {
                slot_id: session.slot_id,
                session_id,
                program_number,
                list_management: CaPmtListManagement::Update,
                command: CaPmtCommand::NotSelected,
            });
        }

        touched_slots.sort_unstable();
        touched_slots.dedup();
        Ok(touched_slots)
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
                // a CA application that (re)announces itself holds no
                // selection the host can rely on; nothing is sent here: the
                // controller replays the desired programs through the pacer
                session.caids = Some(caids.clone());
                session.selected.clear();
                ctx.event(CaEvent::CaInfo {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    caids,
                });

                Ok(())
            }
            // an empty reply is a bare acknowledgement
            ApduTag::CA_PMT_REPLY if body.is_empty() => Ok(()),
            ApduTag::CA_PMT_REPLY => {
                let reply = CaPmtReply::parse(body).map_err(|error| {
                    Error::InvalidData(format!("ca slot {}: {error}", ctx.slot_id))
                })?;
                ctx.event(CaEvent::CaPmtReply {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    reply,
                });
                Ok(())
            }
            ApduTag::CA_UPDATE => Ok(()),
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
