//! Optional Google Analytics 4 on chosen public booking pages (Upraise).
//!
//! Off unless `CALRS_GA4_ID` is set. Then the Google tag loads only on the
//! event types listed in `CALRS_GA4_PAGES` (their slot page, booking form and
//! confirmation), never on the dashboard, admin, login or any other event
//! type, and the CSP is widened for those pages only.
//!
//! - `CALRS_GA4_ID`: the GA4 measurement ID, `G-XXXXXXX`.
//! - `CALRS_GA4_PAGES`: comma-separated event type paths, `/team/bd/intro-call`
//!   or `/u/alice/intro`. Nothing listed means nothing is tracked.
//! - `CALRS_GA4_EVENT`: the GA4 event sent once a booking is made (confirmed or
//!   waiting for approval), to be marked as a key event. Default `book_call`.
//! - `CALRS_GA4_LINKER_DOMAINS`: comma-separated domains that share the GA4
//!   session with these pages (cross-domain measurement), for example the
//!   marketing site that links here.
//!
//! The event carries the event type path and whether the booking is confirmed
//! or pending, never the guest's name, email, phone or notes.

use serde::Serialize;

pub const ID_VAR: &str = "CALRS_GA4_ID";
pub const PAGES_VAR: &str = "CALRS_GA4_PAGES";
pub const EVENT_VAR: &str = "CALRS_GA4_EVENT";
pub const LINKER_VAR: &str = "CALRS_GA4_LINKER_DOMAINS";
const DEFAULT_EVENT: &str = "book_call";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ga4Config {
    pub id: String,
    pub pages: Vec<String>,
    pub event: String,
    pub linker_domains: Vec<String>,
}

