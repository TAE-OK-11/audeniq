//! Artist-facing MCP tools. Each handler validates its arguments against the
//! published input schema, then calls the existing services with the token's
//! [`Actor`] and workspace: authorization, state transitions, QC and the
//! submission rules are exactly the Studio's. Nothing here writes the
//! database directly.
//!
//! Files go through [`super::files`]: the bytes go straight to storage (or
//! are fetched from a public link) and take the Studio's upload completion.
//!
//! What is deliberately not exposed: signing the application or agreement,
//! withdrawals, takedowns, payouts, payout accounts, members and contracts.
use super::{
    SCOPE_PAYOUT, SCOPE_READ, SCOPE_SETTLEMENT, SCOPE_SUBMIT, SCOPE_WRITE, oauth::Principal,
    protocol::error_code,
};
use crate::{
    api::AppState,
    auth::Actor,
    catalog::{self, Kind},
    drafts,
    error::{Error, Result},
    operations,
    submission::{self, DeclarationsInput},
};
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashMap, sync::OnceLock};
use uuid::Uuid;

pub const WIDGET_URI: &str = "ui://audeniq/release-card.html";
const WIDGET_MIME: &str = "text/html;profile=mcp-app";
const WIDGET_HTML: &str = include_str!("widget.html");
/// Studio credit roles (the wizard's composer/lyricist/arranger/producer
/// fields), so a draft edited here reopens unchanged in the Studio.
const CREDIT_ROLES: [(&str, &str, &str); 4] = [
    ("composers", "Composer", "composers"),
    ("lyricists", "Lyricist", "lyricists"),
    ("arrangers", "Arranger", "arrangers"),
    ("producers", "Producer", "producer"),
];
/// Longest a tool call may run (the edge gives up at about 100 s).
const TOOL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);
/// A preview's confirmation is valid this long.
const CONFIRM_SECONDS: i64 = 15 * 60;

pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub scope: &'static str,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    widget: bool,
    schema: fn() -> Value,
}

/// Platforms an artist page can be registered for (artist_pages::PLATFORMS).
const PLATFORM_LINK_PROPS: [&str; 8] = [
    "spotify", "apple", "melon", "genie", "flo", "bugs", "vibe", "youtube",
];

/// Public information anyone may read without connecting an account, under
/// daily limits ([`super::public`]).
const PUBLIC_TOOLS: [&str; 4] = [
    "get_release_requirements",
    "list_announcements",
    "get_announcement",
    "get_service_status",
];

impl ToolDef {
    pub fn public(&self) -> bool {
        PUBLIC_TOOLS.contains(&self.name)
    }
    fn retry_safe(&self) -> bool {
        RETRY_SAFE_TOOLS.contains(&self.name)
    }
    /// The published input schema (retry-safe creates also take
    /// `client_request_id`).
    pub fn input_schema(&self) -> Value {
        let mut s = (self.schema)();
        if self.retry_safe() {
            s["properties"]["client_request_id"] = json!({"type":"string","minLength":8,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$",
                "description":"Optional: a random id for this request. Sending the same id again (e.g. a retry after a lost response) returns the first result instead of creating a duplicate."});
        }
        s
    }
}

/// Tools that create something and accept `client_request_id` ([`super::safety`]).
const RETRY_SAFE_TOOLS: [&str; 3] = [
    "create_release_draft",
    "create_inquiry",
    "create_platform_request",
];

