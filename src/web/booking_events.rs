//! Booking events for a CRM (Upraise Outreach): booked, rescheduled and cancelled bookings are sent to one
//! configured URL in the Cal.com webhook format, so any receiver that understands Cal.com webhooks (Upraise Outreach's
//! "Cal.com" integration among them) records them without a dedicated adapter.
//!
//! Delivery is a reconciliation, not a hook in every handler: once a minute (from `run_reminder_loop`) each recent
//! booking is compared with what was last delivered for it (`booking_event_deliveries`), and the difference is sent:
//!
//! * confirmed, never delivered                       → `BOOKING_CREATED`
//! * confirmed, delivered with other start/end times  → `BOOKING_RESCHEDULED` (same uid: reschedules keep the row)
//! * cancelled or declined, delivered as confirmed     → `BOOKING_CANCELLED`
//!
//! Every path that creates, approves, reschedules, declines or cancels a booking is covered without being touched,
//! and a failed delivery is simply retried on the next pass. Pending requests are not sent until approved.
//!
//! Requests are signed like Cal.com's: `X-Cal-Signature-256` = hex HMAC-SHA256 of the raw body with the shared secret.

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::json;
use sqlx::SqlitePool;

/// What to send for one booking, given its current state and what was delivered before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Created,
    Rescheduled,
    Cancelled,
}

impl Trigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Trigger::Created => "BOOKING_CREATED",
            Trigger::Rescheduled => "BOOKING_RESCHEDULED",
            Trigger::Cancelled => "BOOKING_CANCELLED",
        }
    }
}

/// The last delivered state of a booking. `status` is `None` while nothing has been delivered yet.
pub struct Delivered<'a> {
    pub status: Option<&'a str>,
    pub start_at: Option<&'a str>,
    pub end_at: Option<&'a str>,
}

/// Pure decision: which event (if any) brings the receiver up to date with this booking.
pub fn decide(status: &str, start_at: &str, end_at: &str, sent: &Delivered<'_>) -> Option<Trigger> {
    match (sent.status, status) {
        (None, "confirmed") => Some(Trigger::Created),
        (Some("confirmed"), "confirmed")
            if sent.start_at != Some(start_at) || sent.end_at != Some(end_at) =>
        {
            Some(Trigger::Rescheduled)
        }
        (Some("confirmed"), "cancelled" | "declined") => Some(Trigger::Cancelled),
        _ => None,
    }
}

/// Destination and secret, read from `auth_config`. `None` when the feature is off.
pub struct Config {
    pub url: String,
    pub secret: String,
}

pub async fn load_config(pool: &SqlitePool, secret_key: &[u8; 32]) -> Option<Config> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT booking_events_url, booking_events_secret FROM auth_config WHERE id = 'singleton'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let (url, secret_enc) = row?;
    let url = url.filter(|u| !u.trim().is_empty())?;
    let secret = secret_enc
        .filter(|s| !s.is_empty())
        .and_then(|s| crate::crypto::decrypt_value(secret_key, &s).ok())
        .unwrap_or_default();
    Some(Config { url, secret })
}

/// A booking as the receiver needs it.
#[derive(sqlx::FromRow)]
pub struct BookingRow {
    pub id: String,
    pub uid: String,
    pub status: String,
    pub start_at: String,
    pub end_at: String,
    pub guest_name: String,
    pub guest_email: String,
    pub guest_timezone: String,
    pub title: String,
    pub slug: String,
    pub location_type: Option<String>,
    pub location: Option<String>,
    pub organizer_name: Option<String>,
    pub organizer_email: Option<String>,
    pub cancel_token: String,
    pub reschedule_token: String,
    pub sent_status: Option<String>,
    pub sent_start_at: Option<String>,
    pub sent_end_at: Option<String>,
}

/// Stored booking times (UTC, `time_version = 1`) as RFC 3339.
fn rfc3339(stored: &str) -> String {
    let s = stored.trim().trim_end_matches('Z');
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .map(|n| {
            n.and_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        })
        .unwrap_or_else(|_| stored.to_string())
}

