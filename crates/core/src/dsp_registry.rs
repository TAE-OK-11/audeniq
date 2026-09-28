//! DSP registry: every delivery target Studio offers, addressed by a stable
//! internal code (`D-1` .. `D-11`) instead of its commercial name.
//!
//! Call sites use the [`Dsp`] enum (`Dsp::D5`), never a name string, so a
//! typo is a compile error and renaming a platform touches one table. The
//! code is also the adapter `partner_id` for the DSP's direct route and the
//! seed of its internal UUID (`uuid_v5("audeniq:dsp:D-5")`, the same scheme
//! `partner_onboarding::set_dsp` uses), so the registry, the adapter profile,
//! Stage 2 eligibility and the route plan all agree without a lookup.
//!
//! `DspSpec` holds the delivery requirements AUDENIQ checks before a release
//! may be sent. Sources: `DSP_CONDITIONS_RESEARCH_2026-09-25.md`. Only DSPs
//! that publish a DDEX delivery profile are marked `Ddex`; the Korean DSPs
//! publish no distributor spec (partner portals are private), so they carry
//! the common industry baseline and `PartnerSpec` delivery until a contract
//! supplies theirs. Nothing here enables a send: every seeded profile is
//! CONTRACTED + delivery_enabled=false + send_or_publish=false (migration
//! 0042), and the onboarding gate still requires a signed contract.
use crate::ddex_ern::{
    DEAL_ON_DEMAND_SUBSCRIPTION, DEAL_SUBSCRIPTION, DEAL_SUBSCRIPTION_AND_FREE, DealProfile,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Dsp {
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    D7,
    D8,
    D9,
    D10,
    D11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Region {
    /// Korean domestic service (youth-harmful marking is a legal duty).
    Kr,
    Global,
}

/// How the DSP takes deliveries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryFormat {
    /// Publishes a DDEX ERN ingestion profile: AUDENIQ builds ERN 3.8.2.
    Ddex,
    /// No public distributor spec: the format comes with the contract.
    PartnerSpec,
}

/// How the DSP normally ingests from distributors (industry practice;
/// the contract's technical annex is authoritative and the partner config
/// file follows it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Channel {
    /// DDEX ERN over SFTP (Aspera is offered by some as a faster lane).
    Sftp,
    /// iTunes Package via Apple Transporter; DDEX only where the contract
    /// says so.
    Transporter,
    /// The partner's own metadata feed (file drop or API).
    PartnerFeed,
}

/// DDEX ERN choreography profile the DSP expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoreographyProfile {
    Batch,
    ReleaseByRelease,
    /// Not DDEX: the partner feed's own completion convention.
    PartnerFeed,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DspSpec {
    pub dsp: Dsp,
    /// `D-1` .. `D-11`: the only identifier internal call sites use.
    pub code: &'static str,
    /// Studio's platform key (`profile.platforms` values).
    pub slug: &'static str,
    /// Display only (staff screens, audit readability). Never a key.
    pub name: &'static str,
    /// What artists and staff see (Korean brand names for domestic DSPs).
    /// Internal codes never appear in user- or staff-facing text.
    pub name_ko: &'static str,
    /// DDEX ERN version the DSP ingests; empty for partner feeds. The
    /// generator builds 3.8.2; staging refuses a DSP set to anything else.
    pub ern_version: &'static str,
    /// Commercial models / use types the DSP licenses (the ERN deal).
    pub deal: &'static DealProfile,
    pub channel: Channel,
    pub choreography: ChoreographyProfile,
    /// The DSP has a Merlin deal, so the release may go under the Merlin
    /// contract instead of a direct one (per-DSP choice, migration 0056).
    pub merlin_eligible: bool,
    pub region: Region,
    pub format: DeliveryFormat,
    /// Minimum cover side in px (square required everywhere).
    pub artwork_min_px: u32,
    /// Largest cover side the DSP ingests; None = no published cap.
    pub artwork_max_px: Option<u32>,
    pub audio_min_sample_rate: u32,
    pub audio_min_bits: u32,
    /// Only lossless masters (WAV/FLAC) are accepted.
    pub lossless_only: bool,
    /// Days between delivery and release date the DSP needs.
    pub lead_days: i64,
    /// A composer credit per track is required.
    pub requires_composer: bool,
    /// A lyricist credit per track is required (vocal tracks).
    pub requires_lyricist: bool,
    /// Integrated loudness the DSP normalises to (advisory only).
    pub loudness_target_lufs: f32,
    /// DDEX preflight rule ids this DSP treats as deal-breakers.
    pub escalate: &'static [&'static str],
    /// Audio fingerprint rights management (YouTube Content ID): covers,
    /// samples and remixes get claimed or rejected.
    pub content_id: bool,
    /// Covers need proof of license / the original author's consent
    /// (TIDAL; Korean services ask for 원작자 커버 동의서).
    pub cover_license_required: bool,
    /// Published AI-content policy (impersonation, labelling).
    pub ai_policy: bool,
    /// Best quality the DSP streams to listeners (sample rate, bits);
    /// masters above it are delivered but served downsampled.
    pub served_max: (u32, u32),
}

