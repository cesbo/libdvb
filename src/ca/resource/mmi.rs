//! en50221 8.6: Man-Machine Interface resource (high-level MMI)
//!
//! The module opens a session when it needs a dialogue with the user.
//! The host confirms the high-level MMI mode and surfaces the dialogue
//! objects (text, enquiry, menu, list) as events; answers go back with
//! the [`CiSession`] MMI methods.
//!
//! [`CiSession`]: super::super::CiSession

use std::collections::HashMap;

use super::{
    super::apdu,
    super::apdu::ApduTag,
    super::session::CaEvent,
    Resource,
    ResourceContext,
    ResourceId,
};
use crate::error::{
    Error,
    Result,
};

/// Cap for an MMI object reassembled from a *_more chain
const MAX_OBJECT_SIZE: usize = 65536;

/// en50221 Table 42: display_control command: switch the MMI mode
const DCC_SET_MMI_MODE: u8 = 0x01;
/// en50221 Table 44: mmi_mode: high-level MMI
const MM_HIGH_LEVEL: u8 = 0x01;
/// en50221 Table 45: display_reply id: the mode switch is accepted
const DRI_MMI_MODE_ACK: u8 = 0x01;
/// en50221 Table 45: display_reply id: the command is not supported
const DRI_UNKNOWN_CMD: u8 = 0xF0;
/// en50221 Table 45: display_reply id: the mode is not supported
const DRI_UNKNOWN_MMI_MODE: u8 = 0xF1;

/// en50221 8.6.5.1: close_mmi command: close right away
const CLOSE_MMI_IMMEDIATE: u8 = 0x00;
/// en50221 8.6.5.1: close_mmi command: close after a delay
const CLOSE_MMI_DELAY: u8 = 0x01;

/// en50221 8.6.5.5: answ id: the enquiry is cancelled
const ANSW_CANCEL: u8 = 0x00;
/// en50221 8.6.5.5: answ id: the answer text follows
const ANSW_ANSWER: u8 = 0x01;

/// High-level MMI menu or list (en50221 8.6.2)
///
/// All strings are in DVB charset coding (EN 300 468 annex A).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MmiMenu {
    pub title: Vec<u8>,
    pub sub_title: Vec<u8>,
    pub bottom: Vec<u8>,
    /// menu choices; a menu answer is the 1-based item number
    pub items: Vec<Vec<u8>>,
}

/// Parses one nested text object: a text_more chain closed by text_last.
/// Returns the accumulated text with the consumed size, or `None` when
/// the data does not start with a complete text object.
fn parse_text(data: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut text = Vec::new();
    let mut offset = 0;

    loop {
        let (apdu, size) = apdu::parse_at(&data[offset ..]).ok()?;
        offset += size;
        text.extend_from_slice(apdu.body);

        match apdu.tag {
            ApduTag::TEXT_LAST => return Some((text, offset)),
            ApduTag::TEXT_MORE => continue,
            _ => return None,
        }
    }
}

/// Parses a menu_last/list_last body. The parser is tolerant to
/// truncated objects real modules produce: everything from the first
/// damaged text object on is dropped.
fn parse_menu(body: &[u8]) -> MmiMenu {
    if body.is_empty() {
        return MmiMenu::default();
    }

    // choice_nb (the first byte) is unreliable on real modules -
    // the items are parsed to exhaustion instead
    let mut offset = 1;

    let mut header: [Vec<u8>; 3] = Default::default();
    let mut header_complete = true;
    for text in &mut header {
        match parse_text(&body[offset ..]) {
            Some((value, size)) => {
                *text = value;
                offset += size;
            }
            None => {
                header_complete = false;
                break;
            }
        }
    }

    let [title, sub_title, bottom] = header;
    let mut menu = MmiMenu {
        title,
        sub_title,
        bottom,
        items: Vec::new(),
    };

    if !header_complete {
        return menu;
    }

    while let Some((item, size)) = parse_text(&body[offset ..]) {
        menu.items.push(item);
        offset += size;
    }

    menu
}

/// `[answ_id]` or `[answ_id, text...]` - the enq answer body;
/// `None` cancels the enquiry
pub(in super::super) fn build_answ(answer: Option<&[u8]>) -> Vec<u8> {
    match answer {
        Some(text) => {
            let mut body = Vec::with_capacity(1 + text.len());
            body.push(ANSW_ANSWER);
            body.extend_from_slice(text);
            body
        }
        None => vec![ANSW_CANCEL],
    }
}

/// `[close_mmi_cmd_id]` - the host-initiated immediate close_mmi body
pub(in super::super) fn build_close() -> Vec<u8> {
    vec![CLOSE_MMI_IMMEDIATE]
}

/// Man-Machine Interface resource
pub struct MmiResource {
    /// per-session reassembly of text_more/menu_more/list_more chains
    fragments: HashMap<u16, Vec<u8>>,
}

impl MmiResource {
    pub fn new() -> Self {
        MmiResource {
            fragments: HashMap::new(),
        }
    }

    /// Completes an object: the accumulated *_more chain of the session
    /// plus the closing *_last body
    fn take_object(&mut self, session_id: u16, last: &[u8]) -> Vec<u8> {
        match self.fragments.remove(&session_id) {
            Some(mut object) => {
                object.extend_from_slice(last);
                object
            }
            None => last.to_vec(),
        }
    }
}