fn is_http(u: &str) -> bool {
    let l = u.trim().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// The Cal.com webhook body for one booking.
pub fn payload(
    b: &BookingRow,
    trigger: Trigger,
    base_url: Option<&str>,
    now: DateTime<Utc>,
) -> serde_json::Value {
    let location = b.location.clone().unwrap_or_default();
    let video = if is_http(&location) {
        location.clone()
    } else {
        String::new()
    };
    let base = base_url.map(|u| u.trim_end_matches('/').to_string());
    let link = |path: &str, token: &str| {
        base.as_ref()
            .map(|u| format!("{}/booking/{}/{}", u, path, token))
    };
    json!({
        "triggerEvent": trigger.as_str(),
        "createdAt": now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "payload": {
            "type": b.slug,
            "title": b.title,
            "startTime": rfc3339(&b.start_at),
            "endTime": rfc3339(&b.end_at),
            "uid": b.uid,
            "status": if trigger == Trigger::Cancelled { "CANCELLED" } else { "ACCEPTED" },
            "location": if video.is_empty() { location } else { video.clone() },
            "attendees": [{ "email": b.guest_email, "name": b.guest_name, "timeZone": b.guest_timezone }],
            "organizer": { "email": b.organizer_email.clone().unwrap_or_default(), "name": b.organizer_name.clone().unwrap_or_default() },
            "metadata": { "videoCallUrl": video },
            "responses": {},
            "cancellationReason": if b.status == "declined" { "Declined by the host" } else { "" },
            "cancelUrl": link("cancel", &b.cancel_token),
            "rescheduleUrl": link("reschedule", &b.reschedule_token),
            "source": "upraise-booking",
        }
    })
}

/// Hex HMAC-SHA256, the Cal.com `X-Cal-Signature-256` value.
pub fn signature(secret: &str, body: &[u8]) -> String {
    super::meeting::sign_hmac_sha256(secret.as_bytes(), body)
}

const CANDIDATES_SQL: &str = "
    SELECT b.id, b.uid, b.status, b.start_at, b.end_at, b.guest_name, b.guest_email, b.guest_timezone,
           et.title, et.slug, et.location_type,
           COALESCE(NULLIF(b.meeting_url, ''), et.location_value) AS location,
           COALESCE(NULLIF(b.organizer_name, ''), hu.name, ou.name) AS organizer_name,
           COALESCE(NULLIF(b.organizer_email, ''), hu.booking_email, hu.email, ou.booking_email, ou.email) AS organizer_email,
           b.cancel_token, b.reschedule_token,
           d.sent_status, d.sent_start_at, d.sent_end_at
    FROM bookings b
    JOIN event_types et ON et.id = b.event_type_id
    LEFT JOIN accounts a ON a.id = et.account_id
    LEFT JOIN users ou ON ou.id = a.user_id
    LEFT JOIN users hu ON hu.id = b.assigned_user_id
    LEFT JOIN booking_event_deliveries d ON d.booking_id = b.id
    WHERE b.time_version = 1 AND b.end_at >= ?
    ORDER BY b.start_at";

/// One pass: send every event due. Called once a minute; does nothing when the feature is off.
pub async fn deliver_due(pool: &SqlitePool, secret_key: &[u8; 32]) {
    let Some(cfg) = load_config(pool, secret_key).await else {
        return;
    };
    let now = Utc::now();
    // A day back so a cancellation of today's meeting, or of one that just ended, still goes out.
    let cutoff = (now - Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let rows: Vec<BookingRow> = match sqlx::query_as(CANDIDATES_SQL)
        .bind(&cutoff)
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "booking events: could not read bookings");
            return;
        }
    };
    let base_url = crate::settings::base_url();
    let client = reqwest::Client::new();
    for b in rows {
        let sent = Delivered {
            status: b.sent_status.as_deref(),
            start_at: b.sent_start_at.as_deref(),
            end_at: b.sent_end_at.as_deref(),
        };
        let Some(trigger) = decide(&b.status, &b.start_at, &b.end_at, &sent) else {
            continue;
        };
        let body = match serde_json::to_vec(&payload(&b, trigger, base_url.as_deref(), now)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut req = client
            .post(&cfg.url)
            .header("content-type", "application/json")
            .header("user-agent", "upraise-booking-events/1")
            .timeout(std::time::Duration::from_secs(10));
        if !cfg.secret.is_empty() {
            req = req.header("X-Cal-Signature-256", signature(&cfg.secret, &body));
        }
        let result = match req.body(body).send().await {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) => Err(format!("HTTP {}", r.status().as_u16())),
            Err(e) => Err(if e.is_timeout() {
                "timed out".to_string()
            } else {
                "could not connect".to_string()
            }),
        };
        let delivered_status = if trigger == Trigger::Cancelled {
            "cancelled"
        } else {
            "confirmed"
        };
        let saved = match &result {
            Ok(()) => {
                tracing::info!(booking = %b.id, event = trigger.as_str(), "booking event delivered");
                sqlx::query(
                    "INSERT INTO booking_event_deliveries (booking_id, sent_status, sent_start_at, sent_end_at, sent_at, last_error, last_attempt_at)
                     VALUES (?, ?, ?, ?, datetime('now'), NULL, datetime('now'))
                     ON CONFLICT(booking_id) DO UPDATE SET sent_status = excluded.sent_status, sent_start_at = excluded.sent_start_at,
                       sent_end_at = excluded.sent_end_at, sent_at = excluded.sent_at, last_error = NULL, last_attempt_at = excluded.last_attempt_at",
                )
                .bind(&b.id)
                .bind(delivered_status)
                .bind(&b.start_at)
                .bind(&b.end_at)
                .execute(pool)
                .await
            }
            Err(err) => {
                tracing::warn!(booking = %b.id, event = trigger.as_str(), error = %err, "booking event not delivered, retrying next minute");
                sqlx::query(
                    "INSERT INTO booking_event_deliveries (booking_id, last_error, last_attempt_at) VALUES (?, ?, datetime('now'))
                     ON CONFLICT(booking_id) DO UPDATE SET last_error = excluded.last_error, last_attempt_at = excluded.last_attempt_at",
                )
                .bind(&b.id)
                .bind(err)
                .execute(pool)
                .await
            }
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, booking = %b.id, "booking events: could not record delivery");
        }
    }
}

