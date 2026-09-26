//! Protected-artist names: hard block (or review) against artist impersonation.
//!
//! Sandbox round 2 delivered a release by "Taylor Swift" from a random
//! account. Names on the managed list (`catalog.protected_artists` plus
//! per-entry aliases and signature phrases such as "Taylor's Version") are
//! checked wherever they can appear: artist, label and organization names,
//! release and track titles/versions, the release profile (display artist,
//! featured artists) and credited party names. Organizations holding an
//! explicit exception (`catalog.protected_artist_exceptions`) are exempt.
//!
//! Every name/alias carries a policy (migration 0035):
//! - `action`: `BLOCK` refuses the input (422 ARTIST_NAME_PROTECTED, Stage 1
//!   CORRECTION); `REVIEW` lets it through but Stage 1 records
//!   ARTIST_NAME_REVIEW (REVIEW_REQUIRED) for a human.
//! - `match_mode`: `CONTAINS` finds the name anywhere on word boundaries
//!   with full evasion folding (below); `TOKEN` needs the name as whole
//!   words and does no leet folding. `TOKEN` is for short or generic names
//!   ("BTS", "IU", "Drake") that would otherwise hit ordinary words.
//!
//! `CONTAINS` folding: each character is NFKC-normalized, stripped of
//! invisible/format characters and diacritics, lower-cased and folded
//! through the Unicode confusables skeleton (UTS #39: Cyrillic/Greek
//! look-alikes, fullwidth, `1`->`l`...). Leetspeak is matched as additional
//! alternatives per character (`4`/`@`->a, `0`->o, `3`->e, `7`/`+`->t,
//! `$`/`5`->s, `1`/`!`/`|`->i or l, `8`->b, `9`/`6`->g, `2`->z); a symbol
//! may also still act as a separator. Everything that is not a letter or
//! digit is a separator, so inserted spaces/dots/dashes do not help
//! ("T a y l o r", "테일러스위프트" = "테일러 스위프트"). A hit must start at
//! a word start and end at a word end, so "Taylor", "Swift" or "Taylor
//! Swiftly" alone are not matches. A Hangul name may be followed directly by
//! a Korean particle ("테일러 스위프트의").
//!
//! See docs/PROTECTED_ARTISTS.md for policy and list management.
use crate::error::{Error, Result};
use sqlx::PgConnection;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use uuid::Uuid;

/// Error / check code for a blocking protected-name hit.
pub const ARTIST_NAME_PROTECTED: &str = "ARTIST_NAME_PROTECTED";
/// Stage 1 check code for a review-policy protected-name hit.
pub const ARTIST_NAME_REVIEW: &str = "ARTIST_NAME_REVIEW";

/// What a hit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Review,
    Block,
}

impl Action {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("REVIEW") {
            Action::Review
        } else {
            Action::Block
        }
    }
}

/// How a name is matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Contains,
    Token,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("TOKEN") {
            Mode::Token
        } else {
            Mode::Contains
        }
    }
}

/// Korean particles that may follow a Hangul name directly.
const HANGUL_PARTICLES: &[&str] = &[
    "의", "가", "이", "는", "은", "를", "을", "와", "과", "도", "에", "에게", "께", "랑", "이랑",
    "님", "씨", "만", "처럼", "까지", "부터", "로", "으로", "하고", "한테",
];

fn is_hangul(c: char) -> bool {
    matches!(c, '\u{ac00}'..='\u{d7a3}' | '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}')
}

/// Invisible or format characters dropped before matching.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}' | '\u{1160}' | '\u{17b4}' | '\u{17b5}'
        | '\u{180b}'..='\u{180f}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{206f}' | '\u{3164}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}'
        | '\u{ffa0}' | '\u{e0000}'..='\u{e007f}')
        || c.is_control()
}

