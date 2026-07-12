//! en50221 8.4.2: Application Information resource
//!
//! Identifies the application in the module. The host enquires the
//! information right after the session opens; the reply is stored per
//! slot and reported as [`CaEvent::ApplicationInfo`].

use std::collections::HashMap;

use super::{
    super::{
        apdu::ApduTag,
        session::CaEvent,
    },
    Resource,
    ResourceContext,
    ResourceId,
};
use crate::error::{
    Error,
    Result,
};

/// en50221 8.4.2.2: application_info object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationInfo {
    /// en50221 Table 61: 0x01 - Conditional Access, 0x02 - EPG
    pub application_type: u8,
    pub application_manufacturer: u16,
    pub manufacturer_code: u16,
    /// application name in DVB charset coding (EN 300 468 annex A)
    pub menu_string: Vec<u8>,
}

fn parse_application_info(slot_id: u8, body: &[u8]) -> Result<ApplicationInfo> {
    if body.len() < 6 {
        return Err(Error::InvalidData(format!(
            "ca slot {}: application info is too short",
            slot_id
        )));
    }

    let menu_string = body
        .get(6 .. 6 + usize::from(body[5]))
        .ok_or_else(|| {
            Error::InvalidData(format!(
                "ca slot {}: application info menu string is truncated",
                slot_id
            ))
        })?
        .to_vec();

    Ok(ApplicationInfo {
        application_type: body[0],
        application_manufacturer: (u16::from(body[1]) << 8) | u16::from(body[2]),
        manufacturer_code: (u16::from(body[3]) << 8) | u16::from(body[4]),
        menu_string,
    })
}

/// Application Information resource
pub struct ApplicationInfoResource {
    /// application info belongs to a resource session; keeping sessions
    /// separate prevents one close from erasing another live application
    sessions: HashMap<u16, ApplicationInfoSession>,
    revision: u64,
}

struct ApplicationInfoSession {
    slot_id: u8,
    info: Option<ApplicationInfo>,
    revision: u64,
}

impl ApplicationInfoResource {
    pub fn new() -> Self {
        ApplicationInfoResource {
            sessions: HashMap::new(),
            revision: 0,
        }
    }

    /// Last application info received from a live session in the slot
    pub fn info(&self, slot_id: u8) -> Option<&ApplicationInfo> {
        self.info_session(slot_id).map(|(_, info)| info)
    }

    /// Session which supplied the last application info in the slot
    pub fn info_session(&self, slot_id: u8) -> Option<(u16, &ApplicationInfo)> {
        self.sessions
            .iter()
            .filter(|(_, session)| session.slot_id == slot_id)
            .filter_map(|(session_id, session)| {
                session
                    .info
                    .as_ref()
                    .map(|info| (*session_id, session.revision, info))
            })
            .max_by_key(|(session_id, revision, _)| (*revision, *session_id))
            .map(|(session_id, _, info)| (session_id, info))
    }
}

impl Resource for ApplicationInfoResource {
    fn resource_id(&self) -> ResourceId {
        ResourceId::APPLICATION_INFORMATION
    }

    fn on_open(&mut self, ctx: &mut ResourceContext<'_>) -> Result<()> {
        self.sessions.insert(
            ctx.session_id,
            ApplicationInfoSession {
                slot_id: ctx.slot_id,
                info: None,
                revision: 0,
            },
        );
        ctx.send_apdu(ApduTag::APPLICATION_INFO_ENQ, &[])
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()> {
        match tag {
            ApduTag::APPLICATION_INFO => {
                let info = parse_application_info(ctx.slot_id, body)?;
                let session = self.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
                    Error::InvalidData(format!(
                        "ca slot {}: application info on unknown resource session {}",
                        ctx.slot_id, ctx.session_id
                    ))
                })?;
                self.revision = self.revision.saturating_add(1);
                session.info = Some(info.clone());
                session.revision = self.revision;
                ctx.event(CaEvent::ApplicationInfo {
                    slot_id: ctx.slot_id,
                    info,
                });

                Ok(())
            }
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected application info apdu tag {:?}",
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
    fn test_parse() {
        let body = [
            0x01, 0x12, 0x34, 0x56, 0x78, 0x08, b'T', b'e', b's', b't', b' ', b'C', b'A', b'M',
        ];
        assert_eq!(
            parse_application_info(0, &body).unwrap(),
            ApplicationInfo {
                application_type: 0x01,
                application_manufacturer: 0x1234,
                manufacturer_code: 0x5678,
                menu_string: b"Test CAM".to_vec(),
            }
        );
    }

    #[test]
    fn test_parse_empty_menu_string() {
        let body = [0x01, 0x12, 0x34, 0x56, 0x78, 0x00];
        let info = parse_application_info(0, &body).unwrap();
        assert!(info.menu_string.is_empty());
    }

    #[test]
    fn test_parse_ignores_trailing_bytes() {
        let body = [0x01, 0x12, 0x34, 0x56, 0x78, 0x01, b'A', 0xFF, 0xFF];
        let info = parse_application_info(0, &body).unwrap();
        assert_eq!(info.menu_string, b"A".to_vec());
    }

    #[test]
    fn test_parse_errors() {
        // too short for the fixed fields
        assert!(parse_application_info(0, &[0x01, 0x12, 0x34, 0x56, 0x78]).is_err());
        // menu string length exceeds the remaining bytes
        assert!(parse_application_info(0, &[0x01, 0x12, 0x34, 0x56, 0x78, 0x02, b'A']).is_err());
    }
}