fn list(v: Option<String>) -> Vec<String> {
    v.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn valid_id(id: &str) -> bool {
    id.len() > 2
        && id.len() <= 20
        && id.starts_with("G-")
        && id[2..]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

// GA4 event names: a letter, then letters, digits or underscores, at most 40.
fn valid_event(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && name.len() <= 40
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_page(p: &str) -> bool {
    let parts: Vec<&str> = p.trim_matches('/').split('/').collect();
    p.starts_with('/')
        && matches!(parts.as_slice(), ["team" | "u", a, b] if !a.is_empty() && !b.is_empty())
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn valid_domain(d: &str) -> bool {
    d.contains('.')
        && d.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

impl Ga4Config {
    /// Reads the configuration through `get` (the process environment in
    /// production). Invalid entries are dropped with a warning, and an
    /// invalid ID or event name switches tracking off.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let id = get(ID_VAR)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if id.is_empty() {
            return None;
        }
        if !valid_id(&id) {
            tracing::warn!("{ID_VAR} is not a GA4 measurement ID (G-...); Google Analytics is off");
            return None;
        }
        let event = get(EVENT_VAR)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_EVENT.to_string());
        if !valid_event(&event) {
            tracing::warn!("{EVENT_VAR} is not a valid GA4 event name; Google Analytics is off");
            return None;
        }
        let mut pages = Vec::new();
        for p in list(get(PAGES_VAR)) {
            let p = p.trim_end_matches('/').to_string();
            if valid_page(&p) {
                pages.push(p);
            } else {
                tracing::warn!("{PAGES_VAR}: ignoring {p:?} (expected /team/<team>/<event> or /u/<user>/<event>)");
            }
        }
        let mut linker_domains = Vec::new();
        for d in list(get(LINKER_VAR)) {
            let d = d.to_ascii_lowercase();
            if valid_domain(&d) {
                linker_domains.push(d);
            } else {
                tracing::warn!("{LINKER_VAR}: ignoring {d:?}");
            }
        }
        Some(Ga4Config {
            id,
            pages,
            event,
            linker_domains,
        })
    }

    pub fn from_env() -> Option<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// Whether a request path is a tracked event type's slot page or booking
    /// form (the confirmation is the booking form's POST response).
    pub fn tracks(&self, path: &str) -> bool {
        let path = path.trim_end_matches('/');
        let page = path.strip_suffix("/book").unwrap_or(path);
        self.pages.iter().any(|p| p == page)
    }
}

/// The CSP with the origins the Google tag sends to added to `script-src`,
/// `img-src` and `connect-src`: Google's documented list for GA4, plus the
/// Google signals endpoints (`*.g.doubleclick.net`, `www.google.com`) and the
/// bare `analytics.google.com`, which `*.analytics.google.com` does not cover.
pub fn extend_csp(csp: &str) -> String {
    const SCRIPT: &str = " https://*.googletagmanager.com";
    const IMG: &str = " https://*.google-analytics.com https://*.googletagmanager.com https://*.g.doubleclick.net https://*.google.com";
    const CONNECT: &str = " https://*.google-analytics.com https://analytics.google.com https://*.analytics.google.com https://*.googletagmanager.com https://*.g.doubleclick.net https://*.google.com";
    csp.split(';')
        .map(|d| {
            let t = d.trim();
            let extra = if t.starts_with("script-src ") {
                SCRIPT
            } else if t.starts_with("img-src ") {
                IMG
            } else if t.starts_with("connect-src ") {
                CONNECT
            } else {
                ""
            };
            format!("{t}{extra}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(vars: &[(&str, &str)]) -> Option<Ga4Config> {
        let m: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ga4Config::from_vars(|k| m.get(k).cloned())
    }

    #[test]
    fn off_without_an_id_or_with_a_bad_one() {
        assert_eq!(cfg(&[]), None);
        assert_eq!(cfg(&[(ID_VAR, " ")]), None);
        assert_eq!(cfg(&[(ID_VAR, "UA-1234-1")]), None);
        assert_eq!(cfg(&[(ID_VAR, "G-ABC123\"><script>")]), None);
        assert_eq!(cfg(&[(ID_VAR, "G-ABC123"), (EVENT_VAR, "book call")]), None);
    }

    #[test]
    fn reads_pages_event_and_domains() {
        let c = cfg(&[
            (ID_VAR, "G-GXTC85E4TB"),
            (
                PAGES_VAR,
                "/team/bd/intro-call/, /u/alice/chat, /dashboard, team/x/y, /team/bd/a\"b",
            ),
            (LINKER_VAR, "UpraiseSEO.com, book.upraise.uk, bad domain"),
        ])
        .unwrap();
        assert_eq!(c.event, "book_call");
        assert_eq!(c.pages, vec!["/team/bd/intro-call", "/u/alice/chat"]);
        assert_eq!(c.linker_domains, vec!["upraiseseo.com", "book.upraise.uk"]);
        let c = cfg(&[(ID_VAR, "G-A1"), (EVENT_VAR, "intro_call_booked")]).unwrap();
        assert_eq!((c.event.as_str(), c.pages.len()), ("intro_call_booked", 0));
    }

    #[test]
    fn tracks_only_the_listed_event_types() {
        let c = cfg(&[(ID_VAR, "G-A1"), (PAGES_VAR, "/team/bd/intro-call")]).unwrap();
        assert!(c.tracks("/team/bd/intro-call"));
        assert!(c.tracks("/team/bd/intro-call/"));
        assert!(c.tracks("/team/bd/intro-call/book"));
        assert!(!c.tracks("/team/bd"));
        assert!(!c.tracks("/team/bd/intro-call-2"));
        assert!(!c.tracks("/team/bd/other/book"));
        assert!(!c.tracks("/dashboard"));
    }

    // CSP host-source matching, enough for these sources: exact, or "*." for any subdomain.
    fn allows(src: &str, host: &str) -> bool {
        match src.strip_prefix("https://*.") {
            Some(base) => host
                .strip_prefix("https://")
                .is_some_and(|h| h.ends_with(&format!(".{base}"))),
            None => src == host,
        }
    }

    #[test]
    fn csp_gains_google_origins_only_where_needed() {
        let base = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
                    script-src 'self' 'unsafe-inline'; connect-src 'self'; object-src 'none'";
        let out = extend_csp(base);
        assert!(out.contains("script-src 'self' 'unsafe-inline' https://*.googletagmanager.com;"));
        assert!(out.contains("img-src 'self' data: https://*.google-analytics.com https://*.googletagmanager.com https://*.g.doubleclick.net https://*.google.com;"));
        // Every host gtag.js was seen sending to from book.upraise.uk.
        let connect = out
            .split("; ")
            .find(|d| d.starts_with("connect-src"))
            .unwrap();
        for host in [
            "https://analytics.google.com",
            "https://region1.analytics.google.com",
            "https://region1.google-analytics.com",
            "https://www.google.com",
            "https://stats.g.doubleclick.net",
            "https://www.googletagmanager.com",
        ] {
            assert!(
                connect.split(' ').skip(1).any(|src| allows(src, host)),
                "{host} blocked"
            );
        }
        assert!(!connect
            .split(' ')
            .any(|src| allows(src, "https://evil.example")));
        assert!(out.contains("default-src 'self'; ") && out.ends_with("object-src 'none'"));
        assert!(!out.contains("style-src 'self' 'unsafe-inline' https"));
    }
}
