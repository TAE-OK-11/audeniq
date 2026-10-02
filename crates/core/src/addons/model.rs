use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

pub use crate::states::AddonOrderStatus as Status;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateOrder {
    pub service_code: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub details: Details,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Details {
    ArtistProfile {
        request_type: String,
        platforms: Vec<String>,
        #[serde(default)]
        notes: String,
    },
    Migration {
        previous_distributor: String,
        preserve_upc: bool,
        original_release_date: NaiveDate,
        previous_release_urls: Vec<String>,
    },
    Priority {},
    Lyrics {
        #[serde(default)]
        lyrics_text: String,
        lrc_asset_id: Option<Uuid>,
        #[serde(default)]
        basic_video_requested: bool,
    },
    LyricVideo {
        lyrics_text: String,
        lrc_asset_id: Option<Uuid>,
        template_id: String,
        source_asset_id: Option<Uuid>,
    },
    Mv {
        review_evidence_asset_id: Option<Uuid>,
        release_id: Option<Uuid>,
    },
    Promo {
        #[serde(default = "yes")]
        presave_enabled: bool,
        #[serde(default = "yes")]
        preorder_enabled: bool,
    },
    MusicData {},
}
fn yes() -> bool {
    true
}

#[derive(Debug, FromRow)]
pub struct Order {
    pub id: Uuid,
    pub org_id: Uuid,
    pub service_code: String,
    pub status: String,
    pub payment_status: String,
    pub payment_reference: Option<String>,
    pub amount: i64,
    pub target_type: String,
    pub target_id: Uuid,
    pub release_id: Option<Uuid>,
    pub priority: i32,
    pub revision_count: i32,
    pub max_revisions_snapshot: Option<i32>,
    pub row_version: i64,
    pub dispatch_generation: i32,
}
impl Order {
    pub fn state(&self) -> crate::error::Result<Status> {
        serde_json::from_value(Value::String(self.status.clone()))
            .map_err(|_| crate::error::Error::Internal)
    }
    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "COMPLETED" | "REJECTED" | "CANCELLED")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub row_version: i64,
    pub reason: String,
    pub status: Option<Status>,
    pub payment_reference: Option<String>,
    pub refund_reference: Option<String>,
    pub assigned_admin_user_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revise {
    pub row_version: i64,
    pub reason: String,
    pub details: Option<Details>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDecision {
    pub row_version: i64,
    pub reason: String,
    pub approved: bool,
    pub valid_until: Option<DateTime<Utc>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    pub row_version: i64,
    pub reason: String,
    pub external_reference: String,
    pub lrc_asset_id: Option<Uuid>,
    pub output_asset_id: Option<Uuid>,
    pub qr_asset_id: Option<Uuid>,
    pub promo_card_asset_id: Option<Uuid>,
    pub evidence_asset_id: Option<Uuid>,
    pub migration_step: Option<String>,
    pub mv_distribution_status: Option<String>,
    pub dsp_links: Option<Vec<DspLink>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DspLink {
    pub platform: String,
    pub url: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Filters {
    pub service_code: Option<String>,
    pub paid: Option<bool>,
    pub status: Option<Status>,
    pub assigned_admin_user_id: Option<Uuid>,
    pub submitted_from: Option<DateTime<Utc>>,
    pub submitted_to: Option<DateTime<Utc>>,
    pub artist_id: Option<Uuid>,
    pub release_id: Option<Uuid>,
    pub priority: Option<bool>,
    pub needs_info: Option<bool>,
    pub external_pending: Option<bool>,
    pub failed: Option<bool>,
    pub unprocessed_hours: Option<i32>,
    pub before_created_at: Option<DateTime<Utc>>,
    pub before_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogUpdate {
    pub expected_version: i32,
    pub price_krw: i64,
    pub active: bool,
    pub display_name: String,
    pub description: String,
    pub validity_days: Option<i32>,
    pub max_revisions: Option<i32>,
    pub reason: String,
}