const KR_BASE: DspSpec = DspSpec {
    dsp: Dsp::D1,
    code: "",
    slug: "",
    name: "",
    name_ko: "",
    ern_version: "",
    deal: &DEAL_SUBSCRIPTION,
    channel: Channel::PartnerFeed,
    choreography: ChoreographyProfile::PartnerFeed,
    merlin_eligible: false,
    region: Region::Kr,
    format: DeliveryFormat::PartnerSpec,
    artwork_min_px: 3000,
    artwork_max_px: None,
    audio_min_sample_rate: 44_100,
    audio_min_bits: 16,
    lossless_only: true,
    lead_days: 14,
    requires_composer: true,
    requires_lyricist: true,
    loudness_target_lufs: -14.0,
    escalate: &[],
    content_id: false,
    cover_license_required: true,
    ai_policy: false,
    served_max: (192_000, 24),
};

const GLOBAL_BASE: DspSpec = DspSpec {
    region: Region::Global,
    format: DeliveryFormat::Ddex,
    ern_version: "3.8.2",
    channel: Channel::Sftp,
    choreography: ChoreographyProfile::Batch,
    merlin_eligible: true,
    cover_license_required: false,
    requires_lyricist: false,
    ..KR_BASE
};

/// The registry, in code order. `Dsp as usize` indexes it.
pub const REGISTRY: [DspSpec; 11] = [
    DspSpec {
        dsp: Dsp::D1,
        code: "D-1",
        slug: "melon",
        name_ko: "멜론",
        name: "Melon",
        ..KR_BASE
    },
    DspSpec {
        dsp: Dsp::D2,
        code: "D-2",
        slug: "genie",
        name_ko: "지니",
        name: "Genie",
        ..KR_BASE
    },
    DspSpec {
        dsp: Dsp::D3,
        code: "D-3",
        slug: "flo",
        name_ko: "FLO",
        name: "FLO",
        ..KR_BASE
    },
    DspSpec {
        dsp: Dsp::D4,
        code: "D-4",
        slug: "bugs",
        name_ko: "벅스",
        name: "Bugs",
        ..KR_BASE
    },
    DspSpec {
        dsp: Dsp::D5,
        code: "D-5",
        slug: "spotify",
        ai_policy: true,
        served_max: (44_100, 24),
        name_ko: "Spotify",
        deal: &DEAL_SUBSCRIPTION_AND_FREE,
        name: "Spotify",
        artwork_max_px: Some(10_000),
        lead_days: 7,
        requires_composer: false,
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D6,
        code: "D-6",
        slug: "apple",
        ai_policy: true,
        name_ko: "Apple Music",
        deal: &DEAL_SUBSCRIPTION,
        channel: Channel::Transporter,
        choreography: ChoreographyProfile::ReleaseByRelease,
        name: "Apple Music / iTunes",
        lead_days: 10,
        loudness_target_lufs: -16.0,
        // Apple rejects releases whose type contradicts the track layout.
        escalate: &["DDEX-PREFLIGHT-RELEASE-TYPE"],
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D7,
        code: "D-7",
        slug: "youtube",
        content_id: true,
        // YouTube Music streams lossy (AAC/Opus).
        served_max: (44_100, 16),
        name_ko: "YouTube Music",
        deal: &DEAL_SUBSCRIPTION_AND_FREE,
        name: "YouTube Music",
        lead_days: 7,
        requires_composer: false,
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D8,
        code: "D-8",
        slug: "amazon",
        name_ko: "Amazon Music",
        deal: &DEAL_SUBSCRIPTION_AND_FREE,
        name: "Amazon Music",
        lead_days: 7,
        requires_composer: false,
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D9,
        code: "D-9",
        slug: "tidal",
        cover_license_required: true,
        ai_policy: true,
        name_ko: "TIDAL",
        deal: &DEAL_SUBSCRIPTION,
        name: "TIDAL",
        lead_days: 7,
        requires_composer: false,
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D10,
        code: "D-10",
        slug: "deezer",
        served_max: (44_100, 16),
        name_ko: "Deezer",
        deal: &DEAL_SUBSCRIPTION_AND_FREE,
        name: "Deezer",
        artwork_max_px: Some(4096),
        // Deezer: composer + lyricist per track, delivery two weeks ahead.
        requires_lyricist: true,
        loudness_target_lufs: -15.0,
        ..GLOBAL_BASE
    },
    DspSpec {
        dsp: Dsp::D11,
        code: "D-11",
        slug: "qobuz",
        name_ko: "Qobuz",
        deal: &DEAL_ON_DEMAND_SUBSCRIPTION,
        choreography: ChoreographyProfile::ReleaseByRelease,
        merlin_eligible: false,
        name: "Qobuz",
        requires_composer: false,
        ..GLOBAL_BASE
    },
];

impl Dsp {
    pub const ALL: [Dsp; 11] = [
        Dsp::D1,
        Dsp::D2,
        Dsp::D3,
        Dsp::D4,
        Dsp::D5,
        Dsp::D6,
        Dsp::D7,
        Dsp::D8,
        Dsp::D9,
        Dsp::D10,
        Dsp::D11,
    ];