/// When the destination is first set: mark past bookings as already reported, so only upcoming bookings and new
/// changes are sent rather than the whole history.
pub async fn baseline_past(pool: &SqlitePool) {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    if let Err(e) = sqlx::query(
        "INSERT OR IGNORE INTO booking_event_deliveries (booking_id, sent_status, sent_start_at, sent_end_at, sent_at)
         SELECT id, CASE WHEN status = 'confirmed' THEN 'confirmed' ELSE 'cancelled' END, start_at, end_at, datetime('now')
         FROM bookings WHERE start_at < ?",
    )
    .bind(&now)
    .execute(pool)
    .await
    {
        tracing::warn!(error = %e, "booking events: baseline failed");
    }
}

/// For the admin card: (last successful delivery, last error, bookings waiting for a retry).
pub async fn status(pool: &SqlitePool) -> (Option<String>, Option<String>, i64) {
    let last_ok: Option<(Option<String>,)> =
        sqlx::query_as("SELECT MAX(sent_at) FROM booking_event_deliveries")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let last_err: Option<(String,)> = sqlx::query_as(
        "SELECT last_error FROM booking_event_deliveries WHERE last_error IS NOT NULL ORDER BY last_attempt_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let waiting: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM booking_event_deliveries WHERE last_error IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0,));
    (last_ok.and_then(|r| r.0), last_err.map(|r| r.0), waiting.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Delivered<'static> {
        Delivered {
            status: None,
            start_at: None,
            end_at: None,
        }
    }

    #[test]
    fn new_confirmed_booking_is_created() {
        assert_eq!(
            decide("confirmed", "a", "b", &none()),
            Some(Trigger::Created)
        );
    }

    #[test]
    fn pending_request_is_not_sent() {
        assert_eq!(decide("pending", "a", "b", &none()), None);
    }

    #[test]
    fn moved_booking_is_rescheduled() {
        let sent = Delivered {
            status: Some("confirmed"),
            start_at: Some("a"),
            end_at: Some("b"),
        };
        assert_eq!(
            decide("confirmed", "c", "d", &sent),
            Some(Trigger::Rescheduled)
        );
        assert_eq!(decide("confirmed", "a", "b", &sent), None);
    }

    #[test]
    fn cancelled_or_declined_after_delivery_is_cancelled() {
        let sent = Delivered {
            status: Some("confirmed"),
            start_at: Some("a"),
            end_at: Some("b"),
        };
        assert_eq!(
            decide("cancelled", "a", "b", &sent),
            Some(Trigger::Cancelled)
        );
        assert_eq!(
            decide("declined", "a", "b", &sent),
            Some(Trigger::Cancelled)
        );
    }

    #[test]
    fn cancellation_is_sent_once_and_never_before_a_booking() {
        let done = Delivered {
            status: Some("cancelled"),
            start_at: Some("a"),
            end_at: Some("b"),
        };
        assert_eq!(decide("cancelled", "a", "b", &done), None);
        assert_eq!(decide("cancelled", "a", "b", &none()), None);
    }

    #[test]
    fn rescheduled_into_pending_waits_for_approval() {
        let sent = Delivered {
            status: Some("confirmed"),
            start_at: Some("a"),
            end_at: Some("b"),
        };
        assert_eq!(decide("pending", "c", "d", &sent), None);
    }

    #[test]
    fn payload_matches_the_cal_com_shape() {
        let b = BookingRow {
            id: "1".into(),
            uid: "abc@calrs".into(),
            status: "confirmed".into(),
            start_at: "2026-10-20T07:00:00Z".into(),
            end_at: "2026-10-20T07:30:00Z".into(),
            guest_name: "Sara".into(),
            guest_email: "sara@example.com".into(),
            guest_timezone: "Asia/Riyadh".into(),
            title: "30 minutes meeting".into(),
            slug: "30-minutes-meeting".into(),
            location_type: Some("google_meet".into()),
            location: Some("https://meet.google.com/abc-defg-hij".into()),
            organizer_name: Some("Mohamed".into()),
            organizer_email: Some("mohamed@upraiseseo.com".into()),
            cancel_token: "ct".into(),
            reschedule_token: "rt".into(),
            sent_status: None,
            sent_start_at: None,
            sent_end_at: None,
        };
        let now = DateTime::parse_from_rfc3339("2026-10-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let p = payload(&b, Trigger::Created, Some("https://book.upraise.uk/"), now);
        assert_eq!(p["triggerEvent"], "BOOKING_CREATED");
        assert_eq!(p["payload"]["uid"], "abc@calrs");
        assert_eq!(p["payload"]["startTime"], "2026-10-20T07:00:00Z");
        assert_eq!(p["payload"]["endTime"], "2026-10-20T07:30:00Z");
        assert_eq!(p["payload"]["attendees"][0]["email"], "sara@example.com");
        assert_eq!(
            p["payload"]["metadata"]["videoCallUrl"],
            "https://meet.google.com/abc-defg-hij"
        );
        assert_eq!(
            p["payload"]["cancelUrl"],
            "https://book.upraise.uk/booking/cancel/ct"
        );
    }

    #[test]
    fn signature_is_hex_hmac_of_the_body() {
        // Known value: HMAC-SHA256("key", "The quick brown fox jumps over the lazy dog")
        assert_eq!(
            signature("key", b"The quick brown fox jumps over the lazy dog"),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
}