impl Resource for MmiResource {
    fn resource_id(&self) -> ResourceId {
        ResourceId::MMI
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()> {
        match tag {
            // an object larger than one APDU arrives as a *_more chain
            // closed by the matching *_last (en50221 8.6.2)
            ApduTag::TEXT_MORE | ApduTag::MENU_MORE | ApduTag::LIST_MORE => {
                let object = self.fragments.entry(ctx.session_id).or_default();
                if object.len() + body.len() > MAX_OBJECT_SIZE {
                    self.fragments.remove(&ctx.session_id);
                    return Err(Error::InvalidData(format!(
                        "ca slot {}: mmi object reassembly overflow",
                        ctx.slot_id
                    )));
                }
                object.extend_from_slice(body);

                Ok(())
            }
            ApduTag::CLOSE_MMI => {
                let delay = if body.first() == Some(&CLOSE_MMI_DELAY) {
                    body.get(1).copied()
                } else {
                    None
                };
                ctx.event(CaEvent::MmiClose {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    delay,
                });
                // the dialogue is over: take the session down
                ctx.close_session = true;

                Ok(())
            }
            ApduTag::DISPLAY_CONTROL => {
                let reply: &[u8] = match (body.first(), body.get(1)) {
                    (Some(&DCC_SET_MMI_MODE), Some(&MM_HIGH_LEVEL)) => {
                        &[DRI_MMI_MODE_ACK, MM_HIGH_LEVEL]
                    }
                    (Some(&DCC_SET_MMI_MODE), _) => &[DRI_UNKNOWN_MMI_MODE],
                    _ => &[DRI_UNKNOWN_CMD],
                };

                ctx.send_apdu(ApduTag::DISPLAY_REPLY, reply)
            }
            ApduTag::TEXT_LAST => {
                let text = self.take_object(ctx.session_id, body);
                ctx.event(CaEvent::MmiText {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    text,
                });

                Ok(())
            }
            ApduTag::ENQ => {
                if body.len() < 2 {
                    return Err(Error::InvalidData(format!(
                        "ca slot {}: mmi enq is too short",
                        ctx.slot_id
                    )));
                }
                ctx.event(CaEvent::MmiEnq {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    blind: (body[0] & 0x01) != 0,
                    answer_len: body[1],
                    text: body[2 ..].to_vec(),
                });

                Ok(())
            }
            ApduTag::MENU_LAST => {
                let object = self.take_object(ctx.session_id, body);
                ctx.event(CaEvent::MmiMenu {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    menu: parse_menu(&object),
                });

                Ok(())
            }
            ApduTag::LIST_LAST => {
                let object = self.take_object(ctx.session_id, body);
                ctx.event(CaEvent::MmiList {
                    slot_id: ctx.slot_id,
                    session_id: ctx.session_id,
                    menu: parse_menu(&object),
                });

                Ok(())
            }
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected mmi apdu tag {:?}",
                ctx.slot_id, tag
            ))),
        }
    }

    fn on_close(&mut self, _slot_id: u8, session_id: u16) {
        self.fragments.remove(&session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_object(text: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        apdu::build(&mut out, ApduTag::TEXT_LAST, text);
        out
    }

    #[test]
    fn test_parse_menu() {
        let mut body = vec![0x02];
        body.extend_from_slice(&text_object(b"Menu"));
        body.extend_from_slice(&text_object(b"Sub"));
        body.extend_from_slice(&text_object(b""));
        body.extend_from_slice(&text_object(b"Info"));
        body.extend_from_slice(&text_object(b"Exit"));

        assert_eq!(
            parse_menu(&body),
            MmiMenu {
                title: b"Menu".to_vec(),
                sub_title: b"Sub".to_vec(),
                bottom: Vec::new(),
                items: vec![b"Info".to_vec(), b"Exit".to_vec()],
            }
        );
    }

    #[test]
    fn test_parse_menu_text_more_chain() {
        let mut body = vec![0x01];
        apdu::build(&mut body, ApduTag::TEXT_MORE, b"Ti");
        apdu::build(&mut body, ApduTag::TEXT_LAST, b"tle");
        body.extend_from_slice(&text_object(b""));
        body.extend_from_slice(&text_object(b""));
        body.extend_from_slice(&text_object(b"Item"));

        let menu = parse_menu(&body);
        assert_eq!(menu.title, b"Title".to_vec());
        assert_eq!(menu.items, vec![b"Item".to_vec()]);
    }

    #[test]
    fn test_parse_menu_tolerates_damage() {
        // empty body
        assert_eq!(parse_menu(&[]), MmiMenu::default());
        // choice_nb only, no text objects
        assert_eq!(parse_menu(&[0x00]), MmiMenu::default());

        // damaged sub-title: only the title survives
        let mut body = vec![0x00];
        body.extend_from_slice(&text_object(b"Title"));
        body.extend_from_slice(&[0x9F, 0x88]);
        let menu = parse_menu(&body);
        assert_eq!(menu.title, b"Title".to_vec());
        assert!(menu.items.is_empty());

        // damaged trailing item: complete items survive
        let mut body = vec![0x00];
        body.extend_from_slice(&text_object(b"Title"));
        body.extend_from_slice(&text_object(b""));
        body.extend_from_slice(&text_object(b""));
        body.extend_from_slice(&text_object(b"Item"));
        // text_more chain without the closing text_last
        apdu::build(&mut body, ApduTag::TEXT_MORE, b"broken");
        let menu = parse_menu(&body);
        assert_eq!(menu.items, vec![b"Item".to_vec()]);
    }

    #[test]
    fn test_parse_menu_rejects_non_text_object() {
        // a menu nested inside a menu is not a text object
        let mut body = vec![0x00];
        apdu::build(&mut body, ApduTag::MENU_LAST, b"");
        assert_eq!(parse_menu(&body), MmiMenu::default());
    }

    #[test]
    fn test_build_answ() {
        assert_eq!(build_answ(Some(b"1234")), vec![0x01, b'1', b'2', b'3', b'4']);
        assert_eq!(build_answ(None), vec![0x00]);
    }

    #[test]
    fn test_build_close() {
        assert_eq!(build_close(), vec![0x00]);
    }
}