/// Fold one character to its comparable form ("" for separators).
fn fold_char(c: char) -> String {
    let mut out = String::new();
    for n in c.to_string().nfkc() {
        if invisible(n) {
            continue;
        }
        // Case-fold first so ASCII letters compare case-insensitively, then
        // map look-alikes through the confusables skeleton. A non-ASCII letter
        // whose lower-case form has no prototype (Cyrillic т) is retried via
        // its capital (Т -> T).
        let lower: String = n.to_lowercase().collect();
        let mut skel: String = unicode_security::skeleton(&lower)
            .collect::<String>()
            .to_lowercase();
        if !skel.is_ascii() && !lower.chars().any(is_hangul) {
            let upper: String = n.to_uppercase().collect();
            let alt: String = unicode_security::skeleton(&upper)
                .collect::<String>()
                .to_lowercase();
            if alt.is_ascii() {
                skel = unicode_security::skeleton(&alt)
                    .collect::<String>()
                    .to_lowercase();
            }
        }
        if lower.chars().any(is_hangul) {
            // Hangul is compared as NFC syllables, never decomposed to jamo.
            skel = lower.nfc().collect();
            out.extend(skel.chars().filter(|c| c.is_alphanumeric()));
            continue;
        }
        for s in skel.to_lowercase().nfd() {
            if !is_combining_mark(s) && s.is_alphanumeric() {
                out.push(s);
            }
        }
    }
    out
}

/// Leetspeak readings of one (NFKC-normalized) character.
fn leet(c: char) -> &'static [&'static str] {
    let n: Vec<char> = c.to_string().nfkc().collect();
    let c = if n.len() == 1 { n[0] } else { c };
    match c {
        '4' | '@' => &["a"],
        '0' => &["o"],
        '3' => &["e"],
        '7' | '+' => &["t"],
        '$' | '5' => &["s"],
        '1' | '!' | '|' => &["i", "l"],
        '8' => &["b"],
        '9' | '6' => &["g"],
        '2' => &["z"],
        _ => &[],
    }
}

/// Folded form of a whole needle (list entry): letters/digits only.
pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

/// One visible source character: its possible folded readings.
struct Unit {
    /// Alternatives; "" means "acts as a separator".
    opts: Vec<String>,
    /// Plain reading is a separator (whitespace, punctuation).
    sep: bool,
    /// Plain folded reading (no leet).
    plain: String,
}

fn units(text: &str, with_leet: bool) -> Vec<Unit> {
    let mut out = Vec::new();
    for c in text.chars() {
        if c.to_string().nfkc().all(invisible) {
            continue; // invisible chars neither split nor join words
        }
        let plain = fold_char(c);
        let sep = plain.is_empty();
        let mut opts = vec![plain.clone()];
        if with_leet {
            for l in leet(c) {
                if !opts.iter().any(|o| o == l) {
                    opts.push((*l).to_string());
                }
            }
        }
        out.push(Unit { opts, sep, plain });
    }
    out
}

/// Can a match end before unit `j` (exclusive)? True at the end of the text,
/// before a separator-capable unit, or (Hangul names) before a particle.
fn ends_ok(u: &[Unit], j: usize, needle: &str) -> bool {
    if j >= u.len() || u[j].opts.iter().any(String::is_empty) {
        return true;
    }
    if !needle.chars().last().is_some_and(is_hangul) {
        return false;
    }
    let mut rest = String::new();
    for x in &u[j..] {
        if x.sep {
            break;
        }
        rest.push_str(&x.plain);
    }
    HANGUL_PARTICLES.contains(&rest.as_str())
}

/// CONTAINS match with leet alternatives on word boundaries.
fn contains_match(u: &[Unit], needle: &str) -> bool {
    let pat: Vec<char> = needle.chars().collect();
    let m = pat.len();
    if m == 0 {
        return false;
    }
    let fits = |p: usize, opt: &str| -> Option<usize> {
        let oc: Vec<char> = opt.chars().collect();
        (p + oc.len() <= m && pat[p..p + oc.len()] == oc[..]).then_some(p + oc.len())
    };
    for i in 0..u.len() {
        // Word start: text start or after a separator-capable unit.
        if i > 0 && !u[i - 1].opts.iter().any(String::is_empty) {
            continue;
        }
        let mut states: Vec<usize> = Vec::new();
        for opt in u[i].opts.iter().filter(|o| !o.is_empty()) {
            if let Some(q) = fits(0, opt) {
                states.push(q);
            }
        }
        let mut j = i;
        loop {
            states.sort_unstable();
            states.dedup();
            if states.contains(&m) && ends_ok(u, j + 1, needle) {
                return true;
            }
            states.retain(|&p| p < m);
            j += 1;
            if states.is_empty() || j >= u.len() {
                break;
            }
            let mut next = Vec::new();
            for &p in &states {
                for opt in &u[j].opts {
                    if opt.is_empty() {
                        // Separator reading: skipped. Since p < m here, a
                        // skip can never complete a match, so a match always
                        // ends on a letter.
                        next.push(p);
                    } else if let Some(q) = fits(p, opt) {
                        next.push(q);
                    }
                }
            }
            states = next;
        }
    }
    false
}

