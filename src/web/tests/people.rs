use super::*;

/// minijinja escapes `/` and `&` in attributes; compare against the plain text.
fn unescaped(body: String) -> String {
    body.replace("&#x2f;", "/").replace("&amp;", "&")
}

async fn add_team(pool: &SqlitePool, name: &str, slug: &str, visibility: &str, members: &[&str]) {
    let team_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO teams (id, name, slug, visibility) VALUES (?, ?, ?, ?)")
        .bind(&team_id)
        .bind(name)
        .bind(slug)
        .bind(visibility)
        .execute(pool)
        .await
        .unwrap();
    for user_id in members {
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id, role, source) VALUES (?, ?, 'member', 'direct')",
        )
        .bind(&team_id)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn people_page_requires_sign_in() {
    let (app, _pool, _session, _) = setup_test_app().await;
    let response = app.oneshot(get("/dashboard/people")).await.unwrap();
    assert!(response.status().is_redirection());
}

#[tokio::test]
async fn people_page_lists_members_teams_and_booking_links() {
    let (app, pool, _admin_session, _) = setup_test_app().await;
    // A regular member looks at the directory (an admin sees every team).
    let (me, session) =
        insert_test_user_with_session(&pool, "Vera Viewer", "vera@example.com", "vera").await;
    let (bob, _) =
        insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    let (carol, _) =
        insert_test_user_with_session(&pool, "Carol Secret", "carol@example.com", "carol").await;
    let (dave, _) =
        insert_test_user_with_session(&pool, "Dave Gone", "dave@example.com", "dave").await;
    sqlx::query("UPDATE users SET enabled = 0 WHERE id = ?")
        .bind(&dave)
        .execute(&pool)
        .await
        .unwrap();
    let bob_et = insert_personal_et(&pool, &bob, "bob-intro").await;
    let internal_et = insert_personal_et(&pool, &bob, "bob-internal").await;
    sqlx::query(
        "UPDATE event_types SET visibility = 'internal', title = 'Bob internal' WHERE id = ?",
    )
    .bind(&internal_et)
    .execute(&pool)
    .await
    .unwrap();
    let private_et = insert_personal_et(&pool, &bob, "bob-private").await;
    sqlx::query(
        "UPDATE event_types SET visibility = 'private', title = 'Bob private' WHERE id = ?",
    )
    .bind(&private_et)
    .execute(&pool)
    .await
    .unwrap();
    add_team(&pool, "SEO Squad", "seo", "public", &[&bob, &me]).await;
    add_team(&pool, "Board Room", "board", "private", &[&carol]).await;

    let response = app
        .oneshot(get_authed("/dashboard/people", &session))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = unescaped(body_string(response).await);
    assert!(body.contains("Bob Builder") && body.contains("Carol Secret"));
    assert!(
        !body.contains("Dave Gone"),
        "disabled members are not listed"
    );
    assert!(
        body.contains("href=\"/u/bob/bob-intro\""),
        "public meeting types link to the booking page"
    );
    assert!(
        body.contains(&format!("data-et-id=\"{}\"", internal_et)),
        "internal ones get an invite link"
    );
    assert!(
        !body.contains("Bob private"),
        "private meeting types stay hidden"
    );
    assert!(body.contains("SEO Squad"));
    assert!(
        !body.contains("Board Room"),
        "a private team is shown only to its members"
    );
    let _ = bob_et;
}

#[tokio::test]
async fn group_meeting_shows_common_times_and_a_guest_link() {
    let (app, pool, session, _) = setup_test_app().await;
    insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;

    let response = app
        .oneshot(get_authed("/dashboard/group-meeting?with=bob", &session))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = unescaped(body_string(response).await);
    assert!(
        body.contains("/u/testuser+bob/test-meeting"),
        "the guest link is the dynamic group link"
    );
    assert!(
        body.contains("/u/testuser+bob/test-meeting/book?date="),
        "each common time links to the booking form"
    );
}