fn defs() -> &'static [ToolDef] {
    static DEFS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        vec![
            ToolDef {
                name: "get_account_context",
                title: "AUDENIQ account",
                description: "The connected AUDENIQ workspace (name, the artist's role), the permissions this connection has, and whether it can submit releases. Call first.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_distribution_platforms",
                title: "Distribution platforms",
                description: "Streaming/download platforms AUDENIQ distributes to, whether each can be selected for this workspace right now, and each platform's delivery requirements (artwork size, audio format, lead time, required credits). Use the slug values in a draft's platforms.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "get_release_requirements",
                title: "Release requirements",
                description: "What an AUDENIQ distribution application needs: required metadata fields and formats, release types and track counts, accepted audio/artwork files and size limits, credit roles and the declarations made at submission.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_announcements",
                title: "AUDENIQ notices and events",
                description: "AUDENIQ's published notices (service news, policy changes, holiday schedules) or events, newest first, with a short excerpt and a link. Works without connecting an account (a few calls a day); call get_announcement for the full text.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "kind": {"type":"string","enum":["notices","events"],"default":"notices"},
                    "limit": {"type":"integer","minimum":1,"maximum":50,"default":20}}}),
            },
            ToolDef {
                name: "get_announcement",
                title: "AUDENIQ notice or event",
                description: "The full text of one published AUDENIQ notice or event (id from list_announcements). Works without connecting an account.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["kind","id"],"properties":{
                    "kind": {"type":"string","enum":["notices","events"]},
                    "id": {"type":"string","pattern":"^[a-z0-9-]{1,64}$"}}}),
            },
            ToolDef {
                name: "get_service_status",
                title: "AUDENIQ service status",
                description: "Whether AUDENIQ is under maintenance now or has maintenance scheduled, with times and the message to show. Works without connecting an account.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_releases",
                title: "List releases",
                description: "The workspace's releases (drafts and submitted), newest page first, with status. Use next_cursor to page.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
                    "cursor":{"type":"string","format":"uuid","description":"next_cursor from the previous page"}}}),
            },
            ToolDef {
                name: "get_release",
                title: "Release details",
                description: "One release: metadata, tracks with credits and attached audio, status and per-platform delivery state.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: true,
                schema: release_id_schema,
            },
            ToolDef {
                name: "create_release_draft",
                title: "Create release draft",
                description: "Create a new release draft in AUDENIQ. Only title, release_type and artist_name are required; add the rest now or later with update_release_draft. Creates a new draft on every call: check list_releases first to avoid duplicates.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: true,
                schema: || {
                    let mut s = draft_fields_schema();
                    s["properties"]["title"] = title_schema();
                    s["properties"]["release_type"] = release_type_schema();
                    s["required"] = json!(["title", "release_type", "artist_name"]);
                    s
                },
            },
            ToolDef {
                name: "update_release_draft",
                title: "Update release draft",
                description: "Change fields of a draft (or of a release returned for correction). Only the fields given change. Submitted releases under review cannot be edited.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: true,
                schema: || {
                    let mut s = draft_fields_schema();
                    s["properties"]["release_id"] = uuid_schema("The release to change");
                    s["properties"]["artwork_asset_id"] = uuid_schema("Cover art: an IMAGE asset from start_upload/complete_upload or import_file_from_url (state READY)");
                    s["properties"]["title"] = title_schema();
                    s["properties"]["release_type"] = release_type_schema();
                    s["required"] = json!(["release_id"]);
                    s
                },
            },
            ToolDef {
                name: "upsert_track",
                title: "Add or edit a track",
                description: "Add a track to a draft, or change one (pass track_id). The track's artist is the release's artist_name. Audio is attached with asset_id: a file sent with start_upload/complete_upload or import_file_from_url, or uploaded in AUDENIQ Studio. Omitted fields keep their current value when editing.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","title","track_number"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "track_id": uuid_schema("Existing track to edit; omit to add a track"),
                    "title": {"type":"string","minLength":1,"maxLength":200,"description":"Track title without version text, 'feat.' or explicit markers"},
                    "track_number": {"type":"integer","minimum":1,"maximum":99},
                    "disc_number": {"type":"integer","minimum":1,"maximum":9,"default":1},
                    "version": {"type":"string","maxLength":100,"description":"Version such as 'Radio Edit' or '2024 Remaster'; empty for none"},
                    "isrc": {"type":"string","pattern":"^$|^[A-Za-z]{2}-?[A-Za-z0-9]{3}-?[0-9]{2}-?[0-9]{5}$","description":"Existing ISRC if the recording has one; AUDENIQ issues one otherwise"},
                    "explicit": {"type":"boolean","description":"Explicit lyrics/content (parental advisory)"},
                    "lyrics": {"type":"string","maxLength":20000},
                    "asset_id": uuid_schema("Registered audio file (start_upload/complete_upload, import_file_from_url or AUDENIQ Studio)")}}),
            },
            ToolDef {
                name: "set_track_credits",
                title: "Set track credits",
                description: "Replace a track's songwriting/production credits with the given names (real names as the artist states them). At least one composer or lyricist is required for submission.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || {
                    let names = json!({"type":"array","maxItems":20,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":120,"pattern":"^[^,]+$"}});
                    json!({"type":"object","additionalProperties":false,"required":["release_id","track_id"],"properties":{
                        "release_id": uuid_schema("The release"),
                        "track_id": uuid_schema("The track"),
                        "composers": names, "lyricists": names, "arrangers": names, "producers": names}})
                },
            },
            ToolDef {
                name: "validate_release",
                title: "Check release",
                description: "Run AUDENIQ's pre-submission checks on a draft: missing or invalid fields (field-level), missing audio/artwork, audio QC results, platform availability and account gates. Returns what blocks submission and what will go to human review.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: true,
                schema: release_id_schema,
            },
            ToolDef {
                name: "request_permission",
                title: "Ask for more permissions",
                description: "Ask the artist to give this connection more permissions (releases:write, releases:submit, settlement:read, payouts:request) instead of reconnecting. Returns an approve_url: show it to the artist, who approves in AUDENIQ Studio within 60 minutes. Approved permissions stay until the connection is disconnected and apply to the tokens you already hold. Only ask for what the current task needs and say why in reason.",
                scope: SCOPE_READ, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["scopes","reason"],"properties":{
                    "scopes": {"type":"array","minItems":1,"maxItems":5,"uniqueItems":true,"items":{"type":"string","enum":["releases:read","releases:write","releases:submit","settlement:read","payouts:request"]}},
                    "reason": {"type":"string","minLength":1,"maxLength":500,"description":"Shown to the artist: what you need it for"}}}),
            },
            ToolDef {
                name: "get_permission_request",
                title: "Permission request status",
                description: "Whether the artist approved, rejected or let expire a request from request_permission, and the permissions this connection has now.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["request_id"],"properties":{
                    "request_id": uuid_schema("From request_permission")}}),
            },
            ToolDef {
                name: "preview_submission",
                title: "Preview application",
                description: "The complete distribution application exactly as it would be submitted, with the declarations the artist made, and the checks result. When everything passes and this connection may submit, it returns a confirmation_token for submit_release, valid 15 minutes and only for this exact content. Show the whole preview to the artist before asking them to confirm.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: true,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","declarations"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "declarations": declarations_schema()}}),
            },
            ToolDef {
                name: "submit_release",
                title: "Submit application",
                description: "Submit the previewed distribution application to AUDENIQ review. Only after the artist explicitly confirmed the preview in this conversation. Requires the confirmation_token from preview_submission and the same declarations. Fails if the release changed since the preview. Safe to retry with the same token (no duplicate application).",
                scope: SCOPE_SUBMIT, read_only: false, destructive: true, idempotent: true, widget: true,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","confirmation_token","declarations","artist_confirmed"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "confirmation_token": {"type":"string","minLength":20,"maxLength":4096},
                    "declarations": declarations_schema(),
                    "artist_confirmed": {"type":"boolean","const":true,"description":"The artist explicitly confirmed this exact preview and these declarations in the conversation"}}}),
            },
            ToolDef {
                name: "get_submission_status",
                title: "Application status",
                description: "Where a submitted application stands: review stage, check results, what the artist must correct, staff review notes, agreement signature and per-platform delivery/live state.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: true,
                schema: release_id_schema,
            },
            ToolDef {
                name: "get_release_corrections",
                title: "Released metadata corrections",
                description: "For a release already sent to platforms: the current genre and per-track credits (with approved corrections applied), the latest correction request with AUDENIQ's review decision and note, and whether a new request is possible. Title, artist names, audio, identifiers, release date and rights lines cannot be corrected (they need a new release).",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "request_release_correction",
                title: "Request a metadata correction",
                description: "Ask AUDENIQ to correct a delivered release: the genre and/or the full credit list of given tracks (composer, lyricist, arranger, producer, engineer...; artist and featured-artist credits are locked). AUDENIQ staff review it; once approved the update goes to every platform carrying the release. One open request per release. Confirm the exact changes with the artist first.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","reason"],"properties":{
                    "release_id": uuid_schema("The delivered release"),
                    "reason": {"type":"string","minLength":1,"maxLength":1000,"description":"Why the change is needed (shown to the reviewer)"},
                    "genre": {"type":"string","minLength":1,"maxLength":60},
                    "tracks": {"type":"array","maxItems":99,"items":{"type":"object","additionalProperties":false,"required":["track_id","credits"],"properties":{
                        "track_id": uuid_schema("Track of this release"),
                        "credits": {"type":"array","maxItems":50,"description":"The track's complete new credit list (replaces all non-artist credits)",
                            "items":{"type":"object","additionalProperties":false,"required":["name","role"],"properties":{
                                "name":{"type":"string","minLength":1,"maxLength":200},
                                "role":{"type":"string","minLength":1,"maxLength":80,"description":"Composer, Lyricist, Arranger, Producer, Engineer, ..."}}}}}}}}}),
            },
            ToolDef {
                name: "cancel_release_correction",
                title: "Cancel a correction request",
                description: "Withdraw the release's pending correction request before AUDENIQ reviews it.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "get_playlist_pitch",
                title: "Playlist pitch",
                description: "The release's playlist pitch to platform editors: whether one can be sent (submitted releases, at least 7 days before the release date), the latest pitch and its outcome from AUDENIQ.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "submit_playlist_pitch",
                title: "Submit a playlist pitch",
                description: "Pitch one focus track of a submitted release to platform editors (AUDENIQ forwards it). At least 7 days before the release date; one active pitch per release. Draft the texts with the artist and send only what they approved; describe the music truthfully, never invent achievements or numbers.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","track_id","description"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "track_id": uuid_schema("Focus track of this release"),
                    "description": {"type":"string","minLength":20,"maxLength":1000,"description":"What the track is: story, sound, context"},
                    "genres": {"type":"string","maxLength":200},
                    "moods": {"type":"string","maxLength":200},
                    "instruments": {"type":"string","maxLength":200},
                    "similar_artists": {"type":"string","maxLength":200},
                    "marketing": {"type":"string","maxLength":1000,"description":"Promotion plans (videos, shows, campaigns)"}}}),
            },
            ToolDef {
                name: "withdraw_playlist_pitch",
                title: "Withdraw a playlist pitch",
                description: "Withdraw the release's pitch while it is submitted or in review (a new one can then be sent).",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "get_smart_link",
                title: "Smart link and fan alerts",
                description: "The release's public smart link (landing page with every platform link): its URL, platform links (pasted or filled from the platforms' own ids) with their checks, views and clicks over the last 30 days, and how many fans signed up for a release alert.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "save_smart_link",
                title: "Save the smart link",
                description: "Create or update the release's smart link: turn it on/off and set platform links (https links on that platform's own domain; links filled automatically from the platforms need not be given). The page is public.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || {
                    let platforms: Vec<&str> = crate::smart_links::PLATFORMS.iter().map(|(p, _)| *p).collect();
                    json!({"type":"object","additionalProperties":false,"required":["release_id"],"properties":{
                        "release_id": uuid_schema("The release"),
                        "enabled": {"type":"boolean","default":true},
                        "links": {"type":"object","maxProperties":platforms.len(),"propertyNames":{"enum":platforms},
                            "additionalProperties":{"type":"string","maxLength":500,"pattern":"^$|^https://"},
                            "description":"Platform → link; an empty string removes it"}}})
                },
            },
            ToolDef {
                name: "list_release_codes",
                title: "UPC and ISRC codes",
                description: "Every submitted release with its UPC and each track's ISRC.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_notifications",
                title: "Notifications",
                description: "The latest AUDENIQ notifications for this workspace (review results, delivery, settlement, documents) and how many are unread.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_documents",
                title: "Agreements and rights documents",
                description: "The workspace's distribution agreements and rights documents with their status and AUDENIQ's review note, and the Studio link where each is read and signed (signing is only possible in Studio).",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_inquiries",
                title: "Support inquiries",
                description: "The workspace's inquiries to AUDENIQ support with their status.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "get_inquiry",
                title: "Inquiry conversation",
                description: "One inquiry with all messages between the artist and AUDENIQ support.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["inquiry_id"],"properties":{"inquiry_id": uuid_schema("The inquiry")}}),
            },
            ToolDef {
                name: "create_inquiry",
                title: "Ask AUDENIQ support",
                description: "Send a new inquiry to AUDENIQ support, optionally about one release. Show the artist the subject and message and send only after they agree; it is sent in their name.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["category","subject","body"],"properties":{
                    "category": {"type":"string","enum":["RELEASE","SETTLEMENT","CONTRACT","ACCOUNT","IDENTITY","OTHER"],"description":"IDENTITY: identity or real-name verification problems (AUDENIQ support can verify with an ID at the customer center; never ask for ID photos or numbers in the message)"},
                    "release_id": uuid_schema("Release the inquiry is about (optional)"),
                    "subject": {"type":"string","minLength":1,"maxLength":200},
                    "body": {"type":"string","minLength":1,"maxLength":4000}}}),
            },
            ToolDef {
                name: "reply_inquiry",
                title: "Reply to an inquiry",
                description: "Add the artist's message to an open inquiry. Send only text the artist agreed to.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["inquiry_id","body"],"properties":{
                    "inquiry_id": uuid_schema("The inquiry"),
                    "body": {"type":"string","minLength":1,"maxLength":4000}}}),
            },
            ToolDef {
                name: "get_settlement_summary",
                title: "Settlement balance",
                description: "The workspace's royalty balance in KRW: payable, pending payouts, held, available to withdraw, whether a payout account is registered (bank and last 4 digits only), the payout fee policy, and how much can still be requested through the assistant in the next 24 hours.",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_settlement_statements",
                title: "Settlement statement",
                description: "Royalty ledger lines (earnings, payouts, adjustments), newest first. Use next_before to page.",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "limit":{"type":"integer","minimum":1,"maximum":100,"default":30},
                    "before":{"type":"string","maxLength":120,"description":"next_before from the previous page"}}}),
            },
            ToolDef {
                name: "get_royalty_reports",
                title: "Royalty reports",
                description: "Streams and revenue reported by platforms, by month, platform and release, for a period of whole months (default: the last 12).",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "from":{"type":"string","pattern":"^[0-9]{4}-(0[1-9]|1[0-2])$","description":"First month YYYY-MM"},
                    "to":{"type":"string","pattern":"^[0-9]{4}-(0[1-9]|1[0-2])$","description":"Last month YYYY-MM"}}}),
            },
            ToolDef {
                name: "list_payouts",
                title: "Payout requests",
                description: "The workspace's payout requests with amount, fee, net, status and whether each can still be cancelled.",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "preview_payout",
                title: "Preview a payout",
                description: "Quote a payout to the registered account: fee, net amount, available balance and the assistant limit. Larger amounts must be requested in AUDENIQ Studio. When the payout is allowed, returns a confirmation_token for request_payout (10 minutes, this amount and account only). Only workspace owners can request payouts.",
                scope: SCOPE_PAYOUT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["amount"],"properties":{
                    "amount":{"type":"integer","minimum":1,"maximum":1000000000,"description":"Requested amount in KRW (before the fee)"}}}),
            },
            ToolDef {
                name: "request_payout",
                title: "Request a payout",
                description: "Request the previewed payout to the registered account. Only after the artist explicitly confirmed the amount, fee, net amount and account in this conversation. Requires the confirmation_token from preview_payout. Safe to retry with the same token.",
                scope: SCOPE_PAYOUT, read_only: false, destructive: true, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["amount","confirmation_token","artist_confirmed"],"properties":{
                    "amount":{"type":"integer","minimum":1,"maximum":1000000000},
                    "confirmation_token":{"type":"string","minLength":20,"maxLength":4096},
                    "artist_confirmed":{"type":"boolean","const":true,"description":"The artist explicitly confirmed this payout in the conversation"}}}),
            },
            ToolDef {
                name: "get_upload_link",
                title: "Upload files in AUDENIQ Studio",
                description: "Which of this release's files are still missing, and the AUDENIQ Studio page where the artist can upload them. Files can also be sent with start_upload or import_file_from_url.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "start_upload",
                title: "Start a file upload",
                description: "Get a signed URL to upload one audio master or cover image directly (for agents that can send HTTP requests with a file body). PUT the bytes to the returned url with exactly its headers within 10 minutes, then call complete_upload. Audio: WAV, FLAC, AIFF, ALAC (.m4a), WavPack or TTA up to 512 MiB; cover: square JPG or PNG up to 20 MiB (see get_release_requirements). When you only have a link to the file, use import_file_from_url instead.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["kind","size_bytes","content_type"],"properties":{
                    "kind": {"type":"string","enum":["AUDIO","IMAGE","DOCUMENT"],"description":"AUDIO for a track master, IMAGE for cover art, DOCUMENT for a rights proof (PDF, JPG or PNG; attach with attach_rights_proof)"},
                    "size_bytes": {"type":"integer","minimum":1,"maximum":536870912,"description":"Exact file size in bytes"},
                    "content_type": {"type":"string","enum":["audio/wav","audio/x-wav","audio/flac","audio/mp4","audio/x-m4a","audio/aiff","audio/x-aiff","audio/wavpack","audio/x-wavpack","audio/tta","audio/x-tta","image/jpeg","image/png","application/pdf"]}}}),
            },
            ToolDef {
                name: "complete_upload",
                title: "Finish a file upload",
                description: "Register a file uploaded to a start_upload URL. AUDENIQ checks the bytes (real format, malware scan, lossless audio). Returns state READY with the asset_id to attach, or PROCESSING for a long master: then poll get_upload_status instead of uploading again.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["upload_session_id","asset_id","expected_key"],"properties":{
                    "upload_session_id": uuid_schema("From start_upload"),
                    "asset_id": uuid_schema("From start_upload"),
                    "expected_key": {"type":"string","minLength":1,"maxLength":300,"description":"From start_upload"}}}),
            },
            ToolDef {
                name: "import_file_from_url",
                title: "Import a file from a link",
                description: "Fetch an audio master or cover image from a public https:// download link the artist gave you (cloud drive share link set to anyone with the link, file host) and register it like an upload. Use only links the artist provided. The link must lead straight to the file and report its size. Returns state READY with the asset_id to attach, or PROCESSING: then poll get_upload_status.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["kind","url"],"properties":{
                    "kind": {"type":"string","enum":["AUDIO","IMAGE","DOCUMENT"],"description":"AUDIO for a track master, IMAGE for cover art, DOCUMENT for a rights proof (PDF, JPG or PNG; attach with attach_rights_proof)"},
                    "url": {"type":"string","maxLength":2048,"pattern":"^https://","description":"Public direct download link"},
                    "content_type": {"type":"string","enum":["audio/wav","audio/flac","audio/mp4","audio/aiff","audio/wavpack","audio/tta","image/jpeg","image/png","application/pdf"],"description":"File type when the link does not show it"}}}),
            },
            ToolDef {
                name: "get_upload_status",
                title: "File upload status",
                description: "How an upload or import is going: READY (asset_id can be attached), PROCESSING, WAITING_FOR_FILE, EXPIRED or FAILED with an error_code.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["upload_session_id"],"properties":{
                    "upload_session_id": uuid_schema("From start_upload or import_file_from_url")}}),
            },
            ToolDef {
                name: "mark_notifications_read",
                title: "Mark notifications read",
                description: "Mark notifications as read: the given notification_ids (from list_notifications) or all of them.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "notification_ids": {"type":"array","maxItems":100,"uniqueItems":true,"items": uuid_schema("Notification")},
                    "all": {"type":"boolean","description":"Mark every notification read"}}}),
            },
            ToolDef {
                name: "close_inquiry",
                title: "Close an inquiry",
                description: "Close a support inquiry the artist says is resolved. Only when the artist asks to close it.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["inquiry_id"],"properties":{
                    "inquiry_id": uuid_schema("The inquiry (from list_inquiries)")}}),
            },
            ToolDef {
                name: "attach_rights_proof",
                title: "Attach a rights proof",
                description: "Attach a rights proof file (licence, consent letter; a DOCUMENT or IMAGE asset from start_upload/complete_upload or import_file_from_url) to a rights document waiting for one (document_id from list_documents), or add a new proof to a release (release_id and title instead of document_id). AUDENIQ then reviews it. Use only files the artist provided.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["asset_id","file_name"],"properties":{
                    "document_id": uuid_schema("The rights document waiting for a file (from list_documents)"),
                    "release_id": uuid_schema("New proof: the release it belongs to"),
                    "title": {"type":"string","minLength":1,"maxLength":200,"description":"New proof: what it is, e.g. 'Sample licence from X'"},
                    "asset_id": uuid_schema("The uploaded proof file"),
                    "file_name": {"type":"string","minLength":1,"maxLength":200,"description":"File name shown to reviewers"}}}),
            },
            ToolDef {
                name: "get_file_checks",
                title: "File check results",
                description: "Detailed check results of one uploaded audio or image file (format, sample rate, bit depth, loudness, clipping, image size and so on).",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["asset_id"],"properties":{
                    "asset_id": uuid_schema("The file (asset_id from get_release or an upload)")}}),
            },
            ToolDef {
                name: "get_delivery_status",
                title: "Delivery by platform",
                description: "Where a submitted release stands at each platform: sent, accepted, live or failed, with dates.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "get_content_id_status",
                title: "Content ID status",
                description: "YouTube Content ID state of a release: whether it is registered and any claims or requests in progress.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "check_smart_link",
                title: "Check smart link links",
                description: "Check platform links for a release's smart link without saving them: one verdict per platform (invalid host, other release, matches).",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","links"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "links": {"type":"object","minProperties":1,"maxProperties":20,"additionalProperties":{"type":"string","maxLength":500,"pattern":"^https://"},"description":"Platform key to link, as in save_smart_link"}}}),
            },
            ToolDef {
                name: "list_signing_requests",
                title: "Signing requests",
                description: "Electronic signing requests sent for this workspace's releases (rights holder, signer, status, expiry). Signing itself happens only in the signer's own link.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "release_id": uuid_schema("Only this release's requests")}}),
            },
            ToolDef {
                name: "list_members",
                title: "Workspace members",
                description: "Members of this workspace with role and status. E-mails are masked unless the artist owns the workspace.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "list_infringement_claims",
                title: "Infringement claims",
                description: "Rights claims filed against a release, their status, AUDENIQ's note and whether the artist can still answer (counter deadline).",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "resend_split_invite",
                title: "Resend a split invite",
                description: "E-mail a collaborator's revenue split invite again (share_id from get_release_splits). Limited to a few resends.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","share_id"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "share_id": uuid_schema("The collaborator's share (from get_release_splits)")}}),
            },
            ToolDef {
                name: "export_csv",
                title: "Export CSV",
                description: "Settlement statements or royalty reports as CSV text (up to 256 KB; larger exports are downloaded in AUDENIQ Studio).",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["kind"],"properties":{
                    "kind": {"type":"string","enum":["statements","royalty_reports"]},
                    "from": {"type":"string","pattern":"^[0-9]{4}-[0-9]{2}$","description":"royalty_reports: first month YYYY-MM"},
                    "to": {"type":"string","pattern":"^[0-9]{4}-[0-9]{2}$","description":"royalty_reports: last month YYYY-MM"}}}),
            },
            ToolDef {
                name: "delete_release_draft",
                title: "Delete a release draft",
                description: "Delete a draft that was never submitted. Only after the artist explicitly asked to delete this draft. Submitted releases cannot be deleted.",
                scope: SCOPE_WRITE, read_only: false, destructive: true, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","artist_confirmed"],"properties":{
                    "release_id": uuid_schema("The draft"),
                    "artist_confirmed": {"type":"boolean","const":true,"description":"The artist explicitly asked to delete this draft"}}}),
            },
            ToolDef {
                name: "request_release_withdrawal",
                title: "Ask to withdraw an application",
                description: "Ask the artist to approve withdrawing a submitted release's application (it stops the review; limited per month; impossible once delivery started). Only when the artist asked for it. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id"],"properties":{
                    "release_id": uuid_schema("The submitted release"),
                    "reason": {"type":"string","maxLength":500,"description":"Why, in the artist's words"}}}),
            },
            ToolDef {
                name: "request_takedown",
                title: "Ask to take a release down",
                description: "Ask the artist to approve a takedown of a release that is on platforms; AUDENIQ reviews it and removes it everywhere. Only when the artist asked for it. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","reason"],"properties":{
                    "release_id": uuid_schema("The delivered release"),
                    "reason": {"type":"string","minLength":2,"maxLength":1000,"description":"Why, in the artist's words"}}}),
            },
            ToolDef {
                name: "cancel_takedown",
                title: "Cancel a takedown request",
                description: "Cancel the artist's pending takedown request, so the release stays on platforms. Only after the artist explicitly asked.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","artist_confirmed"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "artist_confirmed": {"type":"boolean","const":true}}}),
            },
            ToolDef {
                name: "request_split_change",
                title: "Ask to change revenue splits",
                description: "Ask the artist to approve the collaborators and percentages of the listed tracks (replaces those tracks' splits; share_bps 2550 = 25.5%; existing collaborators by id from get_release_splits, new ones with name and email). New people get an invite to accept. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","tracks"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "tracks": {"type":"array","minItems":1,"maxItems":99,"items":{"type":"object","additionalProperties":false,"required":["track_id","shares"],"properties":{
                        "track_id": uuid_schema("The track"),
                        "shares": {"type":"array","maxItems":20,"items":{"type":"object","additionalProperties":false,"required":["name","share_bps"],"properties":{
                            "id": uuid_schema("An existing collaborator (from get_release_splits)"),
                            "name": {"type":"string","minLength":1,"maxLength":120},
                            "email": {"type":"string","maxLength":320,"description":"Needed for a new collaborator"},
                            "share_bps": {"type":"integer","minimum":1,"maximum":10000}}}}}}}}}),
            },
            ToolDef {
                name: "request_release_end",
                title: "Ask to end a release",
                description: "Ask the artist to approve ending a delivered release's service on a date (contract end, licence expiry, or transfer to another distributor named in transfer_to). The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","end_date","reason"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "end_date": {"type":"string","pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}$","description":"YYYY-MM-DD"},
                    "reason": {"type":"string","enum":["CONTRACT_END","LICENSE_EXPIRY","TRANSFER"]},
                    "note": {"type":"string","maxLength":1000},
                    "transfer_to": {"type":"string","maxLength":200,"description":"TRANSFER: the distributor taking it over"}}}),
            },
            ToolDef {
                name: "cancel_release_end",
                title: "Cancel a scheduled release end",
                description: "Cancel a release's scheduled end date before it arrives. Only after the artist explicitly asked.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","artist_confirmed"],"properties":{
                    "release_id": uuid_schema("The release"),
                    "artist_confirmed": {"type":"boolean","const":true}}}),
            },
            ToolDef {
                name: "request_contract_end",
                title: "Ask to end the AUDENIQ contract",
                description: "Ask the workspace owner to approve ending this workspace's AUDENIQ contract on a date (no new releases after that; all releases end on the date). The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["end_date"],"properties":{
                    "end_date": {"type":"string","pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}$","description":"YYYY-MM-DD"},
                    "note": {"type":"string","maxLength":1000}}}),
            },
            ToolDef {
                name: "cancel_contract_end",
                title: "Cancel a scheduled contract end",
                description: "Cancel the workspace's scheduled contract end before it arrives. Only after the artist explicitly asked.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["artist_confirmed"],"properties":{
                    "artist_confirmed": {"type":"boolean","const":true}}}),
            },
            ToolDef {
                name: "request_counter_notice",
                title: "Ask to answer an infringement claim",
                description: "Ask the artist to approve a counter notice to a rights claim that suspended their release (claim_id from list_infringement_claims, while can_answer is true). The statement is a legal statement in the artist's own words; never write claims for them. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["claim_id","statement"],"properties":{
                    "claim_id": uuid_schema("The claim"),
                    "statement": {"type":"string","minLength":1,"maxLength":3000}}}),
            },
            ToolDef {
                name: "request_member_invite",
                title: "Ask to invite a member",
                description: "Ask the workspace owner to approve inviting an existing AUDENIQ account by e-mail as EDITOR or VIEWER. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["email","role"],"properties":{
                    "email": {"type":"string","minLength":3,"maxLength":320},
                    "role": {"type":"string","enum":["EDITOR","VIEWER"]}}}),
            },
            ToolDef {
                name: "request_large_payout",
                title: "Ask for a payout above the assistant limit",
                description: "Ask the workspace owner to approve a payout to the registered account (any amount up to the available balance; amount, fee and net shown on the approval page). Never on your own initiative. The artist must approve it once in AUDENIQ Studio (approve_url); nothing happens until then. Then call get_action_request.",
                scope: SCOPE_PAYOUT, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["amount"],"properties":{
                    "amount": {"type":"integer","minimum":1,"maximum":1000000000,"description":"KRW before the fee"}}}),
            },
            ToolDef {
                name: "get_action_request",
                title: "Approval request status",
                description: "The state of an action the artist was asked to approve: PENDING, DONE (with result), FAILED (error_code), REJECTED, EXPIRED or REVOKED.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["request_id"],"properties":{
                    "request_id": uuid_schema("From a request_* tool")}}),
            },
            ToolDef {
                name: "set_special_options",
                title: "Set special-content details",
                description: "Set a draft's special-content details as the artist states them: cover song (original title, artist and writers per track, rights confirmation, licence file), samples (licence file), AI use (what for, which tools) and express release (reason, terms). Only facts the artist told you, after they confirmed the exact content; declarations at submission must match.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["release_id","artist_confirmed"],"properties":{
                    "release_id": uuid_schema("The draft"),
                    "artist_confirmed": {"type":"boolean","const":true,"description":"The artist confirmed these details"},
                    "cover": {"type":"object","additionalProperties":false,"required":["is_cover"],"properties":{
                        "is_cover": {"type":"boolean"},
                        "tracks": {"type":"array","maxItems":99,"items":{"type":"object","additionalProperties":false,"required":["track_id","original_title","original_artist"],"properties":{
                            "track_id": uuid_schema("The cover track"),
                            "original_title": {"type":"string","minLength":1,"maxLength":200},
                            "original_artist": {"type":"string","minLength":1,"maxLength":200},
                            "original_writers": {"type":"string","maxLength":300}}}},
                        "rights_confirmed": {"type":"boolean","description":"The artist confirmed they secured the cover licence"},
                        "license_asset_id": uuid_schema("Licence file (DOCUMENT upload)"),
                        "license_file_name": {"type":"string","maxLength":200}}},
                    "sample": {"type":"object","additionalProperties":false,"required":["contains_samples"],"properties":{
                        "contains_samples": {"type":"boolean"},
                        "license_asset_id": uuid_schema("Sample clearance file (DOCUMENT upload)"),
                        "license_file_name": {"type":"string","maxLength":200}}},
                    "ai": {"type":"object","additionalProperties":false,"required":["involved"],"properties":{
                        "involved": {"type":"boolean"},
                        "uses": {"type":"array","maxItems":10,"items":{"type":"string","maxLength":200},"description":"What AI was used for, e.g. '작곡(멜로디)에 AI 도움을 받았어요'"},
                        "tools": {"type":"array","maxItems":10,"items":{"type":"string","maxLength":100},"description":"e.g. Suno, Udio"}}},
                    "express": {"type":"object","additionalProperties":false,"required":["requested"],"properties":{
                        "requested": {"type":"boolean"},
                        "reason": {"type":"string","maxLength":300},
                        "terms_accepted": {"type":"boolean","description":"The artist accepted that schedule and costs are confirmed separately"}}}}}),
            },
            ToolDef {
                name: "list_artist_pages",
                title: "Artist platform pages",
                description: "Each artist in this workspace with their own page on Spotify, Apple Music, Melon and other platforms (link and status). AUDENIQ delivers new releases to these pages so they never land on a same-name stranger's page; PENDING means a first release is waiting for the new page's link.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "set_artist_pages",
                title: "Register artist platform pages",
                description: "Register or correct an artist's own page links (artist_id from list_artist_pages). Use only links the artist gave you; show them first. Each link must be that platform's artist page.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["artist_id","links"],"properties":{
                    "artist_id": uuid_schema("The artist (from list_artist_pages)"),
                    "links": {"type":"object","minProperties":1,"additionalProperties":false,"properties":
                        PLATFORM_LINK_PROPS.iter().map(|p| (p.to_string(), json!({"type":"string","maxLength":500,"pattern":"^https://"}))).collect::<serde_json::Map<String, Value>>()}}}),
            },
            ToolDef {
                name: "list_platform_requests",
                title: "Platform requests",
                description: "Requests the artist made for AUDENIQ to fix something at a platform after release (wrong artist page, merging or splitting pages, YouTube Official Artist Channel, Content ID), with status and the history of what AUDENIQ did.",
                scope: SCOPE_READ, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"properties":{
                    "group": {"type":"string","enum":["profile","content_id"],"description":"profile = artist pages and OAC, content_id = YouTube Content ID; omit for all"}}}),
            },
            ToolDef {
                name: "create_platform_request",
                title: "Ask AUDENIQ to fix something at a platform",
                description: "File a platform request: PROFILE_MISMATCH (a release sits on the wrong artist page; urls = that wrong page; release_id required), PROFILE_SPLIT (two artists share one page; description required), PROFILE_MERGE (duplicate pages; at least 2 urls), OAC (YouTube Official Artist Channel; url = the artist's channel), CID_ALLOWLIST (YouTube channels never to claim), CID_RELEASE_CLAIM (release a claim on a video), CID_DISPUTE (someone else's claim; description required), CID_OPT_OUT / CID_OPT_IN (Content ID for a release; release_id required). Show the artist exactly what will be sent and file it only after they agree; it is filed in their name.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: false, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["kind","artist_name"],"properties":{
                    "kind": {"type":"string","enum":["PROFILE_MISMATCH","PROFILE_SPLIT","PROFILE_MERGE","OAC","CID_ALLOWLIST","CID_RELEASE_CLAIM","CID_DISPUTE","CID_OPT_OUT","CID_OPT_IN"]},
                    "platform": {"type":"string","maxLength":40,"description":"For PROFILE_* kinds: melon, genie, flo, bugs, vibe, spotify, apple, youtube, amazon, deezer, tidal. YouTube kinds ignore it."},
                    "release_id": uuid_schema("The release concerned (required for PROFILE_MISMATCH, CID_OPT_OUT, CID_OPT_IN)"),
                    "artist_name": {"type":"string","minLength":1,"maxLength":200},
                    "urls": {"type":"array","maxItems":10,"items":{"type":"string","maxLength":500,"pattern":"^https://"}},
                    "description": {"type":"string","maxLength":2000}}}),
            },
            ToolDef {
                name: "cancel_platform_request",
                title: "Cancel a platform request",
                description: "Cancel a platform request AUDENIQ has not finished yet.",
                scope: SCOPE_WRITE, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["request_id"],"properties":{
                    "request_id": uuid_schema("The platform request")}}),
            },
            ToolDef {
                name: "get_release_splits",
                title: "Revenue splits of a release",
                description: "Per track, the collaborators who share its net royalty, their percentage (share_bps / 100), whether they accepted, and what was held or transferred to them; owner_bps is what the artist keeps. Read only: splits are changed in AUDENIQ Studio.",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: release_id_schema,
            },
            ToolDef {
                name: "list_received_splits",
                title: "Splits received from others",
                description: "Shares of other people's releases this workspace accepted, with the percentage and what was received so far.",
                scope: SCOPE_SETTLEMENT, read_only: true, destructive: false, idempotent: true, widget: false,
                schema: empty_schema,
            },
            ToolDef {
                name: "cancel_payout",
                title: "Cancel a payout request",
                description: "Cancel a payout request that has not been sent to the bank yet; the amount returns to the available balance.",
                scope: SCOPE_PAYOUT, read_only: false, destructive: false, idempotent: true, widget: false,
                schema: || json!({"type":"object","additionalProperties":false,"required":["payout_id"],"properties":{
                    "payout_id": uuid_schema("The payout request")}}),
            },
        ]
    })
}

