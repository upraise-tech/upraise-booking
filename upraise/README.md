# Upraise Booking

This is [Cal.rs](https://github.com/olivierlambert/calrs) (AGPL-3.0) as run by Upraise at https://book.upraise.uk.
Our changes are branding only: the product name "Upraise Booking" in on-screen and email text, page titles, favicon,
sidebar logo and line icons, and a link back to Upraise Outreach. They are made by `upraise/apply_upraise.py`,
which is re-run on each new Cal.rs release. Scheduling, calendar and booking behaviour are unchanged from upstream.

- `main`: the version running at book.upraise.uk (an upstream release tag + the branding commit)
- `upstream-main`: tracks upstream `main`

Upgrade: merge the new upstream tag into a branch, run `python3 upraise/apply_upraise.py .`, build, open a pull request.

## Google Analytics on chosen booking pages

Off unless `CALRS_GA4_ID` is set (`src/web/ga4.rs`, `templates/_ga4.html`). Then the Google tag loads only on the
event types listed in `CALRS_GA4_PAGES` (their slot page, booking form and confirmation), with the CSP widened for
those pages only. A booking on one of them sends the `CALRS_GA4_EVENT` event (default `book_call`) with the event type
path and whether the booking is confirmed or pending; nothing about the guest. `CALRS_GA4_LINKER_DOMAINS` lists the
domains that share the visit (cross-domain measurement). At book.upraise.uk: `G-GXTC85E4TB` (the upraiseseo.com
property), on `/team/bd/intro-call`, linked with `upraiseseo.com`.
