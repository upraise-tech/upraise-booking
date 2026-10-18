-- Which connected Google account creates the user's Google Meet links.
-- NULL keeps the default: the first Google calendar bookings are written to.
ALTER TABLE users ADD COLUMN meet_source_id TEXT;