fn empty_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{}})
}
fn uuid_schema(desc: &str) -> Value {
    json!({"type":"string","format":"uuid","pattern":"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$","description":desc})
}
fn release_id_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["release_id"],"properties":{"release_id": uuid_schema("The release")}})
}
fn title_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":200,"description":"Release title as it should appear on platforms"})
}
fn release_type_schema() -> Value {
    json!({"type":"string","enum":["SINGLE","EP","ALBUM"],"description":"SINGLE = 1 track, EP = 2-6 tracks, ALBUM = 7 or more"})
}
fn declarations_schema() -> Value {
    let b = |d: &str| json!({"type":"boolean","description":d});
    json!({"type":"object","additionalProperties":false,
        "required":["rights_confirmed","adult_confirmed","is_cover","is_remix","contains_samples","ai_involved","explicit_content"],
        "description":"Statements the artist makes about this release. Ask the artist; never assume.",
        "properties":{
            "rights_confirmed": b("The artist owns or controls all rights needed to distribute the recordings, compositions and artwork"),
            "adult_confirmed": b("The artist is 18 or older"),
            "is_cover": b("Contains a cover of someone else's song"),
            "is_remix": b("Is a remix of an existing recording"),
            "contains_samples": b("Contains samples of other recordings"),
            "ai_involved": b("AI tools were used to create the music, vocals or lyrics"),
            "explicit_content": b("Contains explicit content")}})
}
fn draft_fields_schema() -> Value {
    let date = json!({"type":"string","pattern":"^$|^[0-9]{4}-[0-9]{2}-[0-9]{2}$"});
    let mut release_date = date.clone();
    release_date["description"] = json!("Release date YYYY-MM-DD");
    let mut preorder = date;
    preorder["description"] =
        json!("Pre-order start YYYY-MM-DD, 1-90 days before the release date; empty for none");
    json!({"type":"object","additionalProperties":false,"properties":{
        "artist_name": {"type":"string","minLength":1,"maxLength":200,"description":"Primary artist name exactly as it should appear on platforms"},
        "language": {"type":"string","pattern":"^[a-z]{2,3}(-[A-Za-z0-9]{2,4})*$","maxLength":20,"description":"Metadata language, ISO 639 code (ko, en, ja, pt-BR)"},
        "genre": {"type":"string","minLength":1,"maxLength":60},
        "label": {"type":"string","maxLength":200,"description":"Label name; empty when self-released"},
        "release_date": release_date,
        "release_time": {"type":"string","pattern":"^$|^([01][0-9]|2[0-3]):(00|30)$","description":"Release time HH:MM in KST, on the hour or half hour; empty for platform default"},
        "preorder_start": preorder,
        "territories": {"type":"array","minItems":1,"maxItems":260,"uniqueItems":true,"items":{"type":"string","pattern":"^(WORLD|-?[A-Z]{2})$"},
            "description":"['WORLD'] for worldwide, ['WORLD','-JP'] to exclude countries, or ISO country codes ['KR','US'] for only those"},
        "platforms": {"type":"array","minItems":1,"maxItems":40,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":64},
            "description":"Platform slugs from list_distribution_platforms (available ones only)"},
        "p_line": {"type":"string","maxLength":200,"description":"Sound recording copyright: year and owner, e.g. '2026 Artist Name'"},
        "c_line": {"type":"string","maxLength":200,"description":"Composition/artwork copyright: year and owner"},
        "notes": {"type":"string","maxLength":2000,"description":"Album introduction"},
        "upc": {"type":"string","pattern":"^$|^[0-9]{12}$","description":"Existing UPC-A if the release has one; AUDENIQ issues one otherwise"}}})
}

pub fn find(name: &str) -> Option<&'static ToolDef> {
    defs().iter().find(|d| d.name == name)
}

/// Tool listing; `public_only` for the endpoint without an account.
/// Whether the connection may call the tool: its scope, or for
/// submit_release a write connection (the application then waits for the
/// artist's one-time approval).
pub fn permitted(def: &ToolDef, p: &Principal) -> bool {
    p.has(def.scope) || (def.name == "submit_release" && p.has(SCOPE_WRITE))
}

pub fn list_json(public_only: bool) -> Vec<Value> {
    defs()
        .iter()
        .filter(|d| !public_only || d.public())
        .map(|d| {
            let security = if d.public() {
                json!([{"type":"noauth"}, {"type":"oauth2","scopes":[d.scope]}])
            } else {
                json!([{"type":"oauth2","scopes":[d.scope]}])
            };
            let mut meta = json!({
                "securitySchemes": security,
                "openai/toolInvocation/invoking": format!("{}…", d.title),
                "openai/toolInvocation/invoked": d.title,
            });
            if d.widget {
                meta["ui"] = json!({"resourceUri": WIDGET_URI});
                meta["openai/outputTemplate"] = json!(WIDGET_URI);
            }
            json!({
                "name": d.name,
                "title": d.title,
                "description": d.description,
                "inputSchema": d.input_schema(),
                "annotations": {
                    "title": d.title,
                    "readOnlyHint": d.read_only,
                    "destructiveHint": d.destructive,
                    "idempotentHint": d.idempotent,
                    "openWorldHint": false,
                },
                "securitySchemes": security,
                "_meta": meta,
            })
        })
        .collect()
}

pub fn resources_json() -> Vec<Value> {
    vec![json!({
        "uri": WIDGET_URI,
        "name": "release-card",
        "title": "AUDENIQ release card",
        "description": "Renders release details, check results, application previews and status.",
        "mimeType": WIDGET_MIME,
    })]
}

pub fn read_resource(uri: &str) -> Option<Value> {
    (uri == WIDGET_URI).then(|| {
        json!({
            "uri": WIDGET_URI,
            "mimeType": WIDGET_MIME,
            "text": WIDGET_HTML,
            "_meta": {
                // No network access and no external resources: the card only
                // renders the tool result it is given.
                "ui": {"csp": {"connectDomains": [], "resourceDomains": []}, "prefersBorder": true},
                "openai/widgetDescription": "Shows an AUDENIQ release, its check results, the application preview or its review status.",
                "openai/widgetCSP": {"connect_domains": [], "resource_domains": []},
            }
        })
    })
}

fn validator(def: &ToolDef) -> &'static jsonschema::Validator {
    static CACHE: OnceLock<HashMap<&'static str, jsonschema::Validator>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        defs()
            .iter()
            .map(|d| {
                (
                    d.name,
                    jsonschema::validator_for(&d.input_schema()).expect("tool schemas are valid"),
                )
            })
            .collect()
    });
    &cache[def.name]
}

/// A failed tool call: the stable code, a human explanation and, where it
/// helps, what to do next. Internal details never reach the client.
pub(super) struct ToolError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}
impl From<Error> for ToolError {
    fn from(e: Error) -> Self {
        let code = error_code(&e);
        if code == "INTERNAL_ERROR" {
            tracing::warn!(error = %e, "mcp tool failed");
        }
        ToolError {
            code: code.into(),
            message: crate::error::message(code)
                .or_else(|| mcp_message(code))
                .unwrap_or("The request was refused.")
                .into(),
            details: None,
        }
    }
}
pub(super) fn tool_error(code: &str, message: &str) -> ToolError {
    ToolError {
        code: code.into(),
        message: message.into(),
        details: None,
    }
}
fn mcp_message(code: &str) -> Option<&'static str> {
    Some(match code {
        "DSP_SELECTION_REQUIRED" => {
            "Choose at least one platform (platforms) from list_distribution_platforms."
        }
        "DSP_SELECTION_INVALID" => {
            "A selected platform is unknown. Use slugs from list_distribution_platforms."
        }
        "DSP_UNAVAILABLE" => "A selected platform is not available for this workspace right now.",
        "DECLARATION_REQUIRED" => {
            "The artist must confirm they hold the rights and are 18 or older before submitting."
        }
        "MINORITY_REVIEW_REQUIRED" => {
            "Releases by artists under 18 need a guardian process that is not available yet. Contact AUDENIQ support."
        }
        "CONSENT_SCOPE_MISMATCH" => "The release changed while submitting. Preview again.",
        "ORG_CONTRACT_ENDING" => {
            "This workspace's AUDENIQ contract is ending, so new releases are not accepted."
        }
        "SUBMIT_PERMISSION_REQUIRED" => "Only workspace owners and editors can submit releases.",
        "TERRITORY_INVALID" => {
            "territories must be ['WORLD'], ['WORLD','-XX',...] or a list of ISO country codes."
        }
        "RELEASE_TIME_INVALID" => "release_time must be HH:MM on the hour or half hour (KST).",
        "PREORDER_START_INVALID" => "preorder_start must be 1 to 90 days before the release date.",
        "SPECIAL_DECLARATION_MISMATCH" => {
            "The draft's cover/sample/AI options disagree with the declarations. Fix them in AUDENIQ Studio or adjust the declarations."
        }
        "CORRECTION_NOTHING_SENT" => {
            "Corrections are for releases already sent to a platform. Before that, edit the draft (or the release returned for correction) instead."
        }
        "CORRECTION_ALREADY_REQUESTED" => {
            "This release already has a correction request waiting for review. Cancel it first to send a different one."
        }
        "CORRECTION_ARTIST_LOCKED" => {
            "Artist and featured-artist names cannot be corrected after delivery; that needs a new release."
        }
        "CORRECTION_INVALID" => "The correction lists an unknown track or an invalid credit.",
        "CORRECTION_EMPTY" => "Give a new genre or at least one track's credits.",
        "CORRECTION_REASON_REQUIRED" => "Give a reason for the correction.",
        "PITCH_RELEASE_NOT_SUBMITTED" => {
            "Pitches are for submitted releases. Submit the release first."
        }
        "PITCH_TOO_LATE" => {
            "Pitches must reach editors at least 7 days before the release date; this release is too close (or has no date)."
        }
        "PITCH_INVALID" => {
            "The description must be 20 to 1000 characters and the track must belong to the release."
        }
        "PAYOUT_ACCOUNT_REQUIRED" => {
            "No payout account is registered. The artist registers it in AUDENIQ Studio (settlement)."
        }
        "PAYOUT_EXCEEDS_BALANCE" => "The amount is more than the available balance.",
        "PAYOUT_BELOW_MINIMUM" => "The amount is below the minimum payout.",
        "PAYOUT_NET_NOT_POSITIVE" => {
            "After the fee nothing would be paid out; request a larger amount."
        }
        "PAYEE_ON_HOLD" => "Payouts are on hold for this account. Contact AUDENIQ support.",
        "PAYOUT_NOT_CANCELLABLE" => {
            "This payout was already sent to the bank and can no longer be cancelled."
        }
        "COVER_DETAILS_REQUIRED" | "AI_DETAILS_REQUIRED" | "EXPRESS_ACK_REQUIRED" => {
            "This release's special options need details: set them with set_special_options (or in AUDENIQ Studio)."
        }
        _ => return None,
    })
}

pub(super) fn ok_result(text: String, structured: Value) -> Value {
    json!({"content": [{"type": "text", "text": text}], "structuredContent": structured, "isError": false})
}
pub(super) fn err_result(e: ToolError) -> Value {
    let mut err = json!({"code": e.code, "message": e.message});
    if let Some(d) = e.details {
        err["details"] = d;
    }
    json!({
        "content": [{"type": "text", "text": format!("Error {}: {}", e.code, e.message)}],
        "structuredContent": {"view": "error", "error": err},
        "isError": true,
    })
}

