-- Sales meetings: only bookings of event types with this switch on are sent to Booking events (Upraise Outreach and
-- the notetaker). Off by default, so internal meetings stay internal.
ALTER TABLE event_types ADD COLUMN sales_meeting INTEGER NOT NULL DEFAULT 0;
