//! Google Analytics on chosen booking pages (CALRS_GA4_*): the tag, its CSP
//! and the booking event appear on the listed event type only.
use super::*;
use crate::test_support::{EnvGuard, ENV_LOCK};

const CSP: &str = "content-security-policy";

async fn get_page(app: &Router, uri: &str) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{uri}");
    let csp = response.headers()[CSP].to_str().unwrap().to_string();
    (body_string(response).await, csp)
}

fn set_ga4() {
    std::env::set_var("CALRS_GA4_ID", "G-TEST123");
    std::env::set_var("CALRS_GA4_PAGES", "/team/horizon-team/team-sync");
    std::env::set_var(
        "CALRS_GA4_LINKER_DOMAINS",
        "upraiseseo.com, book.upraise.uk",
    );
}

#[tokio::test]
async fn listed_event_type_loads_the_tag_with_cross_domain() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::new();
    set_ga4();
    let (app, pool, _, _) = setup_test_app().await;
    seed_team_event_type(&pool).await;

    let (body, csp) = get_page(&app, "/team/horizon-team/team-sync").await;
    assert!(body.contains("googletagmanager.com/gtag/js?id=G-TEST123"));
    assert!(body.contains(
        r#"linker: {domains: ["upraiseseo.com","book.upraise.uk"], accept_incoming: true}"#
    ));
    assert!(
        !body.contains("book_call"),
        "no booking event before a booking"
    );
    assert!(csp.contains("script-src 'self' 'unsafe-inline' https://*.googletagmanager.com"));
    assert!(csp.contains("https://*.google-analytics.com"));

    // Another event type stays untracked, with the strict CSP.
    let (body, csp) = get_page(&app, "/u/testuser/test-meeting").await;
    assert!(!body.contains("gtag"));
    assert!(!csp.contains("google"));
}

#[tokio::test]
async fn booking_the_listed_event_type_sends_the_booking_event_without_guest_details() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::new();
    set_ga4();
    let (app, pool, _, _) = setup_test_app().await;
    let (_, et_id) = seed_team_event_type(&pool).await;

    let csrf = "csrf-ga4-team";
    let response = app
        .clone()
        .oneshot(post_form_unauthed(
            "/team/horizon-team/team-sync/book",
            csrf,
            &book_body(csrf),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers()[CSP]
        .to_str()
        .unwrap()
        .contains("googletagmanager"));
    let body = body_string(response).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bookings WHERE event_type_id = ?")
        .bind(&et_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let line = body
        .lines()
        .find(|l| l.contains("gtag('event'"))
        .expect("booking event on the confirmation page");
    assert_eq!(
        line.trim(),
        r#"gtag('event', "book_call", {event_type: "/team/horizon-team/team-sync", booking_status: "confirmed"});"#
    );
}

#[tokio::test]
async fn nothing_is_loaded_when_ga4_is_not_configured() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::new();
    let (app, pool, _, _) = setup_test_app().await;
    seed_team_event_type(&pool).await;
    let (body, csp) = get_page(&app, "/team/horizon-team/team-sync").await;
    assert!(!body.contains("gtag"));
    assert!(!csp.contains("google"));
}