    pub fn spec(self) -> &'static DspSpec {
        &REGISTRY[self as usize]
    }

    pub fn code(self) -> &'static str {
        self.spec().code
    }

    /// The name artists and staff see.
    pub fn display_name(self) -> &'static str {
        self.spec().name_ko
    }

    /// Internal DSP id used by Stage 2 scope, route plans and profiles.
    pub fn uuid(self) -> Uuid {
        uuid_for_code(self.code())
    }

    pub fn from_code(code: &str) -> Option<Dsp> {
        Dsp::ALL.into_iter().find(|d| d.code() == code)
    }

    pub fn from_slug(slug: &str) -> Option<Dsp> {
        Dsp::ALL.into_iter().find(|d| d.spec().slug == slug)
    }

    pub fn from_uuid(id: Uuid) -> Option<Dsp> {
        Dsp::ALL.into_iter().find(|d| d.uuid() == id)
    }
}

/// Same derivation as `partner_onboarding::set_dsp` without an explicit id.
pub fn uuid_for_code(code: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("audeniq:dsp:{code}").as_bytes(),
    )
}

/// DSPs an application asked for (`release.draft.platforms`, Studio slugs,
/// or `D-n` codes). Unknown values are ignored; duplicates collapse. `None`
/// when the draft names no platforms at all (legacy drafts: every DSP).
pub fn requested(draft: &serde_json::Value) -> Option<Vec<Dsp>> {
    let list = draft.get("platforms")?.as_array()?;
    let mut out: Vec<Dsp> = list
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|v| Dsp::from_slug(v).or_else(|| Dsp::from_code(v)))
        .collect();
    out.sort();
    out.dedup();
    Some(out)
}

/// Registry as JSON for staff screens and the artist delivery view.
pub fn catalog() -> serde_json::Value {
    serde_json::Value::Array(
        REGISTRY
            .iter()
            .map(|s| {
                let mut v = serde_json::to_value(s).expect("spec serializes");
                v["dsp"] = serde_json::Value::String(s.code.into());
                v["dsp_id"] = serde_json::Value::String(s.dsp.uuid().to_string());
                v
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_indexed_by_enum_and_codes_are_unique() {
        for (i, d) in Dsp::ALL.into_iter().enumerate() {
            assert_eq!(d as usize, i);
            assert_eq!(d.spec().dsp, d);
            assert_eq!(d.code(), format!("D-{}", i + 1));
            assert_eq!(Dsp::from_code(d.code()), Some(d));
            assert_eq!(Dsp::from_slug(d.spec().slug), Some(d));
            assert_eq!(Dsp::from_uuid(d.uuid()), Some(d));
        }
        let mut slugs: Vec<_> = REGISTRY.iter().map(|s| s.slug).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), REGISTRY.len());
    }

    #[test]
    fn every_dsp_has_a_delivery_profile() {
        for s in &REGISTRY {
            assert!(!s.name_ko.is_empty(), "{}", s.code);
            assert!(!s.name_ko.starts_with("D-"), "{}", s.code);
            assert!(!s.deal.commercial_models.is_empty() && !s.deal.use_types.is_empty());
            match s.format {
                DeliveryFormat::Ddex => {
                    assert_eq!(s.ern_version, "3.8.2", "{}", s.code);
                    assert_ne!(s.choreography, ChoreographyProfile::PartnerFeed);
                }
                DeliveryFormat::PartnerSpec => {
                    assert!(s.ern_version.is_empty());
                    assert_eq!(s.channel, Channel::PartnerFeed);
                    assert!(!s.merlin_eligible, "no Merlin deal with Korean services");
                }
            }
        }
        assert_eq!(Dsp::D1.display_name(), "멜론");
        assert_eq!(Dsp::D6.spec().channel, Channel::Transporter);
        assert!(
            Dsp::D5
                .spec()
                .deal
                .commercial_models
                .contains(&"AdvertisementSupportedModel")
        );
    }

    #[test]
    fn uuid_matches_set_dsp_derivation() {
        assert_eq!(
            Dsp::D5.uuid(),
            Uuid::new_v5(&Uuid::NAMESPACE_URL, b"audeniq:dsp:D-5")
        );
    }

    #[test]
    fn requested_maps_studio_slugs_and_codes() {
        let d = serde_json::json!({"platforms":["spotify","melon","D-10","nope","melon"]});
        assert_eq!(requested(&d), Some(vec![Dsp::D1, Dsp::D5, Dsp::D10]));
        assert_eq!(requested(&serde_json::json!({})), None);
    }

    /// The seed migration must list exactly the registry (codes, slugs and
    /// UUIDs), so SQL-side joins never drift from the Rust table.
    #[test]
    fn seed_migration_matches_registry() {
        let sql = include_str!("../../../migrations/0042_dsp_registry.sql");
        for s in &REGISTRY {
            let row = format!("('{}','{}','{}'", s.code, s.dsp.uuid(), s.slug);
            assert!(sql.contains(&row), "missing registry seed {row}");
        }
    }
}