/// Runs a tool for an authenticated principal (scope already checked).
/// The error result for arguments that do not match the tool's schema.
pub(super) fn invalid_arguments(def: &ToolDef, args: &Value) -> Option<Value> {
    let errors: Vec<String> = validator(def)
        .iter_errors(args)
        .take(10)
        .map(|e| e.to_string().chars().take(300).collect())
        .collect();
    if errors.is_empty() {
        return None;
    }
    let mut e = tool_error(
        "INVALID_ARGUMENTS",
        "The arguments do not match the tool's input schema.",
    );
    e.details = Some(json!({"problems": errors}));
    Some(err_result(e))
}

pub async fn call(s: &AppState, p: &Principal, def: &ToolDef, args: Value) -> Value {
    if let Some(invalid) = invalid_arguments(def, &args) {
        return invalid;
    }
    let started = std::time::Instant::now();
    // Shorter than the edge's and clients' own deadlines, so the client gets
    // an answer it can act on instead of a dropped connection and a retry.
    let request_key = args
        .get("client_request_id")
        .and_then(Value::as_str)
        .filter(|_| def.retry_safe())
        .map(str::to_owned);
    let outcome = match &request_key {
        Some(key) => match super::safety::reserve(&s.pool, p.grant, def.name, key).await {
            Ok(super::safety::Reserved::New) => {
                let r = run_with_deadline(s, p, def, &args).await;
                let id = r.as_ref().ok().and_then(|v| created_id(def.name, v));
                super::safety::finish(&s.pool, p.grant, def.name, key, id).await;
                r
            }
            Ok(super::safety::Reserved::Done(id)) => Box::pin(replay(s, p, def, id)).await,
            Ok(super::safety::Reserved::Running) => Err(tool_error(
                "REQUEST_IN_PROGRESS",
                "The first request with this client_request_id is still running. Wait a moment and check the result (list_releases, list_inquiries, list_platform_requests) instead of sending it again.",
            )),
            Err(e) => Err(e.into()),
        },
        None => run_with_deadline(s, p, def, &args).await,
    };
    let ok = outcome.is_ok();
    let code = outcome
        .as_ref()
        .err()
        .map(|e| e.code.as_str())
        .unwrap_or("");
    tracing::info!(tool = def.name, grant = %p.grant, ok, code, elapsed_ms = started.elapsed().as_millis(), "mcp_tool");
    if !def.read_only {
        // Consequential calls are recorded with the client that made them;
        // the services add their own audit rows under the same request id.
        let release = args
            .get("release_id")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<Uuid>().ok())
            .or_else(|| {
                outcome
                    .as_ref()
                    .ok()
                    .and_then(|v| v["release_id"].as_str()?.parse().ok())
            });
        let reason = if ok { "MCP_OK" } else { "MCP_REJECTED" };
        let action = format!("mcp.{}", def.name);
        let audit: Result<()> = async {
            let mut tx = s.pool.begin().await?;
            operations::audit(
                &mut tx,
                Some(p.actor.user),
                Some(p.org),
                release,
                &action,
                reason,
                p.actor.request,
            )
            .await?;
            tx.commit().await?;
            Ok(())
        }
        .await;
        if let Err(e) = audit {
            tracing::warn!(error = %e, "mcp audit failed");
        }
    }
    match outcome {
        Ok(mut v) => {
            if let Some(m) = super::voice::message_ko(&v) {
                v["message_ko"] = json!(m);
            }
            let text = summary(def.name, &v);
            ok_result(text, v)
        }
        Err(e) => err_result(e),
    }
}

async fn run_with_deadline(
    s: &AppState,
    p: &Principal,
    def: &ToolDef,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    match tokio::time::timeout(TOOL_DEADLINE, run(s, p, def, args)).await {
        Ok(v) => v,
        Err(_) => Err(tool_error(
            "TOOL_TIMEOUT",
            "This took too long and was stopped. It may have partly completed: check the current state (get_release, get_submission_status, list_payouts) before trying again.",
        )),
    }
}

/// Records a retry-safe create's result the moment it is committed, so a
/// later failure (or the deadline) in the same call cannot free the
/// `client_request_id` for a duplicate.
async fn record_created(s: &AppState, grant: Uuid, tool: &str, args: &Value, id: Uuid) {
    if let Some(key) = args.get("client_request_id").and_then(Value::as_str) {
        super::safety::finish(&s.pool, grant, tool, key, Some(id)).await;
    }
}

/// The id a retry-safe create returned.
fn created_id(tool: &str, v: &Value) -> Option<Uuid> {
    let key = if tool == "create_release_draft" {
        "release_id"
    } else {
        "id"
    };
    v[key].as_str().and_then(|v| v.parse().ok())
}

/// What the first call of a retried create made.
async fn replay(
    s: &AppState,
    p: &Principal,
    def: &ToolDef,
    id: Uuid,
) -> std::result::Result<Value, ToolError> {
    let (a, org) = (&p.actor, p.org);
    let mut v = match def.name {
        "create_release_draft" => release_view(s, a, org, id).await?,
        "create_inquiry" => {
            let mut v = crate::portal::get_inquiry(s, a, org, id).await?;
            v["view"] = json!("inquiry");
            v
        }
        _ => json!({"view": "platform_request", "id": id}),
    };
    v["duplicate_request"] = json!(true);
    Ok(v)
}

/// A short text rendering for clients that only read `content`, followed by
/// the structured data.
fn summary(tool: &str, v: &Value) -> String {
    let mut json = v.to_string();
    if json.len() > 60_000 {
        // Cut on a character boundary (Korean text is 3 bytes per character).
        let mut cut = 60_000;
        while !json.is_char_boundary(cut) {
            cut -= 1;
        }
        json.truncate(cut);
        json.push('…');
    }
    let lead = match tool {
        "submit_release" => format!(
            "Application submitted to AUDENIQ review (status {}).",
            v["status"].as_str().unwrap_or("?")
        ),
        "validate_release" | "preview_submission" => {
            if v["ready_to_submit"] == true {
                "All pre-submission checks pass.".to_string()
            } else {
                format!(
                    "{} issue(s) block submission.",
                    v["blocking"].as_array().map_or(0, Vec::len)
                )
            }
        }
        "request_payout" => format!(
            "Payout requested: {} KRW (fee {}, net {}), status {}.",
            v["amount"].as_str().unwrap_or("?"),
            v["fee"].as_str().unwrap_or("?"),
            v["net"].as_str().unwrap_or("?"),
            v["status"].as_str().unwrap_or("?")
        ),
        _ => String::new(),
    };
    if lead.is_empty() {
        json
    } else {
        format!("{lead}\n{json}")
    }
}

type ToolFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = std::result::Result<Value, ToolError>> + Send + 'a>,
>;

async fn rate(s: &AppState, p: &Principal, def: &ToolDef) -> Result<()> {
    crate::auth::rate(&s.pool, &format!("mcp:{}", p.grant), 900).await?;
    if !def.read_only {
        crate::auth::rate(&s.pool, &format!("mcp-write:{}", p.grant), 300).await?;
    }
    if def.scope == SCOPE_PAYOUT && !def.read_only {
        crate::auth::rate(&s.pool, &format!("mcp-payout:{}", p.grant), 10).await?;
    }
    if def.scope == SCOPE_SUBMIT {
        crate::auth::rate(&s.pool, &format!("mcp-submit:{}", p.grant), 30).await?;
    }
    Ok(())
}

async fn run(
    s: &AppState,
    p: &Principal,
    def: &ToolDef,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    rate(s, p, def).await?;
    let a = &p.actor;
    let org = p.org;
    // Each tool's future is boxed: together they would make one very large
    // state machine (deep stacks in debug builds).
    let fut: ToolFuture<'_> = match def.name {
        "get_account_context" => Box::pin(account(s, p)),
        "request_permission" => Box::pin(super::approvals::request(s, p, args)),
        "get_permission_request" => Box::pin(super::approvals::status(s, p, args)),
        "start_upload" => Box::pin(super::files::start(s, p, args)),
        "complete_upload" => Box::pin(super::files::complete(s, p, args)),
        "import_file_from_url" => Box::pin(super::files::import(s, p, args)),
        "get_upload_status" => Box::pin(super::files::status(s, p, args)),
        "list_distribution_platforms" => Box::pin(platforms(s, a, org)),
        "get_release_requirements" => Box::pin(async { Ok(requirements()) }),
        "list_releases" => Box::pin(list_releases(s, a, org, args)),
        "get_release" => Box::pin(async move { release_view(s, a, org, release_arg(args)?).await }),
        "create_release_draft" => Box::pin(create_draft(s, p.grant, a, org, args)),
        "update_release_draft" => Box::pin(update_draft(s, a, org, args)),
        "upsert_track" => Box::pin(upsert_track(s, a, org, args)),
        "set_track_credits" => Box::pin(set_credits(s, a, org, args)),
        "validate_release" => Box::pin(async move {
            validate(s, a, org, release_arg(args)?, &DeclarationsInput::default()).await
        }),
        "preview_submission" => Box::pin(preview(s, p, args)),
        "submit_release" => Box::pin(submit(s, p, args)),
        "get_submission_status" => {
            Box::pin(async move { status(s, a, org, release_arg(args)?).await })
        }
        "get_release_corrections" => {
            Box::pin(async move { corrections_view(s, a, org, release_arg(args)?).await })
        }
        "request_release_correction" => Box::pin(request_correction(s, a, org, args)),
        "cancel_release_correction" => Box::pin(async move {
            let release = release_arg(args)?;
            let v = crate::corrections::cancel(s, a, org, release).await?;
            Ok(json!({"view": "correction", "release_id": release, "request": v}))
        }),
        "get_playlist_pitch" => Box::pin(async move {
            let release = release_arg(args)?;
            let mut v = crate::pitching::status(s, a, org, release).await?;
            v["view"] = json!("pitch");
            v["release_id"] = json!(release);
            v["lead_days"] = json!(crate::pitching::LEAD_DAYS);
            Ok(v)
        }),
        "submit_playlist_pitch" => Box::pin(async move {
            let release = release_arg(args)?;
            let text = |k: &str| str_arg(args, k).unwrap_or_default().to_string();
            let input = crate::pitching::PitchInput {
                track_id: uuid_arg(args, "track_id")?,
                description: text("description"),
                genres: text("genres"),
                moods: text("moods"),
                instruments: text("instruments"),
                similar_artists: text("similar_artists"),
                marketing: text("marketing"),
            };
            crate::pitching::submit(s, a, org, release, input).await?;
            let mut v = crate::pitching::status(s, a, org, release).await?;
            v["view"] = json!("pitch");
            v["release_id"] = json!(release);
            Ok(v)
        }),
        "withdraw_playlist_pitch" => Box::pin(async move {
            let release = release_arg(args)?;
            crate::pitching::withdraw(s, a, org, release).await?;
            let mut v = crate::pitching::status(s, a, org, release).await?;
            v["view"] = json!("pitch");
            v["release_id"] = json!(release);
            Ok(v)
        }),
        "get_smart_link" => {
            Box::pin(async move { smart_link(s, a, org, release_arg(args)?).await })
        }
        "save_smart_link" => Box::pin(async move {
            let release = release_arg(args)?;
            let links = args
                .get("links")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect();
            let input = crate::smart_links::LinkInput {
                enabled: args.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                links,
            };
            crate::smart_links::save(s, a, org, release, input).await?;
            smart_link(s, a, org, release).await
        }),
        "list_release_codes" => Box::pin(async move {
            Ok(json!({"view": "codes", "releases": crate::codes::list(s, a, org).await?}))
        }),
        "list_notifications" => Box::pin(async move {
            let mut v = crate::portal::list_notifications(s, a, org).await?;
            v["view"] = json!("notifications");
            Ok(v)
        }),
        "list_documents" => Box::pin(documents(s, a, org)),
        "list_inquiries" => Box::pin(async move {
            let mut v = crate::portal::list_inquiries(s, a, org).await?;
            v["view"] = json!("inquiries");
            v["studio_url"] = json!(format!("{}/inquiries", super::issuer(s)));
            Ok(v)
        }),
        "get_inquiry" => Box::pin(async move {
            let mut v =
                crate::portal::get_inquiry(s, a, org, uuid_arg(args, "inquiry_id")?).await?;
            v["view"] = json!("inquiry");
            Ok(v)
        }),
        "create_inquiry" => Box::pin(async move {
            let input = crate::portal::InquiryInput {
                category: str_arg(args, "category").unwrap_or_default().to_string(),
                release_id: args
                    .get("release_id")
                    .and_then(Value::as_str)
                    .and_then(|v| v.parse().ok()),
                subject: str_arg(args, "subject").unwrap_or_default().to_string(),
                body: str_arg(args, "body").unwrap_or_default().to_string(),
            };
            let created = crate::portal::create_inquiry(s, a, org, input).await?;
            let id = created["id"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .ok_or(Error::Internal)?;
            record_created(s, p.grant, "create_inquiry", args, id).await;
            let mut v = crate::portal::get_inquiry(s, a, org, id).await?;
            v["view"] = json!("inquiry");
            Ok(v)
        }),
        "reply_inquiry" => Box::pin(async move {
            let id = uuid_arg(args, "inquiry_id")?;
            let input = crate::portal::MessageInput {
                body: str_arg(args, "body").unwrap_or_default().to_string(),
            };
            crate::portal::add_message(s, a, org, id, input).await?;
            let mut v = crate::portal::get_inquiry(s, a, org, id).await?;
            v["view"] = json!("inquiry");
            Ok(v)
        }),
        "get_settlement_summary" => Box::pin(settlement_summary(s, p)),
        "list_settlement_statements" => Box::pin(statements(s, a, org, args)),
        "get_royalty_reports" => Box::pin(async move {
            let period = crate::portal::Period {
                from: str_arg(args, "from").map(str::to_owned),
                to: str_arg(args, "to").map(str::to_owned),
            };
            let mut v = crate::portal::reports(s, a, org, period).await?;
            v["view"] = json!("royalty_reports");
            Ok(v)
        }),
        "list_payouts" => Box::pin(payouts(s, a, org)),
        "preview_payout" => Box::pin(preview_payout(s, p, args)),
        "request_payout" => Box::pin(request_payout(s, p, args)),
        "cancel_payout" => Box::pin(async move {
            let id = uuid_arg(args, "payout_id")?;
            let mut v = crate::portal::cancel_payout(s, a, org, id).await?;
            v["view"] = json!("payout");
            Ok(v)
        }),
        "get_upload_link" => Box::pin(upload_link(s, a, org, args)),
        "list_artist_pages" => Box::pin(async move {
            let mut v = crate::artist_pages::list(s, a, org).await?;
            v["view"] = json!("artist_pages");
            Ok(v)
        }),
        "set_artist_pages" => Box::pin(async move {
            let artist = uuid_arg(args, "artist_id")?;
            let links = args["links"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect();
            let input = crate::artist_pages::PagesInput { links };
            let mut v = crate::artist_pages::set_pages(s, a, org, artist, input).await?;
            v["view"] = json!("artist_pages_saved");
            Ok(v)
        }),
        "list_platform_requests" => Box::pin(async move {
            let q = crate::platform_requests::ListQuery {
                group: str_arg(args, "group").unwrap_or_default().to_string(),
            };
            let mut v = crate::platform_requests::list(s, a, org, q).await?;
            v["view"] = json!("platform_requests");
            v["studio_url"] = json!(format!("{}/inquiries", super::issuer(s)));
            Ok(v)
        }),
        "create_platform_request" => Box::pin(async move {
            let input = crate::platform_requests::NewRequest {
                kind: str_arg(args, "kind").unwrap_or_default().to_string(),
                platform: str_arg(args, "platform").unwrap_or_default().to_string(),
                release_id: args
                    .get("release_id")
                    .and_then(Value::as_str)
                    .and_then(|v| v.parse().ok()),
                artist_name: str_arg(args, "artist_name").unwrap_or_default().to_string(),
                urls: args["urls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect(),
                description: str_arg(args, "description").unwrap_or_default().to_string(),
            };
            let mut v = crate::platform_requests::create(s, a, org, input).await?;
            if let Some(id) = v["id"].as_str().and_then(|v| v.parse().ok()) {
                record_created(s, p.grant, "create_platform_request", args, id).await;
            }
            v["view"] = json!("platform_request");
            Ok(v)
        }),
        "cancel_platform_request" => Box::pin(async move {
            let id = uuid_arg(args, "request_id")?;
            let mut v = crate::platform_requests::cancel(s, a, org, id).await?;
            v["view"] = json!("platform_request");
            Ok(v)
        }),
        "get_release_splits" => Box::pin(async move {
            let release = release_arg(args)?;
            let mut v = crate::splits::sheet(s, a, org, release).await?;
            v["view"] = json!("splits");
            v["release_id"] = json!(release);
            v["studio_url"] = json!(studio_url(s, release));
            Ok(v)
        }),
        "list_received_splits" => Box::pin(async move {
            let mut v = crate::splits::received(s, a, org).await?;
            v["view"] = json!("received_splits");
            Ok(v)
        }),
        // One arm for the workspace tools keeps this (non-async) match's
        // stack frame small in debug builds.
        n if super::workspace::TOOLS.contains(&n) => {
            Box::pin(super::workspace::call(s, p, n, args))
        }
        n if super::actions::TOOLS.contains(&n) => Box::pin(super::actions::call(s, p, n, args)),
        _ => return Err(tool_error("UNKNOWN_TOOL", "Unknown tool.")),
    };
    fut.await
}

fn uuid_arg(args: &Value, key: &str) -> std::result::Result<Uuid, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| tool_error("INVALID_ARGUMENTS", &format!("{key} must be a UUID.")))
}
fn release_arg(args: &Value) -> std::result::Result<Uuid, ToolError> {
    uuid_arg(args, "release_id")
}
fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn studio_url(s: &AppState, release: Uuid) -> String {
    format!("{}/releases/{release}", super::issuer(s))
}
fn studio_edit_url(s: &AppState, release: Uuid) -> String {
    format!("{}/upload?edit={release}", super::issuer(s))
}

