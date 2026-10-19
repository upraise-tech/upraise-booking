//! Team directory and group meetings.
//!
//! The directory lists every member so anyone can find a colleague's booking page and book them.
//! A group meeting picks any members, whatever their teams, and shows the times when all of them
//! are free. It is built on dynamic group links (`/u/a+b/slug`): the first person's meeting type
//! sets the length, location and hours, everyone's calendars and working hours block time, and
//! booking a time goes through the normal dynamic-group booking (invites to everyone, write-back).

use super::*;

/// How many days ahead the group meeting page looks for common free times.
const GROUP_MEETING_DAYS: i32 = 14;
/// How many days with common times the page lists before pointing to the full booking page.
const GROUP_MEETING_MAX_DAYS_SHOWN: usize = 7;
/// Longest wait for the members' calendars to sync before showing times from the cache.
const GROUP_MEETING_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

type MemberRow = (
    String,         // id
    String,         // username
    String,         // name
    Option<String>, // title
    Option<String>, // avatar_path
    bool,           // allow_dynamic_group
);

async fn enabled_members(pool: &SqlitePool) -> Vec<MemberRow> {
    sqlx::query_as(
        "SELECT id, username, name, title, avatar_path, allow_dynamic_group FROM users
         WHERE enabled = 1 AND username IS NOT NULL AND username <> ''
         ORDER BY lower(name)",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Team names per user, as the viewer may see them: public teams, and private teams the viewer
/// belongs to (an admin sees every team).
async fn visible_memberships(
    pool: &SqlitePool,
    viewer_id: &str,
    viewer_is_admin: bool,
) -> (
    HashMap<String, Vec<(String, String)>>,
    Vec<(String, String)>,
) {
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT tm.user_id, t.name, t.slug, t.visibility,
                EXISTS (SELECT 1 FROM team_members v WHERE v.team_id = t.id AND v.user_id = ?)
         FROM team_members tm JOIN teams t ON t.id = tm.team_id
         ORDER BY lower(t.name)",
    )
    .bind(viewer_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut by_user: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut teams: Vec<(String, String)> = Vec::new();
    for (user_id, name, slug, visibility, viewer_in) in rows {
        if visibility != "public" && !viewer_in && !viewer_is_admin {
            continue;
        }
        if !teams.iter().any(|(s, _)| s == &slug) {
            teams.push((slug.clone(), name.clone()));
        }
        let list = by_user.entry(user_id).or_default();
        if !list.iter().any(|(s, _)| s == &slug) {
            list.push((slug, name));
        }
    }
    (by_user, teams)
}

#[derive(Deserialize)]
pub(super) struct PeopleQuery {
    #[serde(default)]
    team: Option<String>,
}

/// `GET /dashboard/people`: everyone, their teams and the meetings you can book with them.
pub(super) async fn dashboard_people(
    State(state): State<Arc<AppState>>,
    auth_user: crate::auth::AuthUser,
    Query(query): Query<PeopleQuery>,
) -> impl IntoResponse {
    let user = &auth_user.user;
    let members = enabled_members(&state.pool).await;
    let (memberships, teams) =
        visible_memberships(&state.pool, &user.id, user.role == "admin").await;

    // Personal meeting types: public ones link straight to the booking page; internal ones get an
    // invite link (any signed-in member may make one, as on the Invite links page).
    let ets: Vec<(String, String, String, i32, String, String)> = sqlx::query_as(
        "SELECT et.id, et.slug, et.title, et.duration_min, et.visibility, a.user_id
         FROM event_types et JOIN accounts a ON a.id = et.account_id
         WHERE et.enabled = 1 AND et.team_id IS NULL AND et.visibility IN ('public', 'internal')
         ORDER BY et.duration_min, et.title",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let people: Vec<minijinja::Value> = members
        .iter()
        .map(|(id, username, name, title, avatar, allow_group)| {
            let mine: Vec<&(String, String, String, i32, String, String)> =
                ets.iter().filter(|e| &e.5 == id).collect();
            let public_ets: Vec<minijinja::Value> = mine
                .iter()
                .filter(|e| e.4 == "public")
                .map(|e| context! { slug => e.1, title => e.2, duration_min => e.3 })
                .collect();
            let internal_ets: Vec<minijinja::Value> = mine
                .iter()
                .filter(|e| e.4 == "internal")
                .map(|e| context! { id => e.0, title => e.2, duration_min => e.3 })
                .collect();
            let user_teams = memberships.get(id).cloned().unwrap_or_default();
            context! {
                id => id,
                username => username,
                name => name,
                title => title.as_deref().unwrap_or(""),
                has_avatar => avatar.is_some(),
                initials => compute_initials(name),
                is_me => id == &user.id,
                can_group => *allow_group,
                teams => user_teams.iter().map(|(slug, name)| context! { slug => slug, name => name }).collect::<Vec<_>>(),
                team_slugs => user_teams.iter().map(|(slug, _)| slug.as_str()).collect::<Vec<_>>().join(" "),
                search => format!("{} {} {}", name, title.as_deref().unwrap_or(""), username).to_lowercase(),
                public_ets => public_ets,
                internal_ets => internal_ets,
            }
        })
        .collect();

    let selected_team = query
        .team
        .as_deref()
        .filter(|t| teams.iter().any(|(s, _)| s == t))
        .unwrap_or("");
    let tmpl = match state.templates.get_template("dashboard_people.html") {
        Ok(t) => t,
        Err(e) => return internal_error_html("template render", &e),
    };
    let (impersonating, impersonating_name, _) = impersonation_ctx(&auth_user);
    Html(
        tmpl.render(context! {
            sidebar => sidebar_context(&auth_user, "people"),
            lang => auth_user.lang,
            impersonating => impersonating,
            impersonating_name => impersonating_name,
            people => people,
            teams => teams.iter().map(|(slug, name)| context! { slug => slug, name => name }).collect::<Vec<_>>(),
            selected_team => selected_team,
        })
        .unwrap_or_else(|e| internal_error_body("template render", &e)),
    )
}

#[derive(Deserialize)]
pub(super) struct GroupMeetingQuery {
    /// Usernames of the other people, comma separated, in the order picked.
    #[serde(default)]
    with: Option<String>,
    /// The meeting type (an id of the first person's public meeting types).
    #[serde(default)]
    et: Option<String>,
    /// "0" leaves the signed-in member out of the meeting.
    #[serde(default)]
    me: Option<String>,
}

/// The people in a group meeting, in order: the signed-in member first unless left out, then the
/// picked members. Unknown, disabled, duplicate and group-links-off usernames are dropped.
pub(super) fn group_meeting_people<'a>(
    members: &'a [MemberRow],
    me_id: &str,
    include_me: bool,
    picked: &[String],
) -> Vec<&'a MemberRow> {
    let mut out: Vec<&MemberRow> = Vec::new();
    if include_me {
        if let Some(me) = members.iter().find(|m| m.0 == me_id) {
            out.push(me);
        }
    }
    for uname in picked {
        let uname = uname.trim().to_lowercase();
        if let Some(m) = members
            .iter()
            .find(|m| m.1.to_lowercase() == uname && m.0 != me_id && m.5)
        {
            if !out.iter().any(|o| o.0 == m.0) {
                out.push(m);
            }
        }
    }
    out
}

/// `GET /dashboard/group-meeting`: pick people and a meeting type, see when everyone is free,
/// book a time or copy a link for the guest.
pub(super) async fn group_meeting_page(
    State(state): State<Arc<AppState>>,
    auth_user: crate::auth::AuthUser,
    Query(query): Query<GroupMeetingQuery>,
) -> impl IntoResponse {
    let user = &auth_user.user;
    let lang = auth_user.lang;
    let include_me = query.me.as_deref() != Some("0");
    let members = enabled_members(&state.pool).await;
    let picked: Vec<String> = query
        .with
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .collect();
    let mut chosen = group_meeting_people(&members, &user.id, include_me, &picked);
    let me_row = members.iter().find(|m| m.0 == user.id);
    let me_group_off = include_me && me_row.map(|m| !m.5).unwrap_or(false);

    // The first person with a public personal meeting type leads the meeting: their meeting type
    // sets the length, location and hours (the link is `/u/lead+others/slug`). That is the
    // signed-in member when they have one, else the first picked person who does.
    let mut owner_ets: Vec<(String, String, String, i32, String)> = vec![];
    for i in 0..chosen.len() {
        let ets: Vec<(String, String, String, i32, String)> = sqlx::query_as(
            "SELECT et.id, et.slug, et.title, et.duration_min, et.location_type
             FROM event_types et JOIN accounts a ON a.id = et.account_id
             WHERE a.user_id = ? AND et.enabled = 1 AND et.team_id IS NULL AND et.visibility = 'public'
             ORDER BY et.duration_min, et.title",
        )
        .bind(&chosen[i].0)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        if !ets.is_empty() {
            let lead = chosen.remove(i);
            chosen.insert(0, lead);
            owner_ets = ets;
            break;
        }
    }
    let owner = chosen.first().copied();
    let selected_et = query
        .et
        .as_deref()
        .and_then(|id| owner_ets.iter().find(|e| e.0 == id))
        .or_else(|| owner_ets.first());

    let guest_tz = parse_guest_tz(Some(&user.timezone));
    let guest_tz_name = guest_tz.name().to_string();
    let mut days_ctx: Vec<minijinja::Value> = vec![];
    let mut more_days = false;
    let mut share_url = String::new();
    let mut booking_path = String::new();
    let mut meet_blocked = false;
    let ready = chosen.len() >= 2 && selected_et.is_some() && !me_group_off;

    if ready {
        let (et_id, et_slug, _title, _duration, location_type) = selected_et.unwrap();
        let combined = chosen
            .iter()
            .map(|m| m.1.as_str())
            .collect::<Vec<_>>()
            .join("+");
        booking_path = format!("/u/{}/{}", combined, et_slug);
        share_url = format!(
            "{}{}",
            crate::settings::base_url()
                .unwrap_or_default()
                .trim_end_matches('/'),
            booking_path
        );
        meet_blocked = location_type == meeting::LOCATION_TYPE_GOOGLE_MEET
            && !crate::google_meet::user_has_google_writeback(&state.pool, &chosen[0].0).await;
        if !meet_blocked {
            let days = common_free_times(&state, et_id, &chosen, guest_tz).await;
            more_days = days.len() > GROUP_MEETING_MAX_DAYS_SHOWN;
            days_ctx = days
                .iter()
                .take(GROUP_MEETING_MAX_DAYS_SHOWN)
                .map(|d| {
                    let slots: Vec<minijinja::Value> = d
                        .slots
                        .iter()
                        .map(|s| {
                            let date = if s.guest_date.is_empty() {
                                d.date.as_str()
                            } else {
                                s.guest_date.as_str()
                            };
                            context! {
                                start => s.start,
                                href => format!(
                                    "{}/book?date={}&time={}&tz={}",
                                    booking_path,
                                    date,
                                    s.start,
                                    urlencoding::encode(&guest_tz_name)
                                ),
                            }
                        })
                        .collect();
                    context! { date => d.date, label => d.label, slots => slots }
                })
                .collect();
        }
    }

    let picked_ids: Vec<&str> = chosen
        .iter()
        .filter(|m| m.0 != user.id)
        .map(|m| m.0.as_str())
        .collect();
    let candidates: Vec<minijinja::Value> = members
        .iter()
        .filter(|m| m.0 != user.id)
        .map(|(id, username, name, title, avatar, allow_group)| {
            context! {
                id => id,
                username => username,
                name => name,
                title => title.as_deref().unwrap_or(""),
                has_avatar => avatar.is_some(),
                initials => compute_initials(name),
                can_group => *allow_group,
                picked => picked_ids.contains(&id.as_str()),
            }
        })
        .collect();

    let tmpl = match state.templates.get_template("dashboard_group_meeting.html") {
        Ok(t) => t,
        Err(e) => return internal_error_html("template render", &e),
    };
    let (impersonating, impersonating_name, _) = impersonation_ctx(&auth_user);
    Html(
        tmpl.render(context! {
            sidebar => sidebar_context(&auth_user, "group-meeting"),
            lang => lang,
            impersonating => impersonating,
            impersonating_name => impersonating_name,
            candidates => candidates,
            include_me => include_me,
            me_group_off => me_group_off,
            people => chosen.iter().map(|m| context! {
                id => m.0, name => m.2, has_avatar => m.4.is_some(), initials => compute_initials(&m.2),
            }).collect::<Vec<_>>(),
            people_count => chosen.len(),
            owner_name => owner.map(|o| o.2.as_str()).unwrap_or(""),
            owner_is_me => owner.map(|o| o.0 == user.id).unwrap_or(false),
            event_types => owner_ets.iter().map(|e| context! {
                id => e.0, title => e.2, duration_min => e.3,
                selected => selected_et.map(|s| s.0 == e.0).unwrap_or(false),
            }).collect::<Vec<_>>(),
            ready => ready,
            meet_blocked => meet_blocked,
            days => days_ctx,
            more_days => more_days,
            share_url => share_url,
            booking_path => booking_path,
            guest_tz => guest_tz_name,
            horizon_days => GROUP_MEETING_DAYS,
        })
        .unwrap_or_else(|e| internal_error_body("template render", &e)),
    )
}

/// The times when everyone in `people` is free for the meeting type, over the next
/// GROUP_MEETING_DAYS days, shown in `guest_tz`. Calendars are refreshed first, but never for
/// longer than GROUP_MEETING_SYNC_TIMEOUT (the booking itself checks again when it is made).
async fn common_free_times(
    state: &AppState,
    et_id: &str,
    people: &[&MemberRow],
    guest_tz: Tz,
) -> Vec<SlotDay> {
    let mut sync_tasks = tokio::task::JoinSet::new();
    for m in people {
        let pool = state.pool.clone();
        let key = state.secret_key;
        let uid = m.0.clone();
        sync_tasks.spawn(async move {
            crate::commands::sync::sync_if_stale(&pool, &key, &uid).await;
        });
    }
    let _ = tokio::time::timeout(GROUP_MEETING_SYNC_TIMEOUT, async {
        while sync_tasks.join_next().await.is_some() {}
    })
    .await;

    let et: Option<(i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT duration_min, buffer_before, buffer_after, min_notice_min FROM event_types WHERE id = ?",
    )
    .bind(et_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);
    let Some((duration, buf_before, buf_after, min_notice)) = et else {
        return vec![];
    };
    let host_tz = get_host_tz(&state.pool, et_id).await;
    let horizon = get_booking_horizon(&state.pool, et_id).await;
    let days_ahead = match horizon {
        Some(h) if h > 0 => GROUP_MEETING_DAYS.min(h),
        _ => GROUP_MEETING_DAYS,
    };
    let now_host = Utc::now().with_timezone(&host_tz).naive_local();
    let window_end = (now_host.date() + Duration::days(days_ahead as i64 + 1))
        .and_hms_opt(23, 59, 59)
        .unwrap_or(now_host);
    let user_ids: Vec<String> = people.iter().map(|m| m.0.clone()).collect();
    let busy =
        dynamic_group_busy_source(&state.pool, et_id, &user_ids, now_host, window_end, host_tz)
            .await;
    compute_slots(
        &state.pool,
        et_id,
        duration,
        buf_before,
        buf_after,
        min_notice,
        0,
        days_ahead,
        horizon,
        host_tz,
        guest_tz,
        busy,
        None,
    )
    .await
    .into_iter()
    .filter(|d| !d.slots.is_empty())
    .collect()
}
