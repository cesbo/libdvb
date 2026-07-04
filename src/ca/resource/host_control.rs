//! en50221 8.4.4: Host Control resource
//!
//! The module asks the host to tune to another transport stream or to
//! substitute PIDs in the stream passed through the module. The base
//! implementation surfaces the requests as events; the decision stays
//! with the application.

use super::{
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

/// 13-bit PID from 2 bytes big-endian
fn read_pid(data: &[u8]) -> u16 {
    (u16::from(data[0] & 0x1F) << 8) | u16::from(data[1])
}

/// Host Control resource
pub struct HostControlResource;

impl Resource for HostControlResource {
    fn resource_id(&self) -> ResourceId {
        ResourceId::HOST_CONTROL
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()> {
        match tag {
            ApduTag::TUNE => {
                if body.len() < 8 {
                    return Err(Error::InvalidData(format!(
                        "ca slot {}: host control tune is too short",
                        ctx.slot_id
                    )));
                }
                ctx.event(CaEvent::Tune {
                    slot_id: ctx.slot_id,
                    network_id: (u16::from(body[0]) << 8) | u16::from(body[1]),
                    original_network_id: (u16::from(body[2]) << 8) | u16::from(body[3]),
                    transport_stream_id: (u16::from(body[4]) << 8) | u16::from(body[5]),
                    service_id: (u16::from(body[6]) << 8) | u16::from(body[7]),
                });

                Ok(())
            }
            ApduTag::REPLACE => {
                if body.len() < 5 {
                    return Err(Error::InvalidData(format!(
                        "ca slot {}: host control replace is too short",
                        ctx.slot_id
                    )));
                }
                ctx.event(CaEvent::Replace {
                    slot_id: ctx.slot_id,
                    replace_ref: body[0],
                    replaced_pid: read_pid(&body[1 .. 3]),
                    replacement_pid: read_pid(&body[3 .. 5]),
                });

                Ok(())
            }
            ApduTag::CLEAR_REPLACE => {
                if body.is_empty() {
                    return Err(Error::InvalidData(format!(
                        "ca slot {}: host control clear_replace is too short",
                        ctx.slot_id
                    )));
                }
                ctx.event(CaEvent::ClearReplace {
                    slot_id: ctx.slot_id,
                    replace_ref: body[0],
                });

                Ok(())
            }
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected host control apdu tag {:?}",
                ctx.slot_id, tag
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_pid() {
        // the top 3 bits are reserved and set on the wire
        assert_eq!(read_pid(&[0xFF, 0xFE]), 0x1FFE);
        assert_eq!(read_pid(&[0xE1, 0x23]), 0x0123);
        assert_eq!(read_pid(&[0x00, 0x00]), 0x0000);
    }
}