/// Tokens (plain folding, no leet) of a text.
fn tokens(u: &[Unit]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for x in u {
        if x.sep {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push_str(&x.plain);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// TOKEN match: the needle's words appear as whole consecutive words.
fn token_match(text_tokens: &[String], needle_tokens: &[String]) -> bool {
    let k = needle_tokens.len();
    if k == 0 || k > text_tokens.len() {
        return false;
    }
    (0..=text_tokens.len() - k).any(|i| {
        (0..k).all(|t| {
            let (have, want) = (&text_tokens[i + t], &needle_tokens[t]);
            have == want
                || (t == k - 1
                    && want.chars().last().is_some_and(is_hangul)
                    && have
                        .strip_prefix(want.as_str())
                        .is_some_and(|rest| HANGUL_PARTICLES.contains(&rest)))
        })
    })
}

#[derive(Debug, Clone)]
struct Needle {
    folded: String,
    tokens: Vec<String>,
    mode: Mode,
    action: Action,
}

/// A protected entry with its folded names and phrases.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    needles: Vec<Needle>,
}

/// One name or alias with its policy.
#[derive(Debug, Clone)]
pub struct NameSpec {
    pub text: String,
    pub mode: Mode,
    pub action: Action,
}

impl NameSpec {
    pub fn new(text: &str, mode: Mode, action: Action) -> Self {
        Self {
            text: text.to_string(),
            mode,
            action,
        }
    }
}

impl Entry {
    /// Build an entry whose name and aliases are all CONTAINS / BLOCK.
    pub fn new(name: &str, aliases: &[String]) -> Self {
        let specs: Vec<NameSpec> = aliases
            .iter()
            .map(|a| NameSpec::new(a, Mode::Contains, Action::Block))
            .collect();
        Self::with_policy(NameSpec::new(name, Mode::Contains, Action::Block), &specs)
    }

    /// Build an entry with per-name policies.
    pub fn with_policy(name: NameSpec, aliases: &[NameSpec]) -> Self {
        let mut needles: Vec<Needle> = Vec::new();
        for s in std::iter::once(&name).chain(aliases.iter()) {
            let folded = fold(&s.text);
            let n = folded.chars().count();
            let hangul = folded.chars().any(is_hangul);
            // Very short CONTAINS needles would match everywhere; TOKEN needs
            // at least two characters. Hangul syllables carry more signal.
            let min = match (s.mode, hangul) {
                (Mode::Token, _) => 2,
                (Mode::Contains, true) => 3,
                (Mode::Contains, false) => 4,
            };
            if n < min {
                continue;
            }
            needles.push(Needle {
                tokens: tokens(&units(&s.text, false)),
                folded,
                mode: s.mode,
                action: s.action,
            });
        }
        Self {
            name: name.text,
            needles,
        }
    }

    /// Strongest action any of this entry's names triggers on `text`.
    pub fn hit(&self, text: &str) -> Option<Action> {
        let leet_units = units(text, true);
        let text_tokens = tokens(&leet_units);
        self.needles
            .iter()
            .filter(|n| match n.mode {
                Mode::Contains => contains_match(&leet_units, &n.folded),
                Mode::Token => token_match(&text_tokens, &n.tokens),
            })
            .map(|n| n.action)
            .max()
    }

    /// True when `text` triggers a BLOCK on this protected artist.
    pub fn matches(&self, text: &str) -> bool {
        self.hit(text) == Some(Action::Block)
    }
}

/// First protected entry that BLOCKs any of `texts`.
pub fn find<'a>(entries: &'a [Entry], texts: &[&str]) -> Option<&'a Entry> {
    entries.iter().find(|e| texts.iter().any(|t| e.matches(t)))
}

/// First entry with a REVIEW-level (but no BLOCK) hit on any of `texts`.
pub fn find_review<'a>(entries: &'a [Entry], texts: &[&str]) -> Option<&'a Entry> {
    entries
        .iter()
        .find(|e| texts.iter().any(|t| e.hit(t) == Some(Action::Review)))
}

