# Upraise Booking

This is [Cal.rs](https://github.com/olivierlambert/calrs) (AGPL-3.0) as run by Upraise at https://book.upraise.uk.
Our changes are branding only: the product name "Upraise Booking" in on-screen and email text, page titles, favicon,
sidebar logo and line icons, and a link back to Upraise Outreach. They are made by `upraise/apply_upraise.py`,
which is re-run on each new Cal.rs release. Scheduling, calendar and booking behaviour are unchanged from upstream.

- `main`: the version running at book.upraise.uk (an upstream release tag + the branding commit)
- `upstream-main`: tracks upstream `main`

Upgrade: merge the new upstream tag into a branch, run `python3 upraise/apply_upraise.py .`, build, open a pull request.
