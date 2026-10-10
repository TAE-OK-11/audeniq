//! Durable staff access logs and bounded deletion against disposable PostgreSQL.
mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use support::*;
use tower::ServiceExt;
use uuid::Uuid;

#[sqlx::test]
async fn staff_reads_are_recorded_with_server_route_and_source(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let u = user(&api).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(u.user).execute(&pool).await.unwrap();
    let response = api
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/staff/me")
                .header("x-audeniq-service", SECRET)
                .header("cookie", &u.cookie)
                .header(audeniq_core::auth::CLIENT_IP_HEADER, "2001:db8::12")
                .header("x-audeniq-audit-route", "forged-secret-token")
                .header("x-audeniq-audit-resource", Uuid::new_v4().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let row=sqlx::query("SELECT id,route,method,host(source_ip) AS source,resource_id,retain_until>=occurred_at+interval '2 years' AS retained FROM privacy.staff_access_logs WHERE actor_user_id=$1")
        .bind(u.user).fetch_one(&pool).await.unwrap();
    assert_eq!(row.get::<String, _>("route"), "/api/staff/me");
    assert_eq!(row.get::<String, _>("method"), "GET");
    assert_eq!(row.get::<String, _>("source"), "2001:db8::12");
    assert!(row.get::<Option<Uuid>, _>("resource_id").is_none());
    assert!(row.get::<bool, _>("retained"));
    let id: Uuid = row.get("id");
    assert!(
        sqlx::query("UPDATE privacy.staff_access_logs SET route='tampered' WHERE id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM privacy.staff_access_logs WHERE id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("TRUNCATE privacy.staff_access_logs")
            .execute(&pool)
            .await
            .is_err()
    );
    // Failure to record access must stop a staff data response.
    sqlx::query("DROP TABLE privacy.staff_access_logs")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&api, "GET", "/api/staff/me", Value::Null, Some(&u))
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[sqlx::test]
async fn purge_preserves_live_sessions_and_runtime_cannot_mutate_logs(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let u = user(&api).await;
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.sessions(token_hash,user_id,csrf_hash,expires_at) VALUES($1,$2,$3,now()-interval '2 days')")
        .bind(vec![1u8;32]).bind(u.user).bind(vec![2u8;32]).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO identity.auth_limits(bucket_hash,window_start,attempts) VALUES($1,now()-interval '2 days',1),($2,now(),1)")
        .bind(vec![3u8;32]).bind(vec![4u8;32]).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO privacy.staff_access_logs(id,actor_user_id,request_id,route,method,occurred_at,retain_until) VALUES($1,$2,$3,'/api/staff/me','GET',now()-interval '3 years',now()-interval '1 year')")
        .bind(Uuid::new_v4()).bind(u.user).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let result = audeniq_core::privacy_maintenance::purge_transient(&pool, "test-operator")
        .await
        .unwrap();
    assert_eq!(result["sessions"], 1);
    assert_eq!(result["auth_limits"], 1);
    assert_eq!(result["expired_access_logs"], 1);
    assert_eq!(
        call(&api, "GET", "/api/me", Value::Null, Some(&u)).await.0,
        StatusCode::OK
    );
    let rights:Value=sqlx::query_scalar("SELECT jsonb_build_object('insert',has_table_privilege('audeniq_api','privacy.staff_access_logs','INSERT'),'update',has_table_privilege('audeniq_api','privacy.staff_access_logs','UPDATE'),'delete',has_table_privilege('audeniq_api','privacy.staff_access_logs','DELETE'),'worker_read',has_table_privilege('audeniq_worker','privacy.staff_access_logs','SELECT'),'temp',has_database_privilege('audeniq_api',current_database(),'TEMP'))")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        rights,
        json!({"insert":true,"update":false,"delete":false,"worker_read":false,"temp":false})
    );
}