/// Active protected entries that apply to `org` (entries for which the org
/// holds an unrevoked exception are skipped).
pub async fn load(c: &mut PgConnection, org: Uuid) -> Result<Vec<Entry>> {
    type Row = (
        String,
        String,
        String,
        Vec<String>,
        Vec<String>,
        Vec<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT p.name, p.match_mode, p.action,
                COALESCE(array_agg(a.alias ORDER BY a.alias) FILTER (WHERE a.alias IS NOT NULL), '{}'),
                COALESCE(array_agg(a.match_mode ORDER BY a.alias) FILTER (WHERE a.alias IS NOT NULL), '{}'),
                COALESCE(array_agg(a.action ORDER BY a.alias) FILTER (WHERE a.alias IS NOT NULL), '{}')
         FROM catalog.protected_artists p
         LEFT JOIN catalog.protected_artist_aliases a ON a.protected_artist_id=p.id
         WHERE p.active AND NOT EXISTS (
           SELECT 1 FROM catalog.protected_artist_exceptions x
           WHERE x.protected_artist_id=p.id AND x.org_id=$1 AND x.revoked_at IS NULL)
         GROUP BY p.id, p.name, p.match_mode, p.action",
    )
    .bind(org)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|(n, nm, na, aliases, modes, actions)| {
            let specs: Vec<NameSpec> = aliases
                .iter()
                .zip(modes)
                .zip(actions)
                .map(|((a, m), x)| NameSpec::new(a, Mode::parse(m), Action::parse(x)))
                .collect();
            Entry::with_policy(NameSpec::new(n, Mode::parse(nm), Action::parse(na)), &specs)
        })
        .collect())
}

/// Refuse input that names (BLOCK policy) a protected artist the org is not
/// cleared for. REVIEW-policy hits pass here and are flagged in Stage 1.
pub async fn enforce(c: &mut PgConnection, org: Uuid, texts: &[&str]) -> Result<()> {
    if texts.iter().all(|t| t.trim().is_empty()) {
        return Ok(());
    }
    let entries = load(c, org).await?;
    match find(&entries, texts) {
        Some(e) => {
            tracing::info!(%org, protected = %e.name, "protected artist name refused");
            Err(Error::PolicyGate(ARTIST_NAME_PROTECTED))
        }
        None => Ok(()),
    }
}

