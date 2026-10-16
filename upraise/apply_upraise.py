"""Upraise branding for Cal.rs (book.upraise.uk). Idempotent; re-run on a fresh Cal.rs checkout after every upgrade.

  python3 apply_upraise.py /path/to/calrs-source

What it changes (nothing about how Cal.rs works, only names and looks):
  i18n/*/main.ftl   "calrs" in on-screen and email text -> "Upraise Booking"; guest email signature -> "— Upraise"
                    (these strings are compiled into the binary, so the image is rebuilt with them)
  templates/        page titles, favicon, Upraise stylesheet + script, sidebar brand and icons,
                    "Upraise Outreach" link, no "Powered by calrs" footer on public pages
                    (templates are read from disk, so they are copied into the image too)
The look itself lives in /opt/calrs/branding (booking.css, mark, favicons, Gilroy), served by nginx at /upraise-brand/.
"""
import pathlib
import re
import sys

SRC = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
NAME = "Upraise Booking"
changed = []


def edit(path, fn):
    p = SRC / path
    s = p.read_text(encoding="utf-8")
    n = fn(s)
    if n != s:
        p.write_text(n, encoding="utf-8")
        changed.append(str(path))


# ---------------------------------------------------------------- i18n (compiled in)
def i18n(s):
    out = []
    for line in s.splitlines(keepends=True):
        m = re.match(r"^([A-Za-z0-9-]+)(\s*=\s*)(.*)$", line, re.S)
        if not m:
            out.append(line)
            continue
        key, eq, val = m.groups()
        if key == "email-signature":
            val = "— Upraise\n" if val.endswith("\n") else "— Upraise"
        else:
            val = re.sub(r"(?<![\w-])calrs(?![\w-])", NAME, val)    # leaves X-Calrs-Signature, CALRS_* and calrs_* alone
        out.append(key + eq + val)
    return "".join(out)


for f in sorted((SRC / "i18n").glob("*/main.ftl")):
    edit(f.relative_to(SRC), i18n)


# ---------------------------------------------------------------- templates (read from disk)
for f in sorted((SRC / "templates").rglob("*.html")):
    edit(f.relative_to(SRC), lambda s: s.replace("— calrs{% endblock %}", "— " + NAME + "{% endblock %}"))

HEAD = ('  <link rel="icon" type="image/png" sizes="32x32" href="/upraise-brand/favicon-32x32.png">\n'
        '  <link rel="icon" type="image/png" sizes="16x16" href="/upraise-brand/favicon-16x16.png">\n'
        '  <link rel="apple-touch-icon" href="/upraise-brand/apple-touch-icon.png">\n')
BRAND = '  <link rel="stylesheet" href="/upraise-brand/booking.css?v=__V__">\n'


def base(s):
    s = s.replace("<title>{% block title %}calrs{% endblock %}</title>", "<title>{% block title %}" + NAME + "{% endblock %}</title>")
    s = re.sub(r'  <link rel="icon" href="data:image/svg\+xml,[^"]*">\n', HEAD, s, count=1)
    if "/upraise-brand/booking.css" not in s:
        s = s.replace('  <link rel="stylesheet" href="/accent.css">\n', '  <link rel="stylesheet" href="/accent.css">\n' + BRAND, 1)
    s = s.replace("""{% block footer %}<div class="powered"><span>{{ t('base-powered-by') }} <a href="https://cal.rs">calrs</a> 🦀</span></div>{% endblock %}""",
                  "{% block footer %}{% endblock %}")
    return s


edit("templates/base.html", base)

ICONS = {   # lucide icons, same family as Upraise Outreach
    "overview": '<rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/>',
    "event-types": '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/><path d="M8 14h.01"/><path d="M12 14h.01"/><path d="M16 14h.01"/>',
    "bookings": '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/><path d="m9 16 2 2 4-4"/>',
    "teams": '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
    "invite-links": '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
    "sources": '<path d="M21 7.5V6a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3.5"/><path d="M16 2v4"/><path d="M8 2v4"/><path d="M3 10h5"/><path d="M17.5 17.5 16 16.3V14"/><circle cx="16" cy="16" r="6"/>',
    "settings": '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
    "troubleshoot": '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
    "admin": '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
    "outreach": '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
}