#[tokio::test]
async fn group_meeting_skips_a_time_when_one_person_is_busy() {
    let (app, pool, session, _) = setup_test_app().await;
    let (bob, _) =
        insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    // Bob is booked solid on his own meeting type for the next three weeks.
    let bob_et = insert_personal_et(&pool, &bob, "bob-intro").await;
    let start = (Utc::now() - Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let end = (Utc::now() + Duration::days(21))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    sqlx::query(
        "INSERT INTO bookings (id, event_type_id, uid, guest_name, guest_email, guest_timezone, start_at, end_at, status, cancel_token, reschedule_token)
         VALUES (?, ?, ?, 'G', 'g@example.com', 'UTC', ?, ?, 'confirmed', ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&bob_et)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&start)
    .bind(&end)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(uuid::Uuid::new_v4().to_string())
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(get_authed("/dashboard/group-meeting?with=bob", &session))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(
        !body.contains("/book?date="),
        "no time is offered while Bob is busy"
    );
    assert!(
        body.contains("/u/testuser+bob/test-meeting"),
        "the guest link is still there"
    );
}

#[tokio::test]
async fn group_meeting_without_me_starts_from_the_first_person() {
    let (app, pool, session, _) = setup_test_app().await;
    insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    insert_test_user_with_session(&pool, "Carol Smith", "carol@example.com", "carol").await;

    // Only Bob picked and me left out: one person is not a group.
    let response = app
        .clone()
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob&me=0",
            &session,
        ))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(!body.contains("/u/"), "no link with fewer than two people");

    // Neither Bob nor Carol has a public meeting type: nothing to book.
    let response = app
        .clone()
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob,carol&me=0",
            &session,
        ))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(body.contains("Bob Builder"));
    assert!(
        !body.contains("/u/bob+carol/"),
        "nothing to book without a meeting type"
    );

    // Carol has one: she leads the meeting even though Bob was picked first.
    let carol = sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE username = 'carol'")
        .fetch_one(&pool)
        .await
        .unwrap();
    insert_personal_et(&pool, &carol, "carol-call").await;
    let response = app
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob,carol&me=0",
            &session,
        ))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(
        body.contains("/u/carol+bob/carol-call"),
        "the person with a meeting type leads"
    );
}