/// Where the artist uploads this release's files, and what is missing.
async fn upload_link(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let (r, _) = load(s, a, org, release).await?;
    let missing_audio: Vec<Value> = r["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| !t["asset_id"].is_string())
        .map(|t| json!({"track_id": t["id"], "title": t["title"]}))
        .collect();
    let artwork_missing = !r["artwork_asset_id"].is_string();
    Ok(json!({
        "view": "upload_link",
        "release_id": release,
        "upload_url": studio_edit_url(s, release),
        "tracks_without_audio": missing_audio,
        "artwork_missing": artwork_missing,
        "complete": missing_audio.is_empty() && !artwork_missing,
        "note": "Open while signed in to AUDENIQ Studio. Audio: WAV or FLAC; cover: square JPG/PNG (see get_release_requirements).",
    }))
}

// ---------------------------------------------------------------------------
// Account and discovery
// ---------------------------------------------------------------------------

async fn account(s: &AppState, p: &Principal) -> std::result::Result<Value, ToolError> {
    let ws = catalog::workspace(s, &p.actor, p.org).await?;
    let role = ws["role"].as_str().unwrap_or("");
    let can_edit = role != "VIEWER" && p.has(SCOPE_WRITE);
    let can_submit = matches!(role, "OWNER" | "EDITOR") && p.has(SCOPE_SUBMIT);
    // Without the submit permission a write connection can still send an
    // application, which then waits for the artist's one-time approval.
    let can_submit_with_approval =
        matches!(role, "OWNER" | "EDITOR") && !can_submit && p.has(SCOPE_WRITE);
    let payout_limit = s.mcp.payout_limit_krw;
    let can_request_payouts = role == "OWNER" && p.has(SCOPE_PAYOUT) && payout_limit > 0;
    Ok(json!({
        "view": "account",
        "workspace": ws,
        "connection": {"client": p.client_name, "scopes": p.scopes},
        "can_edit_drafts": can_edit,
        "can_submit": can_submit,
        "can_submit_with_approval": can_submit_with_approval,
        "can_view_settlement": p.has(SCOPE_SETTLEMENT),
        "can_request_payouts": can_request_payouts,
        "assistant_payout_limit_krw": payout_limit,
        "studio_url": super::issuer(s),
        // For clients that drop server instructions: write AUDENIQ in English letters, in Korean too;
        // 어데닉 is only how it is read (particles follow it), never a spelling to use.
        "brand": {"name": "AUDENIQ", "name_ko": "AUDENIQ", "reading_ko": "어데닉", "studio_ko": "AUDENIQ Studio"},
        "not_available_here": ["signing the application or agreement", "withdrawals and takedowns", "payout account changes", "payouts above the assistant limit", "contracts", "account and member settings"],
    }))
}

async fn platforms(s: &AppState, a: &Actor, org: Uuid) -> std::result::Result<Value, ToolError> {
    let avail = crate::api::dsp_availability(s, a, org).await?;
    let items: Vec<Value> = avail["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let slug = item["slug"].as_str().unwrap_or("");
            let mut out = json!({
                "slug": slug, "name": item["name"], "region": item["region"],
                "category": item["category"], "available": item["available"],
            });
            if let Some(spec) = crate::dsp_registry::REGISTRY
                .iter()
                .find(|d| d.slug == slug)
            {
                out["name_en"] = json!(spec.name);
                out["requirements"] = json!({
                    "artwork_min_px": spec.artwork_min_px,
                    "artwork_max_px": spec.artwork_max_px,
                    "audio_min_sample_rate_hz": spec.audio_min_sample_rate,
                    "audio_min_bit_depth": spec.audio_min_bits,
                    "lossless_only": spec.lossless_only,
                    "lead_days": spec.lead_days,
                    "requires_composer_credit": spec.requires_composer,
                    "requires_lyricist_credit": spec.requires_lyricist,
                    "loudness_target_lufs": spec.loudness_target_lufs,
                    "cover_license_required": spec.cover_license_required,
                });
            }
            out
        })
        .collect();
    Ok(json!({
        "view": "platforms",
        "platforms": items,
        "note": "available=false means the platform cannot be selected for this workspace yet (no active delivery contract or route). lead_days is the recommended minimum time between submission and release date.",
    }))
}

pub(super) fn requirements() -> Value {
    let types: Vec<Value> = ["SINGLE", "EP", "ALBUM"]
        .iter()
        .map(|t| {
            let (min, max) = submission::release_type_tracks(t).unwrap_or((1, None));
            json!({"release_type": t, "min_tracks": min, "max_tracks": max})
        })
        .collect();
    let audio: Vec<&str> = [
        "audio/wav",
        "audio/flac",
        "audio/mp4",
        "audio/aiff",
        "audio/wavpack",
        "audio/tta",
    ]
    .into_iter()
    .filter(|t| crate::uploads::expected_container("AUDIO", t).is_some())
    .collect();
    let images: Vec<&str> = ["image/jpeg", "image/png"]
        .into_iter()
        .filter(|t| crate::uploads::expected_container("IMAGE", t).is_some())
        .collect();
    json!({
        "view": "requirements",
        "release_types": types,
        "required_for_submission": [
            {"field": "title", "rule": "not empty; no version text, explicit markers or promotional text"},
            {"field": "artist_name", "rule": "the primary artist as shown on platforms"},
            {"field": "release_date", "rule": "YYYY-MM-DD"},
            {"field": "p_line", "rule": "sound recording copyright: year and owner"},
            {"field": "c_line", "rule": "composition/artwork copyright: year and owner"},
            {"field": "platforms", "rule": "at least one available platform slug"},
            {"field": "tracks", "rule": "track count matching the release type, numbered 1..n per disc without gaps, unique titles"},
            {"field": "track audio", "rule": "every track has an uploaded, verified audio file"},
            {"field": "track credits", "rule": "every track has at least one composer or lyricist"},
        ],
        "optional": ["language (ISO 639, default ko)", "genre", "label", "release_time (HH:MM KST, :00 or :30)", "preorder_start (1-90 days before release_date)", "territories (default worldwide)", "upc", "isrc per track", "version per track", "lyrics", "album notes"],
        "credit_roles": CREDIT_ROLES.iter().map(|(k, role, _)| json!({"argument": k, "role": role})).collect::<Vec<_>>(),
        "files": {
            "upload_in": "AUDENIQ Studio (release editor). Files cannot be sent through the AI assistant.",
            "audio_types": audio,
            "audio_max_bytes": crate::uploads::MAX_AUDIO_BYTES,
            "audio_note": "Lossless masters only (WAV, FLAC, ALAC, AIFF, WavPack, TTA); stored as FLAC. MP3/AAC are refused.",
            "artwork_types": images,
            "artwork_max_bytes": crate::uploads::MAX_IMAGE_BYTES,
            "artwork_note": "Square; see artwork_min_px per platform in list_distribution_platforms.",
        },
        "declarations_at_submission": ["rights_confirmed (required true)", "adult_confirmed (required true)", "is_cover", "is_remix", "contains_samples", "ai_involved", "explicit_content"],
        "after_submission": "AUDENIQ runs automatic checks (metadata, audio QC, rights) and staff review. The artist signs the release application and distribution agreement in AUDENIQ Studio; delivery to platforms starts only after the agreement is signed and approved.",
    })
}

// ---------------------------------------------------------------------------
// Releases
// ---------------------------------------------------------------------------

async fn list_releases(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let page = catalog::Page {
        after: str_arg(args, "cursor").and_then(|v| v.parse().ok()),
        limit: Some(args.get("limit").and_then(Value::as_i64).unwrap_or(20)),
    };
    let v = catalog::list(s, a, org, Kind::Release, page).await?;
    let items: Vec<Value> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let d = &r["draft"];
            json!({
                "release_id": r["id"], "title": r["title"], "release_type": r["release_type"],
                "status": r["status"], "artist": d["artist"], "release_date": d["release_date"],
                "agreement_status": r["agreement_status"],
            })
        })
        .collect();
    Ok(json!({"view": "releases", "releases": items, "next_cursor": v["next_cursor"]}))
}

/// The release as catalog::get returns it, plus credit names.
pub(super) async fn load(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> Result<(Value, HashMap<Uuid, String>)> {
    let r = catalog::get(s, a, org, Kind::Release, release).await?;
    let parties: Vec<Uuid> = r["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["credits"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["party_id"].as_str().and_then(|v| v.parse().ok()))
        .collect();
    let names = if parties.is_empty() {
        HashMap::new()
    } else {
        catalog::party_names(s, a, org, &parties).await?
    };
    Ok((r, names))
}

/// A track's credit names for one role. Credits carry no order, so the
/// order the artist gave (the Studio editor's "A, B" string) is applied.
fn credit_names(
    t: &Value,
    names: &HashMap<Uuid, String>,
    role: &str,
    order: Option<&str>,
) -> Vec<String> {
    let mut out: Vec<String> = t["credits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["role"].as_str() == Some(role))
        .filter_map(|c| c["party_id"].as_str().and_then(|v| v.parse().ok()))
        .filter_map(|id: Uuid| names.get(&id).cloned())
        .collect();
    let order: Vec<&str> = order.unwrap_or("").split(',').map(str::trim).collect();
    out.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(usize::MAX));
    out
}

/// The Studio editor's entry for a server track.
fn draft_entry<'a>(draft: &'a Value, track_id: &str) -> Option<&'a Value> {
    draft["draftTracks"]
        .as_array()?
        .iter()
        .find(|o| o["serverId"].as_str() == Some(track_id))
}

fn track_view(t: &Value, names: &HashMap<Uuid, String>, draft: &Value) -> Value {
    let entry = draft_entry(draft, t["id"].as_str().unwrap_or_default());
    let mut credits = serde_json::Map::new();
    for (key, role, field) in CREDIT_ROLES {
        let order = entry.and_then(|e| e[field].as_str());
        credits.insert(key.into(), json!(credit_names(t, names, role, order)));
    }
    let other: Vec<Value> = t["credits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            !CREDIT_ROLES
                .iter()
                .any(|(_, r, _)| c["role"].as_str() == Some(r))
        })
        .map(|c| {
            let name = c["party_id"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .and_then(|id: Uuid| names.get(&id).cloned());
            json!({"name": name, "role": c["role"]})
        })
        .collect();
    if !other.is_empty() {
        credits.insert("other".into(), json!(other));
    }
    json!({
        "track_id": t["id"], "disc_number": t["disc_number"], "track_number": t["track_number"],
        "title": t["title"], "version": t["version"], "isrc": t["isrc"],
        "explicit": t["parental_advisory"], "has_lyrics": t["lyrics"].as_str().is_some_and(|l| !l.trim().is_empty()),
        "audio_attached": t["asset_id"].is_string(), "asset_id": t["asset_id"],
        "credits": credits,
    })
}

