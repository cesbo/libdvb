//! en50221 8.4.5: Date-Time resource
//!
//! The module enquires the current time with date_time_enq and the host
//! replies with the date_time object: MJD UTC date and BCD time coded as
//! in EN 300 468 annex C. A non-zero response interval makes the host
//! resend the time periodically from the session layer tick.

use std::{
    collections::HashMap,
    time::{
        Duration,
        Instant,
        SystemTime,
        UNIX_EPOCH,
    },
};

use super::{
    super::{
        apdu::ApduTag,
        transport::CiTransport,
    },
    Resource,
    ResourceContext,
    ResourceId,
};
use crate::error::{
    Error,
    Result,
};

/// MJD of the Unix epoch (1970-01-01)
const MJD_UNIX_EPOCH: u64 = 40587;

fn bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// Encodes the date_time object body: MJD (16 bits) and UTC time
/// (24 bits BCD)
fn date_time_body(unix_time: Duration) -> [u8; 5] {
    let mjd = (MJD_UNIX_EPOCH + unix_time.as_secs() / 86400).min(u64::from(u16::MAX));
    let time = unix_time.as_secs() % 86400;

    [
        (mjd >> 8) as u8,
        mjd as u8,
        bcd((time / 3600) as u8),
        bcd((time / 60 % 60) as u8),
        bcd((time % 60) as u8),
    ]
}

fn now_body() -> [u8; 5] {
    let unix_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);

    date_time_body(unix_time)
}

/// Per-session response state
struct DateTimeSession {
    slot_id: u8,
    /// response interval from date_time_enq, seconds; 0 disables the
    /// periodic updates
    interval: u8,
    /// when the date_time object was sent the last time
    sent: Option<Instant>,
}

/// Date-Time resource
pub struct DateTimeResource {
    sessions: HashMap<u16, DateTimeSession>,
}

impl DateTimeResource {
    pub fn new() -> Self {
        DateTimeResource {
            sessions: HashMap::new(),
        }
    }
}

impl Resource for DateTimeResource {
    fn resource_id(&self) -> ResourceId {
        ResourceId::DATE_TIME
    }

    fn on_open(&mut self, ctx: &mut ResourceContext<'_>) -> Result<()> {
        self.sessions.insert(
            ctx.session_id,
            DateTimeSession {
                slot_id: ctx.slot_id,
                interval: 0,
                sent: None,
            },
        );

        // announce the time right away: some modules expect the object
        // shortly after the session opens, before any enquiry
        ctx.send_apdu(ApduTag::DATE_TIME, &now_body())
    }

    fn on_apdu(&mut self, ctx: &mut ResourceContext<'_>, tag: ApduTag, body: &[u8]) -> Result<()> {
        match tag {
            ApduTag::DATE_TIME_ENQ => {
                let interval = body.first().copied().unwrap_or(0);
                if let Some(session) = self.sessions.get_mut(&ctx.session_id) {
                    session.interval = interval;
                    session.sent = None;
                }

                ctx.send_apdu(ApduTag::DATE_TIME, &now_body())
            }
            tag => Err(Error::InvalidData(format!(
                "ca slot {}: unexpected date-time apdu tag {:?}",
                ctx.slot_id, tag
            ))),
        }
    }

    fn on_close(&mut self, _slot_id: u8, session_id: u16) {
        self.sessions.remove(&session_id);
    }

    fn on_tick(&mut self, transport: &mut CiTransport, now: Instant) -> Result<()> {
        for (&session_id, session) in self.sessions.iter_mut() {
            if session.interval == 0 {
                continue;
            }
            let Some(sent) = session.sent else {
                // on_open/on_apdu do not receive the caller's monotonic
                // clock; anchor the interval on the first explicit tick.
                session.sent = Some(now);
                continue;
            };
            if now.saturating_duration_since(sent)
                < Duration::from_secs(u64::from(session.interval))
            {
                continue;
            }

            transport.send_apdu(session.slot_id, session_id, ApduTag::DATE_TIME, &now_body())?;
            session.sent = Some(now);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcd() {
        assert_eq!(bcd(0), 0x00);
        assert_eq!(bcd(9), 0x09);
        assert_eq!(bcd(10), 0x10);
        assert_eq!(bcd(23), 0x23);
        assert_eq!(bcd(59), 0x59);
    }

    #[test]
    fn test_date_time_body() {
        // the Unix epoch: MJD 40587 (0x9E8B), midnight
        assert_eq!(
            date_time_body(Duration::ZERO),
            [0x9E, 0x8B, 0x00, 0x00, 0x00]
        );
        // the last second of the epoch day
        assert_eq!(
            date_time_body(Duration::from_secs(86399)),
            [0x9E, 0x8B, 0x23, 0x59, 0x59]
        );
        // 2026-07-04 (20638 days since the epoch): MJD 61225 (0xEF29)
        assert_eq!(
            date_time_body(Duration::from_secs(
                20638 * 86400 + 12 * 3600 + 34 * 60 + 56
            )),
            [0xEF, 0x29, 0x12, 0x34, 0x56]
        );
    }

    #[test]
    fn test_date_time_body_mjd_saturation() {
        // the 16-bit MJD field ends in April 2038
        let far_future = Duration::from_secs(3_000_000_000);
        assert_eq!(&date_time_body(far_future)[.. 2], &[0xFF, 0xFF]);
    }
}
