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
