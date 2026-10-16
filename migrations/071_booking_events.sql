-- Booking events for a CRM (Upraise Outreach), in the Cal.com webhook format (src/web/booking_events.rs).
--
-- booking_events_url     where BOOKING_CREATED / BOOKING_RESCHEDULED / BOOKING_CANCELLED are POSTed; NULL = off
-- booking_events_secret  shared secret for the X-Cal-Signature-256 HMAC, encrypted at rest like other secrets
ALTER TABLE auth_config ADD COLUMN booking_events_url TEXT;
ALTER TABLE auth_config ADD COLUMN booking_events_secret TEXT;

-- What was last delivered per booking, so each pass sends only the difference and retries what failed.
-- sent_status NULL = nothing delivered yet (only a failed attempt recorded).
CREATE TABLE IF NOT EXISTS booking_event_deliveries (
    booking_id      TEXT PRIMARY KEY REFERENCES bookings(id) ON DELETE CASCADE,
    sent_status     TEXT,
    sent_start_at   TEXT,
    sent_end_at     TEXT,
    sent_at         TEXT,
    last_error      TEXT,
    last_attempt_at TEXT
);
