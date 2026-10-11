//! Virtual royalty report tests: generate a synthetic DSP report,
//! ingest it, verify duplicate rejection, match lines against the catalog.

use audeniq_core::royalty;
use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

fn csv_with(isrc: &str) -> String {
    format!(
        "isrc,dsp_track_id,quantity,gross_amount,currency\n\
         {isrc},DSP001,1500,45.50,USD\n\
         {isrc},DSP002,800,24.00,USD\n\
         XXYYY9900001,DSP999,100,3.00,USD\n"
    )
}

#[sqlx::test(migrations = "../../migrations")]
async fn royalty_01_ingest_parse_match(pool: PgPool) {
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'Test','LABEL')")
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();

    // Seed a catalog track with a known ISRC.
    let release = Uuid::new_v4();
    let track = Uuid::new_v4();
    let artist = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'release'),($1,$3,'artist')",
    )
    .bind(org)
    .bind(release)
    .bind(artist)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','T')",
    )
    .bind(artist)
    .bind(org)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO catalog.artists(id, org_id, name) VALUES($1,$2,'Test Artist')")
        .bind(artist)
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.releases(id, org_id, title, release_type, status) VALUES($1,$2,'T','SINGLE','DRAFT')")
        .bind(release)
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.tracks(id, org_id, release_id, title, disc_number, track_number, artist_id, isrc) VALUES($1,$2,$3,'T',1,1,$4,'USABC2600001')")
        .bind(track)
        .bind(org)
        .bind(release)
        .bind(artist)
        .execute(&pool)
        .await
        .unwrap();

    let csv = csv_with("USABC2600001");
    let period_start = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();

    // Ingest.
    let report_id = royalty::ingest_report(
        &pool,
        org,
        "mockdsp",
        period_start,
        period_end,
        "USD",
        "mockdsp_2026-08.csv",
        &csv,
    )
    .await
    .unwrap();

    // 3 lines stored.
    let line_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM finance.report_lines WHERE report_id=$1")
            .bind(report_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(line_count, 3);

    // Duplicate ingest rejected.
    let dup = royalty::ingest_report(
        &pool,
        org,
        "mockdsp",
        period_start,
        period_end,
        "USD",
        "mockdsp_2026-08.csv",
        &csv,
    )
    .await;
    assert!(matches!(dup, Err(audeniq_core::error::Error::Conflict)));

    // Match: 2 lines hit the catalog ISRC, 1 stays unmatched.
    let (auto, unmatched) = royalty::match_report(&pool, org, report_id).await.unwrap();
    assert_eq!(auto, 2);
    assert_eq!(unmatched, 1);

    let matched_release: Uuid = sqlx::query_scalar(
        "SELECT matched_release_id FROM finance.report_lines WHERE report_id=$1 AND match_status='AUTO' LIMIT 1",
    )
    .bind(report_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(matched_release, release);

    let report_status: String =
        sqlx::query_scalar("SELECT status FROM finance.royalty_reports WHERE id=$1")
            .bind(report_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(report_status, "MATCHED");
}

#[sqlx::test(migrations = "../../migrations")]
async fn royalty_02_rejects_bad_input(pool: PgPool) {
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'Test','LABEL')")
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    let ps = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
    let pe = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();

    // Empty content.
    assert!(
        royalty::ingest_report(&pool, org, "mockdsp", ps, pe, "USD", "e.csv", "")
            .await
            .is_err()
    );
    // Missing column.
    assert!(
        royalty::ingest_report(
            &pool,
            org,
            "mockdsp",
            ps,
            pe,
            "USD",
            "e.csv",
            "isrc,quantity\nUSX,10\n"
        )
        .await
        .is_err()
    );
    // Negative amount.
    assert!(
        royalty::ingest_report(
            &pool,
            org,
            "mockdsp",
            ps,
            pe,
            "USD",
            "e.csv",
            "isrc,dsp_track_id,quantity,gross_amount,currency\nUSX,D1,10,-5.00,USD\n"
        )
        .await
        .is_err()
    );
    // Bad period.
    assert!(
        royalty::ingest_report(
            &pool,
            org,
            "mockdsp",
            pe,
            ps,
            "USD",
            "e.csv",
            &csv_with("USX")
        )
        .await
        .is_err()
    );
    // Bad currency.
    assert!(
        royalty::ingest_report(
            &pool,
            org,
            "mockdsp",
            ps,
            pe,
            "usd",
            "e.csv",
            &csv_with("USX")
        )
        .await
        .is_err()
    );
}

/// The same recording on a single and an album: ISRC alone is ambiguous
/// (splits are per release), so a report line names its product by UPC.
#[sqlx::test(migrations = "../../migrations")]
async fn royalty_03_shared_isrc_matches_by_upc(pool: PgPool) {
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'Test','LABEL')")
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    let artist = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'artist')")
        .bind(org)
        .bind(artist)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','T')",
    )
    .bind(artist)
    .bind(org)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO catalog.artists(id, org_id, name) VALUES($1,$2,'Test Artist')")
        .bind(artist)
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    let mut releases = Vec::new();
    for (kind, upc) in [("SINGLE", "042100005264"), ("ALBUM", "123456789012")] {
        let release = Uuid::new_v4();
        sqlx::query("INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'release')")
            .bind(org)
            .bind(release)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.releases(id, org_id, title, release_type, status, upc) VALUES($1,$2,'T',$3,'DRAFT',$4)")
            .bind(release)
            .bind(org)
            .bind(kind)
            .bind(upc)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.tracks(id, org_id, release_id, title, disc_number, track_number, artist_id, isrc) VALUES($1,$2,$3,'T',1,1,$4,'USABC2600001')")
            .bind(Uuid::new_v4())
            .bind(org)
            .bind(release)
            .bind(artist)
            .execute(&pool)
            .await
            .unwrap();
        releases.push(release);
    }
    // EAN-13 spelling of the single's UPC, a hyphenated lowercase ISRC for
    // the album, and one line with no product (stays for review).
    let csv = "isrc,upc,dsp_track_id,quantity,gross_amount,currency\n\
               USABC2600001,0042100005264,D1,10,1.00,USD\n\
               us-abc-26-00001,123456789012,D2,20,2.00,USD\n\
               USABC2600001,,D3,30,3.00,USD\n";
    let report_id = royalty::ingest_report(
        &pool,
        org,
        "mockdsp",
        NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(),
        "USD",
        "shared.csv",
        csv,
    )
    .await
    .unwrap();
    let (auto, unmatched) = royalty::match_report(&pool, org, report_id).await.unwrap();
    assert_eq!((auto, unmatched), (2, 1));
    let matched: Vec<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT dsp_track_id, matched_release_id FROM finance.report_lines WHERE report_id=$1 ORDER BY line_no",
    )
    .bind(report_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(matched[0], ("D1".into(), Some(releases[0])));
    assert_eq!(matched[1], ("D2".into(), Some(releases[1])));
    assert_eq!(matched[2], ("D3".into(), None));
}
