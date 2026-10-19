-- Guests can add other people's emails to the invite on every event type: up to 10 by default.
-- Event types that had it switched off (0, the old default) move to 10; hosts can still change it.
UPDATE event_types SET max_additional_guests = 10 WHERE max_additional_guests = 0;