def svg(name):
    return ('<span class="icon"><svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.75" '
            'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + ICONS[name] + "</svg></span>")


def dash(s):
    s = s.replace('      <img src="/brand-logo" alt="calrs">\n'
                  '      <span class="sidebar-brand-name"><span style="color: var(--accent);">cal</span><span style="color: #e45724;">rs</span></span>\n',
                  '      <img src="/upraise-brand/mark.svg" alt="">\n'
                  '      <span class="sidebar-brand-name">Upraise <span class="up-sub">Booking</span></span>\n')
    for active, emoji in [("overview", "&#9632;"), ("event-types", "&#128197;"), ("bookings", "&#128203;"), ("teams", "&#128101;"),
                          ("organization", "&#127970;"), ("sources", "&#128197;"), ("settings", "&#9881;"),
                          ("troubleshoot", "&#128269;"), ("admin", "&#128272;")]:
        icon = {"organization": "invite-links"}.get(active, active)
        pat = re.compile(r"(\{% if sidebar\.active == '" + re.escape(active) + r"' %\}active\{% endif %\}\">\n\s*)<span class=\"icon\">" + re.escape(emoji) + "</span>")
        s = pat.sub(lambda m: m.group(1) + svg(icon), s, count=1)
    if "up-outreach" not in s:
        s = s.replace('    <div class="sidebar-nav">\n',
                      '    <div class="sidebar-nav">\n'
                      '      <div class="sidebar-section up-outreach">\n'
                      '        <a href="https://outreach.upraise.uk/app/home" class="sidebar-link">\n'
                      '          ' + svg("outreach") + ' Upraise Outreach\n'
                      '        </a>\n'
                      '      </div>\n', 1)
    return s


edit("templates/dashboard_base.html", dash)


# ---------------------------------------------------------------- page icons: emoji -> line icons (same family as the menu)
P = {   # lucide paths
    "calendar": '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>',
    "calendar-check": ICONS["bookings"],
    "calendar-x": '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/><path d="m14 14-4 4"/><path d="m10 14 4 4"/>',
    "hourglass": '<path d="M5 22h14"/><path d="M5 2h14"/><path d="M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22"/><path d="M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"/>',
    "link": ICONS["invite-links"],
    "sources": ICONS["sources"],
    "globe": '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
    "users": ICONS["teams"],
    "lock": '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
    "share": '<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><path d="m8.59 13.51 6.83 3.98"/><path d="m15.41 6.51-6.82 3.98"/>',
    "refresh": '<path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/>',
    "check": '<path d="M20 6 9 17l-5-5"/>',
    "x": '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    "alert": '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
    "mail": '<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
    "arrow-up-right": '<path d="M7 7h10v10"/><path d="M7 17 17 7"/>',
    "user-plus": '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M19 8v6"/><path d="M22 11h-6"/>',
    "menu": '<path d="M4 12h16"/><path d="M4 6h16"/><path d="M4 18h16"/>',
    "log-out": '<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>',
    "monitor": '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>',
    "sun": '<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>',
    "moon": '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>',
    "clock": '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
    "user": '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
    "map-pin": '<path d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0"/><circle cx="12" cy="10" r="3"/>',
    "video": '<path d="m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5"/><rect x="2" y="6" width="14" height="12" rx="2"/>',
    "phone": '<path d="M13.832 16.568a1 1 0 0 0 1.213-.303l.355-.465A2 2 0 0 1 17 15h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2A18 18 0 0 1 2 4a2 2 0 0 1 2-2h3a2 2 0 0 1 2 2v3a2 2 0 0 1-.8 1.6l-.468.351a1 1 0 0 0-.292 1.233 14 14 0 0 0 6.392 6.384"/>',
    "circle": '<circle cx="12" cy="12" r="9"/>',
}


def ico(name):
    return ('<svg class="up-i up-i-' + name + '" viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" '
            'stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + P[name] + "</svg>")


ENTITY = {128197: "calendar", 128203: "calendar-check", 9203: "hourglass", 8987: "hourglass", 128279: "link",
          127760: "globe", 128101: "users", 128274: "lock", 127970: "share", 128260: "refresh", 10003: "check",
          10007: "x", 9993: "mail", 8599: "arrow-up-right", 8862: "user-plus", 9776: "menu", 8592: "log-out",
          128187: "monitor", 9728: "sun", 127769: "moon", 128339: "clock", 128336: "clock", 128347: "clock",
          128100: "user", 128205: "map-pin", 128683: "calendar-x"}
RAW = {"✓": "check", "✗": "x", "⚠": "alert", "○": "circle"}
STAT_GHOST = re.compile(r'<div style="position: absolute; top: 50%; left: 50%; transform: translate\(-50%, -50%\); font-size: 4rem; '
                        r'opacity: 0\.04; line-height: 1; pointer-events: none;">&#(\d+);</div>')
STAT_CARD = 'class="card" style="text-align: center; padding: 1.25rem; text-decoration: none; color: inherit; position: relative; overflow: hidden;"'
LOCATION = '<span class="slots-info-meta-icon">&#128205;</span>'


def icons(s):
    # overview: faint background emoji on the stat tiles -> a small icon chip; the sources tile gets the sources icon
    s = STAT_GHOST.sub(lambda m: '<div class="up-stat-icon">' + ico("sources" if m.group(1) == "128279" else ENTITY[int(m.group(1))]) + "</div>", s)
    s = s.replace(STAT_CARD, STAT_CARD.replace('class="card"', 'class="card up-stat"'))
    # booking page: the location icon follows the location (video / phone / in person)
    s = s.replace(LOCATION, '<span class="slots-info-meta-icon">{% if event_type.location_type is defined and event_type.location_type == "in_person" %}'
                  + ico("map-pin") + '{% elif event_type.location_type is defined and event_type.location_type == "phone" %}' + ico("phone")
                  + "{% else %}" + ico("video") + "{% endif %}</span>")
    s = s.replace("&#9728;&#65039;", "&#9728;")
    s = re.sub(r"&#(\d{4,6});", lambda m: ico(ENTITY[int(m.group(1))]) if int(m.group(1)) in ENTITY else m.group(0), s)
    for ch, name in RAW.items():
        s = s.replace(ch, ico(name))
    return s


for f in sorted((SRC / "templates").rglob("*.html")):
    edit(f.relative_to(SRC), icons)

print("changed:", *changed, sep="\n  ") if changed else print("already applied")
