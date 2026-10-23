#![allow(clippy::unwrap_used)]

use postit_core::PlatformId;
use postit_data::platforms::PlatformRepo;
use sqlx::PgPool;

fn pid(s: &str) -> PlatformId {
    PlatformId::new(s).unwrap_or_else(|e| unreachable!("platform id: {e}"))
}

#[sqlx::test]
async fn upsert_inserts_and_refreshes_without_touching_enabled(pool: PgPool) {
    PlatformRepo::upsert_registered(&pool, &[(pid("tiktok"), "TikTok".to_owned())])
        .await
        .unwrap_or_else(|e| unreachable!("upsert: {e}"));
    let first = PlatformRepo::list(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(first.len(), 1);
    assert!(first[0].enabled);

    let matched = PlatformRepo::set_enabled(&pool, &pid("tiktok"), false)
        .await
        .unwrap_or_else(|e| unreachable!("set_enabled: {e}"));
    assert!(matched);

    PlatformRepo::upsert_registered(&pool, &[(pid("tiktok"), "TikTok Inc".to_owned())])
        .await
        .unwrap_or_else(|e| unreachable!("upsert 2: {e}"));
    let rows = PlatformRepo::list(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("list 2: {e}"));
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].enabled);
    assert_eq!(rows[0].display_name, "TikTok Inc");
    assert_eq!(rows[0].first_seen_at, first[0].first_seen_at);
}

#[sqlx::test]
async fn upsert_keeps_rows_not_in_registry(pool: PgPool) {
    PlatformRepo::upsert_registered(
        &pool,
        &[
            (pid("tiktok"), "TikTok".to_owned()),
            (pid("bluesky"), "Bluesky".to_owned()),
        ],
    )
    .await
    .unwrap_or_else(|e| unreachable!("upsert: {e}"));
    PlatformRepo::upsert_registered(&pool, &[(pid("tiktok"), "TikTok".to_owned())])
        .await
        .unwrap_or_else(|e| unreachable!("upsert 2: {e}"));
    let rows = PlatformRepo::list(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["bluesky", "tiktok"]);
}

#[sqlx::test]
async fn set_enabled_reports_no_match(pool: PgPool) {
    let matched = PlatformRepo::set_enabled(&pool, &pid("nothing"), true)
        .await
        .unwrap_or_else(|e| unreachable!("set_enabled: {e}"));
    assert!(!matched);
}

#[sqlx::test]
async fn check_rejects_invalid_ids(pool: PgPool) {
    for bad in ["Tik-Tok", "tik-tok"] {
        let result = sqlx::query("INSERT INTO platforms (id, display_name) VALUES ($1, 'x')")
            .bind(bad)
            .execute(&pool)
            .await;
        assert!(result.is_err(), "{bad} should be rejected");
    }
}

#[sqlx::test]
async fn get_reads_one_row_on_a_connection(pool: PgPool) {
    PlatformRepo::upsert_registered(&pool, &[(pid("tiktok"), "TikTok".to_owned())])
        .await
        .unwrap();
    PlatformRepo::set_enabled(&pool, &pid("tiktok"), false)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let found = PlatformRepo::get(&mut conn, &pid("tiktok")).await.unwrap();
    let missing = PlatformRepo::get(&mut conn, &pid("gone")).await.unwrap();

    assert!(found.is_some_and(|row| !row.enabled && row.display_name == "TikTok"));
    assert!(missing.is_none());
}