/// Every string inside a JSON value (profile documents).
pub fn json_strings(v: &serde_json::Value) -> Vec<&str> {
    let mut out = Vec::new();
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match v {
            serde_json::Value::String(s) => out.push(s),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> Vec<Entry> {
        vec![Entry::new(
            "Taylor Swift",
            &[
                "Taylor Alison Swift".into(),
                "Taylor's Version".into(),
                "테일러 스위프트".into(),
            ],
        )]
    }

    fn seeded() -> Vec<Entry> {
        use Action::*;
        use Mode::*;
        vec![
            Entry::with_policy(
                NameSpec::new("BTS", Token, Review),
                &[
                    NameSpec::new("방탄소년단", Contains, Block),
                    NameSpec::new("Bangtan Boys", Contains, Block),
                ],
            ),
            Entry::with_policy(
                NameSpec::new("IU", Token, Review),
                &[NameSpec::new("아이유", Token, Block)],
            ),
            Entry::with_policy(
                NameSpec::new("BLACKPINK", Contains, Block),
                &[NameSpec::new("블랙핑크", Contains, Block)],
            ),
        ]
    }

    #[test]
    fn blocks_name_and_evasion_variants() {
        let e = ts();
        for bad in [
            "Taylor Swift",
            "taylor swift",
            "TAYLOR SWIFT",
            "TaylorSwift",
            "T a y l o r  S w i f t",
            "T.aylor Swift",
            "T.a.y.l.o.r S.w.i.f.t",
            "Taylor-Swift",
            "Taylor_Swift",
            "Tay\u{200b}lor Sw\u{200d}ift",
            "Taylor \u{202e}Swift",
            "\u{feff}Taylor Swift",
            "Ｔａｙｌｏｒ Ｓｗｉｆｔ", // fullwidth
            "Тaylor Swift",            // Cyrillic Т
            "Taylοr Swift",            // Greek omicron
            "Tay1or Swift",            // digit one for l
            "Taylör Swíft",            // diacritics
            "𝐓𝐚𝐲𝐥𝐨𝐫 𝐒𝐰𝐢𝐟𝐭",            // math bold
            "Song (feat. Taylor Swift)",
            "Artist feat. Taylor Swift",
            "Artist ft. Taylor Swift",
            "Artist with Taylor Swift",
            "Artist x Taylor Swift",
            "Artist & Taylor Swift",
            "Love Story (Taylor's Version)",
            "Love Story (Taylors Version)",
            "Taylor Alison Swift",
        ] {
            assert!(find(&e, &[bad]).is_some(), "should block {bad:?}");
        }
    }

    #[test]
    fn blocks_leetspeak() {
        let e = ts();
        for bad in [
            "T4ylor Swift",
            "Tayl0r Sw1ft",
            "7aylor $wift",
            "T@yl0r $w!ft",
            "+aylor 5wift",
            "TAYL0R SW|FT",
            "Love Story (T4ylor's Version)",
            "Love Story (Tayl0r’s V3rsion)",
            "DJ Nobody ft. T4yl0r Sw1f7",
            "Taylor + Swift",
        ] {
            assert!(find(&e, &[bad]).is_some(), "should block {bad:?}");
        }
    }

    #[test]
    fn blocks_korean_with_and_without_spaces() {
        let e = ts();
        for bad in [
            "테일러 스위프트",
            "테일러스위프트",
            "테일러  스위프트",
            "Song (feat. 테일러 스위프트)",
            "테일러 스위프트의 노래",
            "테일러 스위프트 팬클럽",
        ] {
            assert!(find(&e, &[bad]).is_some(), "should block {bad:?}");
        }
        for ok in ["테일러", "스위프트", "테일러 스위프트니스트"] {
            assert!(find(&e, &[ok]).is_none(), "false positive on {ok:?}");
        }
    }

    #[test]
    fn leaves_unrelated_names_alone() {
        let e = ts();
        for ok in [
            "Taylor",
            "Swift",
            "Taylor Smith",
            "James Taylor",
            "Swift River",
            "Taylor Swiftly",
            "Taylormade Swifts",
            "Tayl0rmade Sw1fts",
            "My Version",
            "Taylor Made Version",
            "Tailor Swift",
            "Swift 4 Taylor",
            "BTS",
            "2024 Mix",
            "",
        ] {
            assert!(find(&e, &[ok]).is_none(), "false positive on {ok:?}");
        }
    }

    #[test]
    fn short_names_use_token_policy() {
        let e = seeded();
        // Short Latin names: whole-word only, and REVIEW rather than BLOCK.
        for review in [
            "BTS",
            "Song (feat. BTS)",
            "bts - dynamite",
            "IU",
            "Love wins all - IU",
        ] {
            assert!(
                find(&e, &[review]).is_none(),
                "{review:?} must not hard-block"
            );
            assert!(
                find_review(&e, &[review]).is_some(),
                "{review:?} should go to review"
            );
        }
        for ok in [
            "Subtitles",
            "ABTS",
            "BTSX",
            "Iuliana",
            "Fiume",
            "Bits",
            "b8ts",
            "Behind the scenes",
        ] {
            assert!(find(&e, &[ok]).is_none(), "false block on {ok:?}");
            assert!(find_review(&e, &[ok]).is_none(), "false review on {ok:?}");
        }
        // Distinctive and Korean names block, with Hangul particles allowed.
        for bad in [
            "방탄소년단",
            "방탄 소년단",
            "방탄소년단의 노래",
            "Bangtan Boys",
            "B4ngtan B0ys",
            "BLACKPINK",
            "Black Pink",
            "BL4CKP1NK",
            "블랙핑크",
            "아이유",
            "아이유의 노래",
            "feat. 아이유",
        ] {
            assert!(find(&e, &[bad]).is_some(), "should block {bad:?}");
        }
        for ok in ["아이유니버스", "아이", "핑크"] {
            assert!(find(&e, &[ok]).is_none(), "false block on {ok:?}");
        }
    }

    #[test]
    fn short_aliases_are_ignored() {
        // Very short folded CONTAINS aliases would match everywhere; dropped.
        let e = vec![Entry::new("Ab", &[])];
        assert!(find(&e, &["Ab"]).is_none());
    }
}
