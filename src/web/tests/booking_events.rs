//! Booking events (src/web/booking_events.rs) end to end: a real database, a local receiver, and a booking taken
//! through confirmed → moved → cancelled.

use super::*;

const KEY: [u8; 32] = [7u8; 32];

/// A receiver on a local port that records each request (raw HTTP) and answers 200.
async fn spawn_receiver() -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = received.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            sink.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).into_owned());
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await;
        }
    });
    (port, received)
}

fn header(req: &str, name: &str) -> Option<String> {
    let head = req.split("\r\n\r\n").next()?;
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().to_string())
    })
}

fn body(req: &str) -> String {
    req.split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default()
}

async fn configure(pool: &SqlitePool, url: &str) {
    let secret = crate::crypto::encrypt_value(&KEY, "s3cret").unwrap();
    let n = sqlx::query(
        "UPDATE auth_config SET booking_events_url = ?, booking_events_secret = ? WHERE id = 'singleton'",
    )
    .bind(url)
    .bind(&secret)
    .execute(pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(n, 1, "auth_config singleton exists after migrate");
}

async fn insert_booking(
    pool: &SqlitePool,
    et_id: &str,
    status: &str,
    start: &str,
    end: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO bookings (time_version, id, event_type_id, uid, guest_name, guest_email, guest_timezone, start_at, end_at, status, cancel_token, reschedule_token)
         VALUES (1, ?, ?, ?, 'Sara Ali', 'sara@example.com', 'Asia/Riyadh', ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(et_id)
    .bind(format!("{}@calrs", id))
    .bind(start)
    .bind(end)
    .bind(status)
    .bind(format!("c-{}", id))
    .bind(format!("r-{}", id))
    .execute(pool)
    .await
    .unwrap();
    id
}

fn at(days: i64, hour: u32) -> String {
    (Utc::now().date_naive() + chrono::Duration::days(days))
        .and_hms_opt(hour, 0, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[tokio::test]
async fn a_booking_is_sent_created_then_rescheduled_then_cancelled_once_each() {
    let pool = setup_test_db().await;
    let (_user, _account, et_id) = seed_test_data(&pool).await;
    let (port, received) = spawn_receiver().await;
    configure(&pool, &format!("http://127.0.0.1:{}/hook", port)).await;

    let id = insert_booking(&pool, &et_id, "confirmed", &at(3, 9), &at(3, 10)).await;
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    {
        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1, "one event for a new confirmed booking");
        let b = body(&got[0]);
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["triggerEvent"], "BOOKING_CREATED");
        assert_eq!(v["payload"]["uid"], format!("{}@calrs", id));
        assert_eq!(v["payload"]["title"], "Test Meeting");
        assert_eq!(v["payload"]["attendees"][0]["email"], "sara@example.com");
        assert_eq!(v["payload"]["organizer"]["email"], "test@example.com");
        let sig = header(&got[0], "X-Cal-Signature-256").expect("signed");
        assert_eq!(
            sig,
            crate::web::booking_events::signature("s3cret", b.as_bytes())
        );
    }

    // Nothing changed: nothing more is sent.
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    assert_eq!(received.lock().unwrap().len(), 1);

    // Moved to another time: rescheduled, same uid.
    sqlx::query("UPDATE bookings SET start_at = ?, end_at = ? WHERE id = ?")
        .bind(at(4, 11))
        .bind(at(4, 12))
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    {
        let got = received.lock().unwrap();
        assert_eq!(got.len(), 2);
        let v: serde_json::Value = serde_json::from_str(&body(&got[1])).unwrap();
        assert_eq!(v["triggerEvent"], "BOOKING_RESCHEDULED");
        assert_eq!(v["payload"]["uid"], format!("{}@calrs", id));
        assert_eq!(v["payload"]["startTime"], at(4, 11));
    }

    // Cancelled: sent once.
    sqlx::query("UPDATE bookings SET status = 'cancelled' WHERE id = ?")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    let got = received.lock().unwrap();
    assert_eq!(got.len(), 3, "the cancellation goes out once");
    let v: serde_json::Value = serde_json::from_str(&body(&got[2])).unwrap();
    assert_eq!(v["triggerEvent"], "BOOKING_CANCELLED");
}

#[tokio::test]
async fn pending_requests_and_past_bookings_are_not_sent() {
    let pool = setup_test_db().await;
    let (_user, _account, et_id) = seed_test_data(&pool).await;
    let (port, received) = spawn_receiver().await;

    // A past booking, from before the feature was switched on.
    insert_booking(&pool, &et_id, "confirmed", &at(-3, 9), &at(-3, 10)).await;
    configure(&pool, &format!("http://127.0.0.1:{}/hook", port)).await;
    crate::web::booking_events::baseline_past(&pool).await;

    insert_booking(&pool, &et_id, "pending", &at(2, 9), &at(2, 10)).await;
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    assert!(received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_delivery_is_recorded_and_retried() {
    let pool = setup_test_db().await;
    let (_user, _account, et_id) = seed_test_data(&pool).await;
    // Nothing listens on this port.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    configure(&pool, &format!("http://127.0.0.1:{}/hook", closed)).await;
    let id = insert_booking(&pool, &et_id, "confirmed", &at(3, 9), &at(3, 10)).await;
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    let (_, err, waiting) = crate::web::booking_events::status(&pool).await;
    assert_eq!(waiting, 1);
    assert!(err.is_some());

    let (port, received) = spawn_receiver().await;
    configure(&pool, &format!("http://127.0.0.1:{}/hook", port)).await;
    crate::web::booking_events::deliver_due(&pool, &KEY).await;
    assert_eq!(
        received.lock().unwrap().len(),
        1,
        "retried on the next pass"
    );
    let (ok, _, waiting) = crate::web::booking_events::status(&pool).await;
    assert!(ok.is_some());
    assert_eq!(waiting, 0);
    let sent: (Option<String>,) =
        sqlx::query_as("SELECT sent_status FROM booking_event_deliveries WHERE booking_id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sent.0.as_deref(), Some("confirmed"));
}