fn release_summary(s: &AppState, r: &Value, names: &HashMap<Uuid, String>) -> Value {
    let d = &r["draft"];
    let id = r["id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    let notes = d["notes_lines"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .or_else(|| d["notes"].as_str().map(str::to_owned));
    json!({
        "release_id": r["id"], "title": r["title"], "release_type": r["release_type"],
        "status": r["status"], "row_version": r["row_version"],
        "artist_name": d["artist"], "language": d["language"], "genre": d["genre"], "label": d["label"],
        "release_date": d["release_date"], "release_time": d["release_time"], "preorder_start": d["preorder_start"],
        "territories": d["territories"], "platforms": d["platforms"],
        "p_line": d["p_line"], "c_line": d["c_line"], "notes": notes, "upc": r["upc"],
        "artwork_attached": r["artwork_asset_id"].is_string(),
        "tracks": r["tracks"].as_array().into_iter().flatten().map(|t| track_view(t, names, d)).collect::<Vec<_>>(),
        "agreement_status": r["agreement_status"],
        "delivery_status_by_dsp": r["delivery_status_by_dsp"],
        "live_status_by_dsp": r["live_status_by_dsp"],
        "studio_url": studio_url(s, id),
    })
}

async fn release_view(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> std::result::Result<Value, ToolError> {
    let (r, names) = load(s, a, org, release).await?;
    let mut v = release_summary(s, &r, &names);
    v["view"] = json!("release");
    Ok(v)
}

/// Draft fields from tool arguments into the Studio's draft keys.
fn apply_fields(draft: &mut Value, args: &Value) {
    if !draft.is_object() {
        *draft = json!({});
    }
    let set = |d: &mut Value, k: &str, v: Value| {
        d[k] = v;
    };
    for (arg, key) in [
        ("artist_name", "artist"),
        ("language", "language"),
        ("genre", "genre"),
        ("label", "label"),
        ("release_date", "release_date"),
        ("release_time", "release_time"),
        ("preorder_start", "preorder_start"),
    ] {
        if let Some(v) = str_arg(args, arg) {
            set(draft, key, json!(v.trim()));
        }
    }
    for key in ["territories", "platforms"] {
        if let Some(v) = args.get(key) {
            set(draft, key, v.clone());
        }
    }
    // The Studio shows phonogram/copyright; Stage 1 and DDEX read p_line/c_line.
    if let Some(v) = str_arg(args, "p_line") {
        set(draft, "p_line", json!(v.trim()));
        set(draft, "phonogram", json!(v.trim()));
    }
    if let Some(v) = str_arg(args, "c_line") {
        set(draft, "c_line", json!(v.trim()));
        set(draft, "copyright", json!(v.trim()));
    }
    if let Some(v) = str_arg(args, "notes") {
        set(draft, "notes_lines", json!(v.lines().collect::<Vec<_>>()));
    }
    if let Some(t) = str_arg(args, "release_type") {
        set(draft, "type", json!(t.to_ascii_lowercase()));
    }
    set(
        draft,
        "saved_at",
        json!(chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string()),
    );
}

/// Same release, new draft/title/type/UPC. Keeps cover art and UPC unless
/// changed; optimistic locking on the version just read.
pub(super) async fn write_release(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    r: &Value,
    draft: Value,
    args: &Value,
) -> Result<Value> {
    let upc = match str_arg(args, "upc") {
        Some(u) => Some(u.to_string()),
        None => r["upc"].as_str().map(str::to_owned),
    };
    let input = catalog::Input {
        name: str_arg(args, "title")
            .unwrap_or(r["title"].as_str().unwrap_or(""))
            .trim()
            .to_string(),
        profile: draft,
        party_id: None,
        label_id: None,
        release_type: Some(
            str_arg(args, "release_type")
                .unwrap_or(r["release_type"].as_str().unwrap_or("SINGLE"))
                .to_string(),
        ),
        row_version: r["row_version"].as_i64(),
        upc,
        artwork_asset_id: str_arg(args, "artwork_asset_id")
            .or(r["artwork_asset_id"].as_str())
            .and_then(|v| v.parse().ok()),
    };
    catalog::update(s, a, org, Kind::Release, release, input).await
}

/// Mirror the server tracks and credits into the draft's `draftTracks`, the
/// Studio editor's working copy, so the Studio reopens (and re-saves) the
/// same tracks and credits. Studio-only fields of existing entries are kept.
/// `given`: credit names just set for one track (tool arguments), whose
/// order wins over the previous entry's.
fn sync_draft_tracks(
    draft: &mut Value,
    r: &Value,
    names: &HashMap<Uuid, String>,
    given: Option<(Uuid, &Value)>,
) {
    let old: Vec<Value> = draft["draftTracks"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for t in r["tracks"].as_array().into_iter().flatten() {
        let id = t["id"].as_str().unwrap_or_default();
        let mut e = old
            .iter()
            .find(|o| o["serverId"].as_str() == Some(id))
            .cloned()
            .unwrap_or_else(|| json!({"id": id, "performers": "", "audioName": "", "audioSize": 0, "duration": ""}));
        e["serverId"] = json!(id);
        e["title"] = t["title"].clone();
        e["version"] = json!(t["version"].as_str().unwrap_or(""));
        e["isrc"] = json!(t["isrc"].as_str().unwrap_or(""));
        e["explicit"] = json!(t["parental_advisory"].as_bool().unwrap_or(false));
        e["lyrics"] = json!(t["lyrics"].as_str().unwrap_or(""));
        e["assetId"] = json!(t["asset_id"].as_str().unwrap_or(""));
        for (key, role, field) in CREDIT_ROLES {
            let given_order = given
                .filter(|(track, _)| track.to_string() == id)
                .and_then(|(_, args)| args.get(key)?.as_array().cloned())
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                });
            let order = given_order.or_else(|| e[field].as_str().map(str::to_owned));
            e[field] = json!(credit_names(t, names, role, order.as_deref()).join(", "));
        }
        out.push(e);
    }
    // Blank rows the artist started in the Studio (no server track yet).
    out.extend(old.into_iter().filter(|o| {
        o["serverId"].as_str().is_none_or(str::is_empty)
            && o["title"].as_str().is_none_or(|t| t.trim().is_empty())
    }));
    draft["draftTracks"] = json!(out);
}

/// Re-reads the release and rewrites its draft (one retry when a concurrent
/// edit bumped the version in between).
const RESYNC_ATTEMPTS: usize = 8;

/// Waits before retrying a write that lost a version race: growing, with
/// jitter so racing calls do not collide again in lock step.
async fn backoff(attempt: usize) {
    let jitter = (Uuid::new_v4().as_u128() % 40) as u64;
    tokio::time::sleep(std::time::Duration::from_millis(
        15 * attempt as u64 + jitter,
    ))
    .await;
}

/// After the track or credit write committed: a draft re-sync that still
/// loses to parallel calls does not fail the call (the write is saved; the
/// next call or the Studio rebuilds the draft's track list from the rows).
async fn resync_after_write(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    given: Option<(Uuid, &Value)>,
) -> Result<bool> {
    match resync(s, a, org, release, given).await {
        Ok(()) => Ok(true),
        Err(Error::Conflict) => {
            tracing::info!(%release, "mcp draft resync deferred after parallel writes");
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

async fn resync(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    given: Option<(Uuid, &Value)>,
) -> Result<()> {
    // Parallel track calls on one release (assistants issue them together)
    // each bump the release version; retry on the latest one a few times.
    for attempt in 0..RESYNC_ATTEMPTS {
        let (r, names) = load(s, a, org, release).await?;
        let mut draft = r["draft"].clone();
        sync_draft_tracks(&mut draft, &r, &names, given);
        match write_release(s, a, org, release, &r, draft, &json!({})).await {
            Err(Error::Conflict) if attempt + 1 < RESYNC_ATTEMPTS => {
                backoff(attempt + 1).await;
                continue;
            }
            other => return other.map(|_| ()),
        }
    }
    Ok(())
}

async fn ensure_artist(s: &AppState, a: &Actor, org: Uuid, name: &str) -> Result<Uuid> {
    let name = name.trim();
    let mut after = None;
    for _ in 0..20 {
        let page = catalog::list(
            s,
            a,
            org,
            Kind::Artist,
            catalog::Page {
                after,
                limit: Some(100),
            },
        )
        .await?;
        for item in page["items"].as_array().into_iter().flatten() {
            if item["name"].as_str() == Some(name)
                && let Some(id) = item["id"].as_str().and_then(|v| v.parse().ok())
            {
                return Ok(id);
            }
        }
        match page["next_cursor"].as_str().and_then(|v| v.parse().ok()) {
            Some(c) => after = Some(c),
            None => break,
        }
    }
    let created = catalog::create(
        s,
        a,
        org,
        Kind::Artist,
        catalog::Input {
            name: name.to_string(),
            profile: json!({"app": "audeniq-mcp"}),
            party_id: None,
            label_id: None,
            release_type: None,
            row_version: None,
            upc: None,
            artwork_asset_id: None,
        },
    )
    .await?;
    created["id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Internal)
}

async fn create_draft(
    s: &AppState,
    grant: Uuid,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let mut draft = json!({"app": "audeniq-mcp", "schema": 1, "draftTracks": []});
    apply_fields(&mut draft, args);
    if draft.get("language").is_none() {
        draft["language"] = json!("ko");
    }
    let artist = str_arg(args, "artist_name").unwrap_or("").to_string();
    // What catalog::create would refuse is checked before the artist is
    // created, so a refusal leaves no orphan artist behind.
    crate::text_policy::check(str_arg(args, "title").unwrap_or("").trim())?;
    if let Some(upc) = str_arg(args, "upc").filter(|v| !v.is_empty()) {
        crate::identifiers::validate_upc(upc)?;
    }
    // The artist must be creatable (protected names, text policy) before the
    // release exists, so a refusal leaves nothing behind.
    ensure_artist(s, a, org, &artist).await?;
    let created = catalog::create(
        s,
        a,
        org,
        Kind::Release,
        catalog::Input {
            name: str_arg(args, "title").unwrap_or("").trim().to_string(),
            profile: draft,
            party_id: None,
            label_id: None,
            release_type: str_arg(args, "release_type").map(str::to_owned),
            row_version: None,
            upc: str_arg(args, "upc").map(str::to_owned),
            artwork_asset_id: None,
        },
    )
    .await?;
    let id: Uuid = created["id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Internal)?;
    record_created(s, grant, "create_release_draft", args, id).await;
    let mut v = release_view(s, a, org, id).await?;
    v["next_steps"] = json!([
        "Add tracks with upsert_track and credits with set_track_credits.",
        format!(
            "Add the audio for each track and the cover art: start_upload or import_file_from_url, or upload in AUDENIQ Studio: {}",
            studio_edit_url(s, id)
        ),
        "Run validate_release to see what is still missing.",
    ]);
    Ok(v)
}

async fn update_draft(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let (r, _) = load(s, a, org, release).await?;
    if !submission::is_editable_status(r["status"].as_str().unwrap_or("")) {
        return Err(tool_error(
            "RELEASE_NOT_EDITABLE",
            "This release is under review and cannot be edited. Use get_submission_status.",
        ));
    }
    let mut draft = r["draft"].clone();
    apply_fields(&mut draft, args);
    if let Some(cover) = str_arg(args, "artwork_asset_id") {
        // The Studio editor keeps the cover in its working copy; point it
        // at the new image and drop the old preview, or its next save
        // would put the previous cover back.
        draft["coverAssetId"] = json!(cover);
        draft["coverData"] = json!("");
        if let Some(d) = draft.as_object_mut() {
            d.remove("coverThumbAssetId");
            d.remove("coverPreviewAssetId");
        }
    }
    write_release(s, a, org, release, &r, draft, args).await?;
    if let Some(name) = str_arg(args, "artist_name") {
        ensure_artist(s, a, org, name).await?;
    }
    release_view(s, a, org, release).await
}

async fn upsert_track(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let (r, _) = load(s, a, org, release).await?;
    if !submission::is_editable_status(r["status"].as_str().unwrap_or("")) {
        return Err(tool_error(
            "RELEASE_NOT_EDITABLE",
            "This release is under review and cannot be edited.",
        ));
    }
    let artist = r["draft"]["artist"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            tool_error(
                "ARTIST_REQUIRED",
                "Set the release's artist_name first (update_release_draft).",
            )
        })?;
    let artist_id = ensure_artist(s, a, org, artist).await?;
    let track_id = args
        .get("track_id")
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<Uuid>().ok());
    let current = track_id.and_then(|id| {
        r["tracks"]
            .as_array()?
            .iter()
            .find(|t| t["id"].as_str() == Some(id.to_string().as_str()))
            .cloned()
    });
    if track_id.is_some() && current.is_none() {
        return Err(Error::NotFound.into());
    }
    let cur = current.clone().unwrap_or(Value::Null);
    let pick = |key: &str, cur_key: &str| -> Option<String> {
        str_arg(args, key)
            .map(str::to_owned)
            .or_else(|| cur[cur_key].as_str().map(str::to_owned))
    };
    let input = catalog::TrackInput {
        title: str_arg(args, "title").unwrap_or("").trim().to_string(),
        disc_number: args
            .get("disc_number")
            .and_then(Value::as_i64)
            .or(cur["disc_number"].as_i64())
            .unwrap_or(1) as i32,
        track_number: args
            .get("track_number")
            .and_then(Value::as_i64)
            .unwrap_or(1) as i32,
        artist_id,
        asset_id: args
            .get("asset_id")
            .and_then(Value::as_str)
            .and_then(|v| v.parse().ok())
            .or_else(|| cur["asset_id"].as_str().and_then(|v| v.parse().ok())),
        row_version: r["row_version"].as_i64().unwrap_or(0),
        lyrics: pick("lyrics", "lyrics").filter(|l| !l.trim().is_empty()),
        parental_advisory: args
            .get("explicit")
            .and_then(Value::as_bool)
            .or(cur["parental_advisory"].as_bool()),
        version: pick("version", "version"),
        isrc: pick("isrc", "isrc"),
    };
    // The draft save checks lyrics against protected names; check before
    // writing so a refusal leaves nothing half-saved.
    if let Some(lyrics) = input.lyrics.as_deref() {
        let mut c = s.pool.acquire().await.map_err(Error::from)?;
        crate::protected_names::enforce(&mut c, org, &[lyrics]).await?;
    }
    let (disc, number) = (input.disc_number, input.track_number);
    // Parallel calls on one release race on its version; a write that lost
    // the race saved nothing, so it retries on the latest version (MCP never
    // shows the artist a version to conflict with).
    let mut input = input;
    let mut attempt = 0;
    let saved = loop {
        let result = match track_id {
            Some(t) => drafts::replace_track(s, a, org, release, t, input.clone()).await,
            None => catalog::track(s, a, org, release, input.clone()).await,
        };
        match result {
            Ok(v) => break v,
            Err(Error::Conflict) if attempt + 1 < RESYNC_ATTEMPTS => {
                attempt += 1;
                backoff(attempt).await;
                let (r, _) = load(s, a, org, release).await?;
                input.row_version = r["row_version"].as_i64().unwrap_or(0);
            }
            Err(Error::Database(sqlx::Error::Database(d)))
                if track_id.is_none() && d.constraint() == Some("tracks_active_position") =>
            {
                return Err(position_taken(s, a, org, release, disc, number).await);
            }
            Err(e) => return Err(e.into()),
        }
    };
    let synced = resync_after_write(s, a, org, release, None).await?;
    let id = saved["id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| track_id.map(|t| t.to_string()));
    let mut v = release_view(s, a, org, release).await?;
    v["track_id"] = json!(id);
    if !synced {
        v["draft_sync_pending"] = json!(true);
    }
    Ok(v)
}

/// A new track at a position another track already holds (often a retry of
/// a call that did save): name that track so the assistant edits it.
async fn position_taken(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    disc: i32,
    number: i32,
) -> ToolError {
    let existing = load(s, a, org, release).await.ok().and_then(|(r, _)| {
        r["tracks"].as_array()?.iter().find_map(|t| {
            (t["disc_number"].as_i64() == Some(disc as i64)
                && t["track_number"].as_i64() == Some(number as i64))
            .then(|| t["id"].clone())
        })
    });
    let mut e = tool_error(
        "TRACK_POSITION_TAKEN",
        "A track already exists at this disc and track number (perhaps an earlier call saved it). Update it with its track_id instead of adding it again.",
    );
    e.details = Some(json!({"disc_number": disc, "track_number": number, "track_id": existing}));
    e
}

async fn set_credits(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let track = uuid_arg(args, "track_id")?;
    let mut credits = Vec::new();
    for (key, role, _) in CREDIT_ROLES {
        for name in args
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (party, _) = catalog::ensure_party(s, a, org, name).await?;
            if !credits
                .iter()
                .any(|c: &drafts::Credit| c.party_id == party && c.role == role)
            {
                credits.push(drafts::Credit {
                    party_id: party,
                    role: role.to_string(),
                });
            }
        }
    }
    // Same retry on the latest version as upsert_track.
    let mut attempt = 0;
    loop {
        let (r, _) = load(s, a, org, release).await?;
        let input = drafts::CreditsInput {
            row_version: r["row_version"].as_i64().unwrap_or(0),
            credits: credits.clone(),
        };
        match drafts::replace_credits(s, a, org, release, track, input).await {
            Ok(_) => break,
            Err(Error::Conflict) if attempt + 1 < RESYNC_ATTEMPTS => {
                attempt += 1;
                backoff(attempt).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let synced = resync_after_write(s, a, org, release, Some((track, args))).await?;
    let mut v = release_view(s, a, org, release).await?;
    if !synced {
        v["draft_sync_pending"] = json!(true);
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Validation, preview, submission, status
// ---------------------------------------------------------------------------

fn issue(code: &str, source: &str, track: Option<&Value>, extra: Option<&str>) -> Value {
    json!({
        "code": code,
        "source": source,
        "message": crate::error::message(code).or_else(|| mcp_message(code)).or_else(|| check_hint(code)).or(extra),
        "track_id": track,
    })
}

/// What an artist does about a pre-submission finding.
fn check_hint(code: &str) -> Option<&'static str> {
    Some(match code {
        "TRACK_REQUIRED" => "Add at least one track (upsert_track).",
        "AUDIO_REQUIRED" | "ASSET_MISSING" => "Upload this track's audio in AUDENIQ Studio.",
        "AUDIO_NOT_REGISTERED" | "AUDIO_NOT_ADMITTED" => {
            "This track's audio upload has not finished or was refused; upload it again in AUDENIQ Studio."
        }
        "AUDIO_QC_PENDING_OR_BLOCKED" => {
            "Audio QC has not passed for this track yet; see the qc section."
        }
        "NOT_DRAFT" | "RELEASE_NOT_SUBMITTABLE" => {
            "The release is already submitted or under review."
        }
        "FIELD_TITLE_MISSING" => "Set the release title.",
        "FIELD_RELEASE_DATE_MISSING" => "Set release_date (YYYY-MM-DD).",
        "FIELD_RELEASE_DATE_INVALID" => "release_date must be YYYY-MM-DD.",
        "PLINE_MISSING" => "Set p_line (year and sound recording owner).",
        "CLINE_MISSING" => "Set c_line (year and composition/artwork owner).",
        "FIELD_LANGUAGE_INVALID" => "language must be an ISO 639 code such as ko or en.",
        "TRACK_WRITER_CREDIT_MISSING" | "CREDIT_MISSING" => {
            "Add at least one composer or lyricist (set_track_credits)."
        }
        "FIELD_RELEASE_TYPE_MISMATCH" => {
            "The track count does not fit the release type (SINGLE 1, EP 2-6, ALBUM 7+)."
        }
        "TRACK_ORDER_GAP" => "Number tracks 1..n on each disc without gaps.",
        "TRACK_TITLE_DUPLICATE" => "Two tracks share the same title and version.",
        "ISRC_FORMAT_INVALID" | "ISRC_DUPLICATE" => {
            "Fix the track ISRC (or leave it empty to get one issued)."
        }
        "UPC_FORMAT_INVALID" => "Fix the UPC (or leave it empty to get one issued).",
        "TRACK_TITLE_HAS_VERSION_INFO" => {
            "Move version text (Remaster, Radio Edit...) from the title to the version field."
        }
        "TRACK_TITLE_EXPLICIT_MARKER" | "RELEASE_TITLE_STYLE" | "TRACK_TITLE_STYLE" => {
            "Remove explicit markers, all caps, emoji, 'feat.' or promotional text from titles."
        }
        "TRACK_TITLE_SEO_SPAM" => "Titles look like search keywords; a reviewer will check them.",
        "ADULT_MARKING_REVIEW" => {
            "Explicit content is reviewed for platform explicit flags and Korean 19+ marking."
        }
        _ => return None,
    })
}

async fn validate(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    declarations: &DeclarationsInput,
) -> std::result::Result<Value, ToolError> {
    let (r, names) = load(s, a, org, release).await?;
    let pre = drafts::preflight(s, a, org, release).await?;
    let gates = submission::presubmit(s, a, org, release).await?;
    let review = submission::draft_review(s, a, org, release, declarations).await?;
    let mut blocking = Vec::new();
    let mut review_items = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut push = |list: &mut Vec<Value>, v: Value| {
        let key = format!("{}:{}", v["code"], v["track_id"]);
        if seen.insert(key) {
            list.push(v);
        }
    };
    // Audio QC per attached track (the upload-time checks the Studio shows).
    let mut qc = Vec::new();
    let mut qc_status: HashMap<String, String> = HashMap::new();
    for t in r["tracks"].as_array().into_iter().flatten() {
        let Some(asset) = t["asset_id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else {
            continue;
        };
        let checks = crate::uploads::checks(s, a, org, asset).await?;
        let problems: Vec<&Value> = checks["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| !matches!(c["status"].as_str(), Some("PASS" | "NOT_APPLICABLE")))
            .collect();
        qc_status.insert(
            t["id"].as_str().unwrap_or_default().to_string(),
            checks["qc_status"].as_str().unwrap_or("").to_string(),
        );
        qc.push(json!({"track_id": t["id"], "title": t["title"], "qc_status": checks["qc_status"], "findings": problems}));
    }
    for i in pre["issues"].as_array().into_iter().flatten() {
        let code = i["code"].as_str().unwrap_or("");
        let track = (i["resource_id"] != r["id"]).then_some(&i["resource_id"]);
        // Like the Studio: audio still being analyzed does not stop a
        // submission (Stage 1 analyzes it); a failed analysis does.
        if code == "AUDIO_QC_PENDING_OR_BLOCKED" {
            let blocked = track
                .and_then(Value::as_str)
                .and_then(|t| qc_status.get(t))
                .is_some_and(|q| q == "BLOCKED");
            if blocked {
                push(
                    &mut blocking,
                    issue("AUDIO_QC_BLOCKED", "audio_qc", track, None),
                );
            } else {
                push(
                    &mut review_items,
                    issue("AUDIO_QC_PENDING", "audio_qc", track, None),
                );
            }
            continue;
        }
        push(&mut blocking, issue(code, "preflight", track, None));
    }
    for g in gates["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        push(&mut blocking, issue(g, "account_and_assets", None, None));
    }
    for f in review["field_findings"].as_array().into_iter().flatten() {
        let code = f["check_code"].as_str().unwrap_or("");
        let detail = f["detail"].as_str().unwrap_or("");
        let track = detail
            .split_whitespace()
            .find_map(|w| w.strip_prefix("track="))
            .map(|t| json!(t));
        let v = issue(code, "metadata", track.as_ref(), None);
        match f["status"].as_str() {
            Some("REVIEW_REQUIRED") => push(&mut review_items, v),
            _ => push(&mut blocking, v),
        }
    }
    if let Some(c) = review["submit_check"].as_object() {
        let code = c.get("code").and_then(Value::as_str).unwrap_or("");
        push(&mut blocking, issue(code, "submission_rules", None, None));
    }
    let ready = blocking.is_empty();
    let edit = studio_edit_url(s, release);
    Ok(json!({
        "view": "validation",
        "release": release_summary(s, &r, &names),
        "ready_to_submit": ready,
        "blocking": blocking,
        "will_be_reviewed": review_items,
        "qc": qc,
        "missing_files": {
            "tracks_without_audio": r["tracks"].as_array().into_iter().flatten()
                .filter(|t| !t["asset_id"].is_string()).map(|t| json!({"track_id": t["id"], "title": t["title"]})).collect::<Vec<_>>(),
            "artwork": !r["artwork_asset_id"].is_string(),
            "upload_url": edit,
        },
        "scope_hash": review["scope_hash"],
    }))
}

pub(super) fn declarations_from(v: &Value) -> DeclarationsInput {
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    DeclarationsInput {
        rights_confirmed: b("rights_confirmed"),
        adult_confirmed: b("adult_confirmed"),
        is_cover: b("is_cover"),
        is_remix: b("is_remix"),
        contains_samples: b("contains_samples"),
        ai_involved: b("ai_involved"),
        explicit_content: b("explicit_content"),
    }
}
fn declarations_json(d: &DeclarationsInput) -> Value {
    json!({
        "rights_confirmed": d.rights_confirmed, "adult_confirmed": d.adult_confirmed,
        "is_cover": d.is_cover, "is_remix": d.is_remix, "contains_samples": d.contains_samples,
        "ai_involved": d.ai_involved, "explicit_content": d.explicit_content,
    })
}

async fn preview(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let d = declarations_from(&args["declarations"]);
    let mut v = Box::pin(validate(s, &p.actor, p.org, release, &d)).await?;
    let mut blocking = v["blocking"].as_array().cloned().unwrap_or_default();
    if !d.rights_confirmed || !d.adult_confirmed {
        blocking.push(issue("DECLARATION_REQUIRED", "declarations", None, None));
    }
    let ready = blocking.is_empty();
    let decl = declarations_json(&d);
    v["view"] = json!("preview");
    v["blocking"] = json!(blocking);
    v["ready_to_submit"] = json!(ready);
    v["declarations"] = decl.clone();
    v["special_flags"] = json!(d.special_flags());
    v["what_happens_next"] = json!([
        "AUDENIQ runs automatic checks (metadata, audio QC, rights) and may ask for corrections.",
        "Staff review the application; flagged items (cover, samples, AI, explicit) get a human review.",
        "The artist signs the release application and distribution agreement in AUDENIQ Studio. Delivery to platforms starts only after the signed agreement is approved.",
    ]);
    if ready && (p.has(SCOPE_SUBMIT) || p.has(SCOPE_WRITE)) {
        let exp = chrono::Utc::now().timestamp() + CONFIRM_SECONDS;
        let token = super::seal(
            s,
            "submit-confirmation",
            &json!({
                "g": p.grant, "r": release, "h": v["scope_hash"],
                "d": crate::domain::sha256_json(&decl), "n": crate::auth::random_token()[..24].to_string(), "exp": exp,
            }),
        );
        v["confirmation_token"] = json!(token);
        v["confirmation_expires_at"] =
            json!(chrono::DateTime::from_timestamp(exp, 0).map(|t| t.to_rfc3339()));
        v["instruction"] = json!(if p.has(SCOPE_SUBMIT) {
            "Show this preview to the artist. Call submit_release only after they explicitly confirm this exact content and these declarations."
        } else {
            "Show this preview to the artist. Call submit_release only after they explicitly confirm this exact content and these declarations. This connection has no submit permission, so the application then waits for the artist's one-time approval in AUDENIQ Studio (or ask for releases:submit with request_permission)."
        });
    } else if ready {
        v["instruction"] = json!(format!(
            "Ready, but this connection may not submit. The artist can submit in AUDENIQ Studio ({}), or ask for releases:write or releases:submit with request_permission.",
            studio_url(s, release)
        ));
    }
    if let Some(o) = v.as_object_mut() {
        o.remove("scope_hash");
    }
    Ok(v)
}

async fn submit(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let release = release_arg(args)?;
    let stale = || {
        tool_error(
            "CONFIRMATION_INVALID",
            "The confirmation is missing, expired, for another release or for different declarations. Call preview_submission again and have the artist confirm the new preview.",
        )
    };
    let token = super::open(
        s,
        "submit-confirmation",
        str_arg(args, "confirmation_token").unwrap_or(""),
    )
    .ok_or_else(stale)?;
    let d = declarations_from(&args["declarations"]);
    if token["g"].as_str() != Some(p.grant.to_string().as_str())
        || token["r"].as_str() != Some(release.to_string().as_str())
        || token["d"].as_str() != Some(crate::domain::sha256_json(&declarations_json(&d)).as_str())
    {
        return Err(stale());
    }
    if args["artist_confirmed"] != true {
        return Err(tool_error(
            "CONFIRMATION_REQUIRED",
            "The artist must explicitly confirm the preview first.",
        ));
    }
    let nonce = token["n"].as_str().ok_or_else(stale)?;
    let idempotency_key = format!("mcp:{release}:{nonce}");
    // A retry after a lost response converges before anything else is
    // checked: the release may have moved on (review started) since.
    if let Some(v) = already_submitted(s, p.org, release, &idempotency_key).await? {
        return Ok(v);
    }
    if let Some((id, status)) = super::safety::pending_for(&s.pool, &idempotency_key).await? {
        return Ok(pending_view(s, release, id, &status, true));
    }
    // The content must be what the artist confirmed.
    let review = Box::pin(submission::draft_review(s, &p.actor, p.org, release, &d)).await?;
    if review["scope_hash"] != token["h"] {
        return Err(tool_error(
            "RELEASE_CHANGED",
            "The release changed after the preview. Call preview_submission again and have the artist confirm the new preview.",
        ));
    }
    let title = super::safety::release_title(&s.pool, p.org, release).await;
    // No submit permission: this one application waits for the artist's
    // approval in the Studio (one-time approval of exactly this content).
    let one_time = !p.has(SCOPE_SUBMIT);
    if one_time || super::safety::studio_approval(&s.pool, p.actor.user).await? {
        let (id, new) = super::safety::hold(
            &s.pool,
            super::safety::Pending {
                org: p.org,
                release,
                user: p.actor.user,
                grant: p.grant,
                client_name: &p.client_name,
                declarations: declarations_json(&d),
                scope_hash: review["scope_hash"].as_str().unwrap_or(""),
                idempotency_key: &idempotency_key,
            },
        )
        .await?;
        if new {
            super::safety::notice(
                &s.pool,
                p.org,
                p.actor.user,
                "AI 앱이 보낸 발매 신청을 승인해 주세요",
                &format!(
                    "{}에서 ‘{title}’ 발매 신청을 보냈어요.{} 프로필 → 연결된 AI 앱에서 승인하면 접수돼요. 7일 안에 승인하지 않으면 취소돼요.",
                    p.client_name,
                    if one_time { " 이 연결에는 발매 신청 권한이 없어서 이번 한 번만 승인을 요청했어요." } else { "" }
                ),
                "/profile?open=ai",
            )
            .await;
        }
        return Ok(pending_view(s, release, id, "PENDING", !new));
    }
    let confirmed = token["h"].as_str().unwrap_or("").to_string();
    let result = match submit_now(s, &p.actor, p.org, release, d, idempotency_key, &confirmed).await
    {
        Err(Error::PolicyGate("RELEASE_CHANGED" | "CONSENT_SCOPE_MISMATCH")) => {
            return Err(tool_error(
                "RELEASE_CHANGED",
                "The release changed after the preview. Call preview_submission again and have the artist confirm the new preview.",
            ));
        }
        other => other?,
    };
    if result["deduped"] != true {
        super::safety::notice(
            &s.pool,
            p.org,
            p.actor.user,
            "AI 앱으로 발매를 신청했어요",
            &format!(
                "{}에서 ‘{title}’ 발매 신청을 접수했어요. 내가 한 신청이 아니라면 프로필 → 연결된 AI 앱에서 연결을 끊고 AUDENIQ에 문의해 주세요.",
                p.client_name
            ),
            &format!("/releases/{release}"),
        )
        .await;
    }
    Ok(submitted_view(s, release, &result))
}

/// Records the consent package and submits (the Studio's own path), with
/// the confirmation's idempotency key.
pub(super) async fn submit_now(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    d: DeclarationsInput,
    idempotency_key: String,
    confirmed_scope: &str,
) -> Result<Value> {
    let consent = submission::create_consent(
        s,
        a,
        org,
        release,
        submission::ConsentInput {
            parties: vec![submission::ConsentPartyInput {
                party_id: a.party,
                role: "RIGHTS_HOLDER".into(),
            }],
            minority_declared: false,
            valid_days: Some(365),
        },
    )
    .await?;
    // The consent records the release as it is now: it must still be what
    // the artist confirmed (a write could land between the check and here).
    // submit then refuses any change after the consent (CONSENT_SCOPE_MISMATCH).
    if consent["scope_hash"].as_str() != Some(confirmed_scope) {
        return Err(Error::PolicyGate("RELEASE_CHANGED"));
    }
    let consent_id: Uuid = consent["consent_id"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Internal)?;
    Box::pin(submission::submit(
        s,
        a,
        org,
        release,
        submission::SubmitInput {
            consent_id,
            minority_declared: false,
            idempotency_key,
            declarations: d,
        },
    ))
    .await
}

fn submitted_view(s: &AppState, release: Uuid, result: &Value) -> Value {
    json!({
        "view": "submitted",
        "release_id": release,
        "application_id": result["revision_id"],
        "revision": result["revision"],
        "status": result["status"],
        "duplicate_request": result["deduped"],
        "next_steps": [
            format!("Sign the release application and distribution agreement in AUDENIQ Studio: {}", studio_url(s, release)),
            "AUDENIQ runs automatic checks and staff review; use get_submission_status to follow it.",
        ],
        "studio_url": studio_url(s, release),
    })
}

fn pending_view(s: &AppState, release: Uuid, id: Uuid, status: &str, duplicate: bool) -> Value {
    json!({
        "view": "pending_approval",
        "release_id": release,
        "approval_id": id,
        "approval_status": status,
        "duplicate_request": duplicate,
        "next_steps": [
            format!("The artist asked to approve AI submissions in AUDENIQ Studio. It is not submitted yet: they approve it under Profile → Connected AI apps ({}/profile?open=ai) within 7 days.", super::issuer(s)),
        ],
        "studio_url": format!("{}/profile?open=ai", super::issuer(s)),
    })
}

/// The application a confirmation already created, if any.
async fn already_submitted(
    s: &AppState,
    org: Uuid,
    release: Uuid,
    key: &str,
) -> std::result::Result<Option<Value>, ToolError> {
    let row = sqlx::query(
        "SELECT ar.id, ar.revision, r.status FROM catalog.application_revisions ar
           JOIN catalog.releases r ON r.org_id=ar.org_id AND r.id=ar.release_id
          WHERE ar.org_id=$1 AND ar.release_id=$2 AND ar.idempotency_key=$3",
    )
    .bind(org)
    .bind(release)
    .bind(key)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?;
    Ok(row.map(|r| {
        submitted_view(
            s,
            release,
            &json!({"revision_id": r.get::<Uuid, _>("id"), "revision": r.get::<i32, _>("revision"),
                    "status": r.get::<String, _>("status"), "deduped": true}),
        )
    }))
}

fn status_text(status: &str) -> &'static str {
    match status {
        "DRAFT" => "Draft: not submitted yet.",
        "SUBMITTED" | "STAGE1_RUNNING" => {
            "Submitted: automatic checks (metadata, audio QC) are running."
        }
        "STAGE1_CORRECTION" | "STAGE2_CORRECTION" | "STAGE3_CORRECTION" => {
            "Correction needed: fix the listed items, then submit again."
        }
        "STAGE1_PASSED" | "STAGE2_RUNNING" => {
            "Automatic checks passed; rights and policy review in progress."
        }
        "STAGE2_REVIEW" => "With AUDENIQ staff for review.",
        "STAGE2_PASSED" | "STAGE3_PREPARING" => {
            "Approved; preparing delivery packages and identifiers."
        }
        "READY_FOR_DELIVERY" => {
            "Ready for delivery: sent to platforms once the signed distribution agreement is approved."
        }
        "ON_HOLD_RIGHTS" => "On hold for a rights question; AUDENIQ will contact the artist.",
        "SUPERSEDED" => "Replaced by a newer application.",
        "WITHDRAWN" => "Withdrawn.",
        _ => "See status.",
    }
}

async fn status(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> std::result::Result<Value, ToolError> {
    let st = submission::submission_status(s, a, org, release).await?;
    let (r, _) = load(s, a, org, release).await?;
    let status = st["status"].as_str().unwrap_or("");
    let corrections: Vec<Value> = st["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| matches!(c["severity"].as_str(), Some("CORRECTION" | "HOLD")))
        .map(|c| {
            let code = c["check_code"].as_str().unwrap_or("");
            json!({"code": code, "severity": c["severity"], "message": crate::error::message(code).or_else(|| check_hint(code))})
        })
        .collect();
    let mut follow_up = Vec::new();
    if submission::is_editable_status(status) && status != "DRAFT" {
        follow_up.push(format!(
            "Fix the corrections and submit again (or edit in AUDENIQ Studio: {}).",
            studio_edit_url(s, release)
        ));
    }
    let agreement = r["agreement_status"].as_str();
    if status != "DRAFT" && status != "WITHDRAWN" && agreement != Some("SIGNED") {
        follow_up.push(match agreement {
            None => format!("Sign the release application in AUDENIQ Studio so the distribution agreement can be reviewed: {}", studio_url(s, release)),
            Some("NEEDS") => format!("AUDENIQ asked for documents for the distribution agreement; open AUDENIQ Studio: {}", studio_url(s, release)),
            Some(_) => format!("The distribution agreement is waiting for review or the artist's signature in AUDENIQ Studio: {}", studio_url(s, release)),
        });
    }
    Ok(json!({
        "view": "status",
        "release_id": release,
        "title": r["title"],
        "status": status,
        "status_explanation": status_text(status),
        "application": st["revision"],
        "checks": st["checks"],
        "corrections_required": corrections,
        "review_notes": st["review_notes"],
        "agreement_status": r["agreement_status"],
        "delivery_status_by_dsp": r["delivery_status_by_dsp"],
        "live_status_by_dsp": r["live_status_by_dsp"],
        "follow_up": follow_up,
        "studio_url": studio_url(s, release),
    }))
}

// ---------------------------------------------------------------------------
// Corrections of delivered releases
// ---------------------------------------------------------------------------

async fn corrections_view(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> std::result::Result<Value, ToolError> {
    let mut v = crate::corrections::status(s, a, org, release).await?;
    let review = match v["request"]["status"].as_str() {
        Some("PENDING") => "Waiting for AUDENIQ review.",
        Some("APPROVED") => "Approved; the update was sent to the platforms carrying the release.",
        Some("DECLINED") => "Declined by AUDENIQ; see decision_note.",
        Some("CANCELLED") => "Cancelled by the artist.",
        _ => "No correction requested yet.",
    };
    v["view"] = json!("correction");
    v["release_id"] = json!(release);
    v["review"] = json!(review);
    v["correctable"] = json!(["genre", "track credits except artist and featured artist"]);
    v["locked"] = json!([
        "title",
        "artist and featured-artist names",
        "audio",
        "ISRC/UPC",
        "release date",
        "P/C lines"
    ]);
    Ok(v)
}

async fn request_correction(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    use crate::corrections::{Changes, CreditInput, RequestInput, TrackCredits};
    let release = release_arg(args)?;
    let mut tracks = Vec::new();
    for t in args
        .get("tracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        tracks.push(TrackCredits {
            track_id: uuid_arg(t, "track_id")?,
            credits: t["credits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| CreditInput {
                    name: c["name"].as_str().unwrap_or_default().to_string(),
                    role: c["role"].as_str().unwrap_or_default().to_string(),
                })
                .collect(),
        });
    }
    let input = RequestInput {
        changes: Changes {
            genre: str_arg(args, "genre").map(str::to_owned),
            tracks,
        },
        reason: str_arg(args, "reason").unwrap_or_default().to_string(),
    };
    crate::corrections::request(s, a, org, release, input).await?;
    corrections_view(s, a, org, release).await
}

// ---------------------------------------------------------------------------
// Promotion, codes, notifications, documents
// ---------------------------------------------------------------------------

async fn smart_link(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> std::result::Result<Value, ToolError> {
    let mut v = crate::smart_links::status(s, a, org, release).await?;
    let url = v["link"]["slug"]
        .as_str()
        .map(|slug| format!("{}/l/{slug}", super::issuer(s)));
    v["view"] = json!("smart_link");
    v["release_id"] = json!(release);
    v["url"] = json!(url);
    Ok(v)
}

/// Status and where to act; the legal text, signatures and electronic
/// records stay in Studio.
async fn documents(s: &AppState, a: &Actor, org: Uuid) -> std::result::Result<Value, ToolError> {
    let v = crate::portal::list_documents(s, a, org).await?;
    let base = super::issuer(s);
    let items: Vec<Value> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            let agreement = d["kind"] == "AGREEMENT";
            let id = d["id"].as_str().unwrap_or_default();
            let next = match (agreement, d["status"].as_str()) {
                (_, Some("SIGNED")) => "Done.",
                (true, Some("APPROVED" | "PREPARED")) => "Ready to sign in AUDENIQ Studio.",
                (_, Some("NEEDS")) => "AUDENIQ asked for more; see review_note and answer in AUDENIQ Studio.",
                (_, Some("AWAITING_DOCUMENTS")) => "Upload the requested document in AUDENIQ Studio.",
                (_, Some("REVIEW")) => "AUDENIQ is reviewing it.",
                _ => "See AUDENIQ Studio.",
            };
            json!({
                "document_id": d["id"], "kind": d["kind"], "title": d["title"], "status": d["status"],
                "release_id": d["release_id"], "release_title": d["release_title"], "review_note": d["review_note"],
                "signed_at": d["signed_at"], "updated_at": d["updated_at"], "next": next,
                "studio_url": format!("{base}/{}/{id}", if agreement { "contracts" } else { "rights" }),
            })
        })
        .collect();
    Ok(json!({"view": "documents", "documents": items}))
}

// ---------------------------------------------------------------------------
// Settlement and payouts
// ---------------------------------------------------------------------------

/// Idempotency-key prefix of payouts requested through an assistant: the
/// 24-hour assistant limit counts exactly these.
const PAYOUT_KEY_PREFIX: &str = "mcp:";
const PAYOUT_WINDOW_HOURS: i32 = 24;
/// A payout preview's confirmation is valid this long.
const PAYOUT_CONFIRM_SECONDS: i64 = 10 * 60;

fn settlement_url(s: &AppState) -> String {
    format!("{}/settlement", super::issuer(s))
}

/// Bank and last four digits only: the holder name and number stay in Studio.
fn account_brief(v: &Value) -> Value {
    if v["registered"] != true {
        return json!({"registered": false});
    }
    json!({"registered": true, "bank_name": v["bank_name"], "account_last4": v["account_last4"], "payee_type": v["payee_type"]})
}

async fn assistant_payout_room(s: &AppState, p: &Principal) -> Result<(i64, i64)> {
    let used = crate::portal::payouts_requested_since(
        s,
        &p.actor,
        p.org,
        PAYOUT_KEY_PREFIX,
        PAYOUT_WINDOW_HOURS,
    )
    .await?;
    let used = rust_decimal::prelude::ToPrimitive::to_i64(&used.trunc()).unwrap_or(i64::MAX);
    Ok((used, (s.mcp.payout_limit_krw - used).max(0)))
}

async fn settlement_summary(s: &AppState, p: &Principal) -> std::result::Result<Value, ToolError> {
    let mut v = crate::portal::finance_summary(s, &p.actor, p.org).await?;
    let account = crate::portal::get_payout_account(s, &p.actor, p.org).await?;
    let (used, room) = assistant_payout_room(s, p).await?;
    v["view"] = json!("settlement");
    v["account"] = account_brief(&account);
    v["assistant_payouts"] = json!({
        "limit_krw_per_24h": s.mcp.payout_limit_krw,
        "requested_last_24h": used,
        "remaining_krw": room,
        "allowed_for_this_connection": p.has(SCOPE_PAYOUT),
        "above_limit": format!("Request larger payouts in AUDENIQ Studio: {}", settlement_url(s)),
    });
    Ok(v)
}

async fn statements(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let page = crate::portal::StatementPage {
        before: str_arg(args, "before").map(str::to_owned),
        limit: Some(args.get("limit").and_then(Value::as_i64).unwrap_or(30)),
    };
    let mut v = crate::portal::statements(s, a, org, page).await?;
    v["view"] = json!("statements");
    Ok(v)
}

async fn payouts(s: &AppState, a: &Actor, org: Uuid) -> std::result::Result<Value, ToolError> {
    let mut v = crate::portal::list_payouts(s, a, org).await?;
    for item in v["items"].as_array_mut().into_iter().flatten() {
        if let Some(acc) = item.get_mut("account").and_then(Value::as_object_mut) {
            acc.remove("holder_name");
        }
    }
    v["view"] = json!("payouts");
    Ok(v)
}

async fn preview_payout(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let amount = args.get("amount").and_then(Value::as_i64).unwrap_or(0);
    let ws = catalog::workspace(s, &p.actor, p.org).await?;
    let quote = crate::portal::quote_payout(s, &p.actor, p.org, &amount.to_string()).await?;
    let account = crate::portal::get_payout_account(s, &p.actor, p.org).await?;
    let (used, room) = assistant_payout_room(s, p).await?;
    let limit = s.mcp.payout_limit_krw;
    let blocked = if ws["role"] != "OWNER" {
        Some((
            "PAYOUT_OWNER_ONLY",
            "Only the workspace owner can request payouts.".to_string(),
        ))
    } else if account["registered"] != true {
        Some((
            "PAYOUT_ACCOUNT_REQUIRED",
            mcp_message("PAYOUT_ACCOUNT_REQUIRED")
                .unwrap_or_default()
                .to_string(),
        ))
    } else if quote["within_balance"] != true {
        Some((
            "PAYOUT_EXCEEDS_BALANCE",
            mcp_message("PAYOUT_EXCEEDS_BALANCE")
                .unwrap_or_default()
                .to_string(),
        ))
    } else if limit == 0 || amount > room {
        Some((
            "PAYOUT_USE_STUDIO",
            format!(
                "Through the assistant at most {limit} KRW can be requested per 24 hours ({room} KRW left). Request this amount in AUDENIQ Studio: {}",
                settlement_url(s)
            ),
        ))
    } else {
        None
    };
    let mut v = json!({
        "view": "payout_preview",
        "amount": quote["amount"], "fee": quote["fee"], "net": quote["net"],
        "available": quote["available"], "policy_version": quote["policy_version"],
        "account": account_brief(&account),
        "assistant_limit_krw": limit, "assistant_requested_last_24h": used,
        "allowed": blocked.is_none(),
    });
    match blocked {
        Some((code, message)) => {
            v["blocked_by"] = json!({"code": code, "message": message});
        }
        None => {
            let exp = chrono::Utc::now().timestamp() + PAYOUT_CONFIRM_SECONDS;
            v["confirmation_token"] = json!(super::seal(
                s,
                "payout-confirmation",
                &json!({"g": p.grant, "o": p.org, "a": amount, "rv": account["row_version"],
                        "n": crate::auth::random_token()[..24].to_string(), "exp": exp}),
            ));
            v["confirmation_expires_at"] =
                json!(chrono::DateTime::from_timestamp(exp, 0).map(|t| t.to_rfc3339()));
            v["instruction"] = json!(
                "Show the amount, fee, net amount and account (bank, last 4 digits) to the artist. Call request_payout only after they explicitly confirm."
            );
        }
    }
    Ok(v)
}

async fn request_payout(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let amount = args.get("amount").and_then(Value::as_i64).unwrap_or(0);
    let stale = || {
        tool_error(
            "CONFIRMATION_INVALID",
            "The confirmation is missing, expired or for another amount. Call preview_payout again and have the artist confirm.",
        )
    };
    let token = super::open(
        s,
        "payout-confirmation",
        str_arg(args, "confirmation_token").unwrap_or(""),
    )
    .ok_or_else(stale)?;
    if token["g"].as_str() != Some(p.grant.to_string().as_str())
        || token["o"].as_str() != Some(p.org.to_string().as_str())
        || token["a"].as_i64() != Some(amount)
    {
        return Err(stale());
    }
    if args["artist_confirmed"] != true {
        return Err(tool_error(
            "CONFIRMATION_REQUIRED",
            "The artist must explicitly confirm the payout first.",
        ));
    }
    let account = crate::portal::get_payout_account(s, &p.actor, p.org).await?;
    if account["row_version"] != token["rv"] {
        return Err(tool_error(
            "PAYOUT_ACCOUNT_CHANGED",
            "The payout account changed after the preview. Call preview_payout again.",
        ));
    }
    let nonce = token["n"].as_str().ok_or_else(stale)?;
    let limit = s.mcp.payout_limit_krw;
    if limit == 0 {
        return Err(tool_error(
            "PAYOUT_USE_STUDIO",
            &format!("Request payouts in AUDENIQ Studio: {}", settlement_url(s)),
        ));
    }
    let retried: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.payout_requests WHERE org_id=$1 AND idempotency_key=$2)",
    )
    .bind(p.org)
    .bind(format!("{PAYOUT_KEY_PREFIX}{nonce}"))
    .fetch_one(&s.pool)
    .await
    .map_err(Error::from)?;
    let v = crate::portal::request_payout_capped(
        s,
        &p.actor,
        p.org,
        crate::portal::PayoutRequestInput {
            amount: amount.to_string(),
            idempotency_key: format!("{PAYOUT_KEY_PREFIX}{nonce}"),
        },
        Some(crate::portal::PayoutCap {
            key_prefix: PAYOUT_KEY_PREFIX,
            limit: rust_decimal::Decimal::from(limit),
            hours: PAYOUT_WINDOW_HOURS,
        }),
    )
    .await
    .map_err(|e| match e {
        Error::PolicyGate("PAYOUT_USE_STUDIO") => tool_error(
            "PAYOUT_USE_STUDIO",
            &format!("This would exceed the assistant payout limit of {limit} KRW per 24 hours. Request it in AUDENIQ Studio: {}", settlement_url(s)),
        ),
        e => e.into(),
    })?;
    if !retried {
        super::safety::notice(
        &s.pool,
        p.org,
        p.actor.user,
        "AI 앱으로 지급을 신청했어요",
        &format!(
            "{}에서 {}원 지급을 신청했어요 (수수료 {}원, 받을 금액 {}원). 내가 한 신청이 아니라면 정산 화면에서 바로 취소하고 AUDENIQ에 문의해 주세요.",
            p.client_name,
            v["amount"].as_str().unwrap_or("?"),
            v["fee"].as_str().unwrap_or("?"),
            v["net"].as_str().unwrap_or("?")
        ),
        "/settlement",
    )
    .await;
    }
    Ok(json!({
        "view": "payout",
        "payout_id": v["id"], "amount": v["amount"], "fee": v["fee"], "net": v["net"], "status": v["status"],
        "account": account_brief(&account),
        "next": "AUDENIQ staff check the request, then send the net amount to the registered account. It can be cancelled until it is sent to the bank (cancel_payout).",
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_korean_results_are_cut_on_a_character_boundary() {
        let v = json!({"note": "가".repeat(30_000)});
        let text = summary("get_inquiry", &v);
        assert!(text.ends_with('…'));
        assert!(text.len() <= 60_000 + '…'.len_utf8());
    }
    use super::*;
    #[test]
    fn every_tool_schema_compiles_and_is_strict() {
        for d in defs() {
            let schema = d.input_schema();
            jsonschema::validator_for(&schema).unwrap_or_else(|e| panic!("{}: {e}", d.name));
            assert_eq!(schema["type"], "object", "{}", d.name);
            assert_eq!(schema["additionalProperties"], false, "{}", d.name);
        }
        let names: std::collections::BTreeSet<_> = defs().iter().map(|d| d.name).collect();
        assert_eq!(names.len(), defs().len(), "tool names are unique");
    }
    #[test]
    fn scopes_and_annotations_match_what_each_tool_does() {
        for d in defs() {
            assert_eq!(
                d.scope == SCOPE_SUBMIT,
                d.name == "submit_release",
                "{}",
                d.name
            );
            // Only reads under the release/settlement read scopes; every
            // write needs a write-type scope.
            // request_permission only files a request the artist answers
            // in the Studio; every connection may make one.
            if (d.scope == SCOPE_READ || d.scope == SCOPE_SETTLEMENT)
                && d.name != "request_permission"
            {
                assert!(d.read_only, "{}", d.name);
            }
            if d.scope == SCOPE_WRITE || d.scope == SCOPE_SUBMIT {
                assert!(!d.read_only, "{}", d.name);
            }
            // Moving money, submitting a release (declarations recorded
            // as the artist's) and deleting a draft (cannot be undone) are
            // what clients must confirm with the user.
            assert_eq!(
                d.destructive,
                matches!(
                    d.name,
                    "request_payout" | "submit_release" | "delete_release_draft"
                ),
                "{}",
                d.name
            );
        }
    }
    #[test]
    fn schemas_reject_unknown_fields_and_bad_values() {
        let check = |name: &str, args: Value| validator(find(name).unwrap()).is_valid(&args);
        let id = Uuid::new_v4().to_string();
        assert!(check("get_release", json!({"release_id": id})));
        assert!(!check(
            "get_release",
            json!({"release_id": "1; DROP TABLE"})
        ));
        assert!(!check(
            "get_release",
            json!({"release_id": id, "org_id": id})
        ));
        assert!(!check(
            "update_release_draft",
            json!({"release_id": id, "release_date": "next friday"})
        ));
        assert!(!check(
            "update_release_draft",
            json!({"release_id": id, "territories": ["WORLD", "jp"]})
        ));
        assert!(check(
            "update_release_draft",
            json!({"release_id": id, "territories": ["WORLD", "-JP"], "release_time": "18:00"})
        ));
        assert!(!check(
            "set_track_credits",
            json!({"release_id": id, "track_id": id, "composers": ["A, B"]})
        ));
        assert!(!check(
            "submit_release",
            json!({"release_id": id, "confirmation_token": "x".repeat(30),
            "declarations": declarations_json(&DeclarationsInput::default()), "artist_confirmed": false})
        ));
    }
    #[test]
    fn draft_fields_land_on_the_studio_keys() {
        let mut d = json!({"coverAssetId": "keep", "draftTracks": []});
        apply_fields(
            &mut d,
            &json!({"artist_name": " Iu ", "p_line": "2026 X", "notes": "a\nb", "release_type": "EP", "platforms": ["spotify"]}),
        );
        assert_eq!(d["artist"], "Iu");
        assert_eq!(d["phonogram"], "2026 X");
        assert_eq!(d["p_line"], "2026 X");
        assert_eq!(d["notes_lines"], json!(["a", "b"]));
        assert_eq!(d["type"], "ep");
        assert_eq!(d["coverAssetId"], "keep");
    }
    #[test]
    fn draft_tracks_mirror_server_tracks_and_keep_studio_fields() {
        let party = Uuid::new_v4();
        let names = HashMap::from([(party, "Kim".to_string())]);
        let mut d = json!({"draftTracks": [
            {"id": "t1", "serverId": "s1", "title": "Old", "audioName": "song.wav", "previewStart": "30"},
            {"id": "gone", "serverId": "s9", "title": "Deleted"},
            {"id": "blank", "title": ""}]});
        let r = json!({"tracks": [{"id": "s1", "title": "New", "version": "", "isrc": null, "parental_advisory": true,
            "lyrics": null, "asset_id": "a1", "credits": [{"party_id": party.to_string(), "role": "Composer"}]}]});
        sync_draft_tracks(&mut d, &r, &names, None);
        let t = &d["draftTracks"];
        assert_eq!(t.as_array().unwrap().len(), 2);
        assert_eq!(t[0]["title"], "New");
        assert_eq!(t[0]["audioName"], "song.wav");
        assert_eq!(t[0]["previewStart"], "30");
        assert_eq!(t[0]["composers"], "Kim");
        assert_eq!(t[0]["explicit"], true);
        assert_eq!(t[1]["id"], "blank");
    }
    #[test]
    fn widget_resource_is_served_and_self_contained() {
        let r = read_resource(WIDGET_URI).unwrap();
        let html = r["text"].as_str().unwrap();
        assert!(html.contains("textContent"));
        assert!(
            !html.contains("innerHTML"),
            "untrusted metadata must never be parsed as HTML"
        );
        assert!(!html.contains("<script src"), "no external scripts");
        assert!(read_resource("ui://other").is_none());
    }
}