#[test]
fn group_meeting_people_keeps_order_and_drops_the_wrong_ones() {
    let row = |id: &str,
               uname: &str,
               allow: bool|
     -> (String, String, String, Option<String>, Option<String>, bool) {
        (id.into(), uname.into(), uname.into(), None, None, allow)
    };
    let members = vec![
        row("me", "me", true),
        row("b", "bob", true),
        row("c", "carol", false),
        row("d", "dave", true),
    ];
    let picked: Vec<String> = ["dave", "bob", "BOB", "carol", "nobody", "me"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let with_me = crate::web::people::group_meeting_people(&members, "me", true, &picked);
    let ids: Vec<&str> = with_me.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(
        ids,
        vec!["me", "d", "b"],
        "me first, then picks in order; group-off, unknown and duplicates dropped"
    );
    let without_me = crate::web::people::group_meeting_people(&members, "me", false, &picked);
    let ids: Vec<&str> = without_me.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(ids, vec!["d", "b"]);
}

#[tokio::test]
async fn new_event_types_allow_ten_extra_guests() {
    let (app, pool, session, _) = setup_test_app().await;
    let csrf = "test-csrf-guests";
    let body = format!(
        "_csrf={}&title=Guests&slug=guests-default&duration_min=30&location_value=https%3A%2F%2Fmeet.example.com&avail_days=1,2,3,4,5&avail_start=09:00&avail_end=17:00",
        csrf
    );
    let response = app
        .clone()
        .oneshot(post_form(
            "/dashboard/event-types/new",
            &session,
            csrf,
            &body,
        ))
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    let max: i32 = sqlx::query_scalar(
        "SELECT max_additional_guests FROM event_types WHERE slug = 'guests-default'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(max, 10);

    // A host who picks "none" keeps it.
    let body = format!(
        "_csrf={}&title=No+guests&slug=no-guests&duration_min=30&location_value=https%3A%2F%2Fmeet.example.com&avail_days=1,2,3,4,5&avail_start=09:00&avail_end=17:00&max_additional_guests=0",
        csrf
    );
    app.oneshot(post_form(
        "/dashboard/event-types/new",
        &session,
        csrf,
        &body,
    ))
    .await
    .unwrap();
    let max: i32 = sqlx::query_scalar(
        "SELECT max_additional_guests FROM event_types WHERE slug = 'no-guests'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(max, 0);
}

#[tokio::test]
async fn migration_turns_extra_guests_on_for_existing_event_types() {
    let pool = setup_test_db().await;
    let (user_id, _, et_id) = seed_test_data(&pool).await;
    let five = insert_personal_et(&pool, &user_id, "five-guests").await;
    sqlx::query("UPDATE event_types SET max_additional_guests = 0 WHERE id = ?")
        .bind(&et_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE event_types SET max_additional_guests = 5 WHERE id = ?")
        .bind(&five)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/073_additional_guests_default.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let get = |id: String| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i32>(
                "SELECT max_additional_guests FROM event_types WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(get(et_id).await, 10, "switched off becomes up to 10");
    assert_eq!(get(five).await, 5, "a limit the host chose stays");
}

// --- Custom group meeting length ---

const TEST_KEY: [u8; 32] = [0u8; 32];

fn people(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn group_length_signature_round_trips_and_refuses_edits() {
    let who = people(&["testuser", "bob"]);
    let sig = group_length_sig(&TEST_KEY, &who, "test-meeting", 45);
    assert_eq!(sig.len(), 16);
    let check = |who: &[String], slug: &str, len: &str, sig: &str| {
        verified_group_length(&TEST_KEY, who, slug, Some(len), Some(sig))
    };
    assert_eq!(check(&who, "test-meeting", "45", &sig), Some(45));
    assert_eq!(
        check(&people(&["TestUser", "Bob"]), "Test-Meeting", "45", &sig),
        Some(45),
        "usernames and slugs are case-insensitive in links"
    );
    assert_eq!(
        check(&who, "test-meeting", "60", &sig),
        None,
        "another length"
    );
    assert_eq!(
        check(&who, "other", "45", &sig),
        None,
        "another meeting type"
    );
    assert_eq!(
        check(&people(&["testuser", "carol"]), "test-meeting", "45", &sig),
        None,
        "other people"
    );
    assert_eq!(
        check(&people(&["bob", "testuser"]), "test-meeting", "45", &sig),
        None,
        "another lead"
    );
    let mut edited = sig.clone();
    edited.replace_range(0..1, if sig.starts_with('0') { "1" } else { "0" });
    assert_eq!(check(&who, "test-meeting", "45", &edited), None);
    assert_eq!(check(&who, "test-meeting", "45", "zz"), None);
    assert_eq!(check(&who, "test-meeting", "45", &sig[..8]), None);
    assert_eq!(
        verified_group_length(&TEST_KEY, &who, "test-meeting", None, Some(&sig)),
        None
    );
    assert_eq!(
        verified_group_length(&TEST_KEY, &who, "test-meeting", Some("45"), None),
        None
    );
    let other_key = [1u8; 32];
    assert_eq!(
        verified_group_length(&other_key, &who, "test-meeting", Some("45"), Some(&sig)),
        None,
        "another instance's key"
    );
}

#[test]
fn group_length_bounds() {
    assert!(valid_group_length(5));
    assert!(valid_group_length(45));
    assert!(valid_group_length(480));
    assert!(!valid_group_length(0));
    assert!(!valid_group_length(7));
    assert!(!valid_group_length(485));
    assert!(!valid_group_length(-30));
    let who = people(&["a", "b"]);
    let sig = group_length_sig(&TEST_KEY, &who, "x", 7);
    assert_eq!(
        verified_group_length(&TEST_KEY, &who, "x", Some("7"), Some(&sig)),
        None,
        "a signed but invalid length is still refused"
    );
    assert_eq!(group_length_params(&TEST_KEY, &who, "x", None), "");
    assert!(group_length_params(&TEST_KEY, &who, "x", Some(45)).starts_with("&len=45&ls="));
}

#[tokio::test]
async fn group_meeting_custom_length_goes_into_every_link() {
    let (app, pool, session, _) = setup_test_app().await;
    insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    let sig = group_length_sig(&TEST_KEY, &people(&["testuser", "bob"]), "test-meeting", 45);

    let response = app
        .clone()
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob&len=45",
            &session,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = unescaped(body_string(response).await);
    assert!(
        body.contains(&format!("/u/testuser+bob/test-meeting?len=45&ls={sig}")),
        "the guest link carries the signed length"
    );
    assert!(
        body.contains(&format!("&len=45&ls={sig}\"")),
        "each common time books that length"
    );
    assert!(body.contains("/u/testuser+bob/test-meeting/book?date="));
    assert!(body.contains(r#"<option value="45" selected"#));

    // The meeting type's own length needs no custom length in the link.
    let response = app
        .clone()
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob&len=30",
            &session,
        ))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(!body.contains("len=30"));
    assert!(body.contains("/u/testuser+bob/test-meeting/book?date="));

    // A length the page can't use is refused, and the type's own applies.
    let response = app
        .oneshot(get_authed(
            "/dashboard/group-meeting?with=bob&len=7",
            &session,
        ))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(body.contains("Pick a length from 5 to 480 minutes"));
    assert!(!body.contains("len=7&"));
    assert!(body.contains("/u/testuser+bob/test-meeting/book?date="));
}

#[tokio::test]
async fn group_link_pages_keep_the_signed_length() {
    let (app, pool, _, _) = setup_test_app().await;
    insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    let sig = group_length_sig(&TEST_KEY, &people(&["testuser", "bob"]), "test-meeting", 45);

    let response = app
        .clone()
        .oneshot(get(&format!(
            "/u/testuser+bob/test-meeting?len=45&ls={sig}"
        )))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = unescaped(body_string(response).await);
    assert!(
        body.contains(&format!(
            "var lengthParams = \"\\u0026len=45\\u0026ls={sig}\""
        )),
        "the slot page passes the length on to the booking form"
    );

    let date = far_future_monday();
    let response = app
        .clone()
        .oneshot(get(&format!(
            "/u/testuser+bob/test-meeting/book?date={date}&time=10:00&tz=UTC&len=45&ls={sig}"
        )))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(body.contains(r#"<input type="hidden" name="len" value="45">"#));
    assert!(body.contains(&format!(r#"<input type="hidden" name="ls" value="{sig}">"#)));
    assert!(body.contains("10:45"), "the form shows the custom end time");

    // An edited length is ignored: the type's own 30 minutes apply.
    let response = app
        .oneshot(get(&format!(
            "/u/testuser+bob/test-meeting/book?date={date}&time=10:00&tz=UTC&len=240&ls={sig}"
        )))
        .await
        .unwrap();
    let body = unescaped(body_string(response).await);
    assert!(!body.contains(r#"name="len""#));
    assert!(body.contains("10:30"));
}

async fn booked_minutes(pool: &SqlitePool, et_id: &str) -> i64 {
    let (start, end): (String, String) =
        sqlx::query_as("SELECT start_at, end_at FROM bookings WHERE event_type_id = ?")
            .bind(et_id)
            .fetch_one(pool)
            .await
            .unwrap();
    let parse = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.naive_utc())
            .or_else(|_| {
                NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S")
            })
            .unwrap()
    };
    (parse(&end) - parse(&start)).num_minutes()
}

#[tokio::test]
async fn group_booking_takes_the_signed_length() {
    let (app, pool, _, et_id) = setup_test_app().await;
    let (bob, _) =
        insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    insert_personal_et(&pool, &bob, "bob-intro").await;
    let sig = group_length_sig(&TEST_KEY, &people(&["testuser", "bob"]), "test-meeting", 45);

    let csrf = "csrf-group-length";
    let response = app
        .oneshot(post_form_unauthed(
            "/u/testuser+bob/test-meeting/book",
            csrf,
            // 45-minute slots start at 09:00, 09:45, 10:30...
            &format!(
                "{}&len=45&ls={sig}",
                book_body(csrf).replace("time=10:00", "time=09:45")
            ),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(booked_minutes(&pool, &et_id).await, 45);
}

#[tokio::test]
async fn group_booking_ignores_an_unsigned_length() {
    let (app, pool, _, et_id) = setup_test_app().await;
    let (bob, _) =
        insert_test_user_with_session(&pool, "Bob Builder", "bob@example.com", "bob").await;
    insert_personal_et(&pool, &bob, "bob-intro").await;

    let csrf = "csrf-group-length-bad";
    let response = app
        .oneshot(post_form_unauthed(
            "/u/testuser+bob/test-meeting/book",
            csrf,
            &format!("{}&len=240&ls=0000000000000000", book_body(csrf)),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(booked_minutes(&pool, &et_id).await, 30);
}
