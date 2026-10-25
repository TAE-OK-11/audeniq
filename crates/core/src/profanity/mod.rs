//! Profanity filter for what fans write on artist pages (cheers: nickname and
//! message), plus words each artist blocks on their own page.
//!
//! Text is compared after folding: NFKC, lowercase, full-width and
//! look-alike characters to their letters, and every character that is not
//! a Hangul syllable, Hangul jamo or a Latin letter removed — so spaces,
//! dots, digits and emoji put between letters (`씨 발`, `시.발`, `씨1발`,
//! `f*ck`) do not get through. Ordinary words that contain a listed form
//! (`시발점` departure point, `병신년` the year 丙申) are taken out before
//! the check. Short initial-consonant forms (`ㅅㅂ`) are matched on the jamo
//! that remain.
//!
//! Words alone that are also everyday Korean (`미친` "crazy good", `새끼`,
//! `보지` "to see") are not listed; only their insulting compounds are.
//!
//! Besides the lists here, the filter uses korcen's lists (MIT, see
//! [`words`]) and its idea of reading symbols as the Hangul they look like
//! (`^^ㅣ발`, `🐶새끼`), digit forms (`씨8`, `18놈`) and Korean typed on an
//! English keyboard (`tlqkf`).
use unicode_normalization::UnicodeNormalization;

/// Forms blocked wherever they appear, spaces and symbols between letters
/// ignored: they do not occur across the boundary of two ordinary words.
const STRICT: &[&str] = &[
    // 욕설
    "씨빨",
    "시빨",
    "씨팔",
    "시팔",
    "씨벌",
    "시벌",
    "쓰발",
    "쓰벌",
    "씌발",
    "쒸발",
    "씨이발",
    "시이발",
    "씨부랄",
    "씨부럴",
    "ㅅㅂ",
    "ㅆㅂ",
    "ㅅㅃ",
    "ㅆㅃ",
    "ㅆ발",
    "ㅅ발",
    "씨ㅂ",
    "시ㅂ",
    "병신",
    "븅신",
    "빙신",
    "병싄",
    "ㅂㅅ",
    "ㅄ",
    "좆",
    "ㅈ같",
    "ㅈ까",
    "ㅈㄴ",
    "개새끼",
    "개새기",
    "개색기",
    "개색히",
    "개세끼",
    "개쉐이",
    "개씨발",
    "씨발새끼",
    "씹새",
    "씹창",
    "씹년",
    "씹놈",
    "ㅆㄹㄱ",
    "쓰레기새끼",
    "니애미",
    "느금마",
    "느검마",
    "느개비",
    "니애비",
    "애미뒤진",
    "엠창",
    "니미럴",
    "지랄",
    "ㅈㄹ",
    "꺼져",
    "닥쳐",
    "닥치라",
    "엿먹",
    "미친놈",
    "미친년",
    "미친새끼",
    "정신병자",
    "썅",
    "쌍년",
    "쌍놈",
    "화냥",
    "창녀",
    "걸레년",
    // 비하·혐오
    "한남충",
    "김치녀",
    "된장녀",
    "틀딱",
    "장애새끼",
    "흑형",
    "짱깨",
    "쪽바리",
    "똥남아",
    // 영어 (여러 낱말에 걸쳐 우연히 생기지 않는 것만)
    "fuck",
    "motherfucker",
    "nigger",
    "faggot",
];

/// Forms that two ordinary words can spell across a space (`6시 발매`,
/// `한결씨 발표`, `또 라이브`, `맘 충전`): blocked only within a word, with
/// symbols and digits between letters still ignored (`시.발`, `씨1발`).
const WORD: &[&str] = &[
    "씨발",
    "시발",
    "씨바",
    "시바",
    "존나",
    "졸라",
    "또라이",
    "돌아이",
    "애자",
    "맘충",
    "급식충",
    "개놈",
    "shit",
    "bitch",
    "biatch",
    "asshole",
    "cunt",
    "nigga",
    "retard",
    "whore",
    "slut",
    "bastard",
    "dickhead",
    "fuk",
    "fck",
];

/// Everyday words that contain a listed form; taken out before the check.
const ALLOWED: &[&str] = &[
    "시발점",
    "시바 강아지",
    "시바 이누",
    "시바 견",
    "시발역",
    "시발택시",
    "병신년",
    "병신정변",
    "시바견",
    "시바이누",
    "애자일",
    "닥쳐오",
    "지랄탄",
    "꺼져가",
    "꺼져서",
    "꺼져도",
    "꺼져있",
    "꺼져요",
    "불이꺼져",
    "scunthorpe",
    // 시·씨 바로 뒤 '발…' 낱말 (몇 시 발매, 한결씨 발표)
    "시발매",
    "씨발매",
    "시발표",
    "씨발표",
    "시발라드",
    "씨발라드",
    "시발견",
    "씨발견",
    "시발송",
    "씨발송",
    "시발랄",
    "씨발랄",
    "시발렌",
    "씨발렌",
    "시발행",
    "씨발행",
    "시발전",
    "씨발전",
    "시발음",
    "씨발음",
    "시발성",
    "씨발성",
    "시발등",
    "씨발등",
    "시바다",
    "씨바다",
    "시바람",
    "씨바람",
    "시바로",
    "씨바로",
    "시바이",
    "씨바이",
    "시바빠",
    "씨바빠",
    "시바쁘",
    "씨바쁘",
];

mod words;

/// NFKC for everything but Hangul: full-width and styled Latin become plain
/// letters, while compatibility jamo (`ㅅㅂ`) stay as typed (NFKC would turn
/// them into conjoining jamo the lists do not use).
fn normalized(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().flat_map(|c| {
        let hangul =
            ('\u{AC00}'..='\u{D7A3}').contains(&c) || ('\u{3131}'..='\u{318E}').contains(&c);
        let one = [c];
        let keep: Vec<char> = if hangul {
            one.to_vec()
        } else {
            one.iter().copied().nfkc().collect()
        };
        keep
    })
}

fn hangul(l: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&l)
        || ('\u{3131}'..='\u{318E}').contains(&l)
        || ('\u{1100}'..='\u{11FF}').contains(&l)
}

/// How digits and symbols between letters are read. Each reading is checked,
/// since one text can mix them (`sh1t`, `씨1발`, `씨8`, `^^ㅣ발`).
#[derive(Clone, Copy)]
enum Reading {
    /// Dropped (`씨1발` → `씨발`).
    Drop,
    /// As the Latin letters they stand for (`sh1t` → `shit`).
    Latin,
    /// As the Hangul jamo they look like (`^^ㅣ발` → `ㅅㅅㅣ발`, `🐶새끼` → `개새끼`).
    Hangul,
    /// Kept: digit forms (`씨8`, `18놈`).
    Digits,
}

fn read(c: char, how: Reading) -> Option<char> {
    match how {
        Reading::Drop => None,
        Reading::Latin => match c {
            '0' => Some('o'),
            '1' | '!' | '|' => Some('i'),
            '3' => Some('e'),
            '4' | '@' => Some('a'),
            '5' | '$' => Some('s'),
            '7' => Some('t'),
            '*' => Some('u'),
            _ => None,
        },
        Reading::Hangul => match c {
            '^' | '人' | '∧' | 'Λ' | '⋀' => Some('ㅅ'),
            '甘' | '廿' | '田' | '口' | '日' | '目' | '囗' => Some('ㅂ'),
            '己' | '乙' | '已' | '巳' => Some('ㄹ'),
            '1' | '!' | '|' | '丨' | '¦' => Some('ㅣ'),
            '0' | '○' | '◯' | '◎' | '●' => Some('ㅇ'),
            '🐶' | '🐕' | '🐺' => Some('개'),
            '🐦' | '🐔' | '🦅' | '🦆' | '🦉' => Some('새'),
            _ => None,
        },
        Reading::Digits => c.is_ascii_digit().then_some(c),
    }
}

/// The text as the filter sees it: NFKC, lowercase, Hangul and Latin letters
/// kept, digits and symbols read as `how` says, everything else dropped.
/// `spaces` keeps whitespace as one space (the word boundary [`WORD`]
/// needs).
fn fold(s: &str, spaces: bool, how: Reading) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last: Option<char> = None;
    for c in normalized(s) {
        if c.is_whitespace() {
            if spaces && !out.ends_with(' ') {
                out.push(' ');
            }
            last = None;
            continue;
        }
        // '77' → 'ㄲ'
        if matches!(how, Reading::Hangul) && c == '7' && last == Some('7') {
            out.push('ㄲ');
            last = None;
            continue;
        }
        last = Some(c);
        for l in c.to_lowercase() {
            // 한글 바로 뒤의 l·i·o는 ㅣ·ㅇ로 ('시l발') — 영어 낱말 안은 그대로 ('scunthorpe')
            let after_hangul = out.chars().last().is_some_and(hangul);
            let latin_look = match l {
                'l' | 'i' => Some('ㅣ'),
                'o' => Some('ㅇ'),
                _ => None,
            };
            if let (Reading::Hangul, true, Some(j)) = (how, after_hangul, latin_look) {
                out.push(j);
            } else if let Some(r) = read(l, how) {
                out.push(r);
            } else if l.is_ascii_lowercase() || hangul(l) {
                out.push(l);
            }
        }
    }
    out
}

/// Jamo typed apart joined into syllables where a consonant is followed by a
/// vowel (`ㅅㅣ발` → `시발`, `ㅆㅣ` → `씨`); finals are not joined.
fn compose(s: &str) -> String {
    const INITIALS: &str = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";
    const VOWELS: &str = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let ini = INITIALS.chars().position(|c| c == chars[i]);
        let vow = chars
            .get(i + 1)
            .and_then(|v| VOWELS.chars().position(|c| c == *v));
        if let (Some(a), Some(b)) = (ini, vow)
            && let Some(syl) = char::from_u32(0xAC00 + (a as u32 * 21 + b as u32) * 28)
        {
            out.push(syl);
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn composed(list: &'static [&'static str]) -> Vec<String> {
    list.iter().map(|w| compose(w)).collect()
}

/// The lists as matched (jamo joined like the text).
struct Lists {
    strict: Vec<String>,
    word: Vec<String>,
    korcen: Vec<String>,
}

fn lists() -> &'static Lists {
    static L: std::sync::OnceLock<Lists> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let word = composed(WORD);
        Lists {
            strict: composed(STRICT),
            korcen: composed(words::KORCEN)
                .into_iter()
                .filter(|w| !word.contains(w))
                .collect(),
            word,
        }
    })
}

/// A word an artist typed, folded: letters and digits as typed.
fn fold_word(w: &str) -> String {
    normalized(w)
        .flat_map(char::to_lowercase)
        .filter(|l| l.is_ascii_lowercase() || l.is_ascii_digit() || hangul(*l))
        .collect()
}

fn without_allowed(folded: &str) -> String {
    ALLOWED
        .iter()
        .chain(words::KORCEN_ALLOWED)
        .fold(folded.to_string(), |t, a| t.replace(a, " "))
}

/// True when `text` contains profanity, or one of `extra` (an artist's own
/// blocked words, matched like [`STRICT`]).
pub fn contains(text: &str, extra: &[String]) -> bool {
    if words::GESTURES.iter().any(|g| text.contains(g)) {
        return true;
    }
    [
        Reading::Drop,
        Reading::Latin,
        Reading::Hangul,
        Reading::Digits,
    ]
    .into_iter()
    .any(|how| check(text, extra, how))
}

fn check(text: &str, extra: &[String], how: Reading) -> bool {
    let l = lists();
    let joined = without_allowed(&compose(&fold(text, false, how)));
    let spaced = without_allowed(&compose(&fold(text, true, how)));
    let tokens: Vec<&str> = spaced.split(' ').filter(|t| !t.is_empty()).collect();
    // 가운데 손가락 ㅗ — 낱말로 홀로 설 때만 (ㅗㅜㅑ 같은 감탄은 그대로)
    let finger = tokens.iter().any(|t| t.chars().all(|c| c == 'ㅗ'));
    // '씨 발'처럼 한 글자만 띄운 것: 홀로 선 씨·시를 다음 낱말과 붙여 다시 본다 ('6시 발매'는 예외 목록이 거른다)
    let split = tokens.windows(2).any(|p| {
        ["씨", "시", "쓰", "ㅅ", "ㅆ"].contains(&p[0]) && {
            let pair = without_allowed(&format!("{}{}", p[0], p[1]));
            l.word.iter().any(|w| pair.contains(w.as_str()))
        }
    });
    finger
        || split
        || l.strict.iter().any(|w| joined.contains(w.as_str()))
        // korcen의 낱말 가운데 띄어 쓴 두 낱말에 걸쳐 우연히 생기는 것(WORD)은 낱말 안에서만 (lists()가 뺀다)
        || l.korcen.iter().any(|w| joined.contains(w.as_str()))
        || l.word.iter().any(|w| spaced.contains(w.as_str()))
        || extra
            .iter()
            .map(|w| compose(&fold_word(w)))
            .any(|w| !w.is_empty() && joined.contains(&w))
}

/// Song texts (release title, track title and version, lyrics) with
/// profanity: the release is marked 19+ (`parental_advisory`, the explicit
/// flag and the domestic 19금 marking) instead of being refused.
pub fn in_song(texts: &[&str]) -> bool {
    texts
        .iter()
        .any(|t| !t.trim().is_empty() && contains(t, &[]))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckInput {
    pub texts: Vec<String>,
}

/// `POST /api/orgs/{org}/text/profanity` `{texts}` → `{flags}`: which texts
/// the filter catches, so a form can say so while the artist types (the
/// server still decides on save).
pub async fn check_texts(
    s: &crate::api::AppState,
    a: &crate::auth::Actor,
    org: uuid::Uuid,
    i: CheckInput,
) -> crate::error::Result<serde_json::Value> {
    if i.texts.len() > 40 || i.texts.iter().any(|t| t.chars().count() > 20_000) {
        return Err(crate::error::Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    crate::auth::membership(&mut tx, a, org, false).await?;
    tx.rollback().await?;
    crate::auth::rate(&s.pool, &format!("profanity-check:{}", a.user), 120).await?;
    let flags: Vec<bool> = i.texts.iter().map(|t| contains(t, &[])).collect();
    Ok(serde_json::json!({ "flags": flags }))
}

async fn h_check(
    axum::extract::State(s): axum::extract::State<crate::api::AppState>,
    axum::extract::Path(org): axum::extract::Path<uuid::Uuid>,
    h: axum::http::HeaderMap,
    axum::Json(i): axum::Json<CheckInput>,
) -> crate::error::Result<axum::Json<serde_json::Value>> {
    let a = crate::auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(axum::Json(check_texts(&s, &a, org, i).await?))
}

pub fn routes() -> axum::Router<crate::api::AppState> {
    axum::Router::new().route(
        "/api/orgs/{org}/text/profanity",
        axum::routing::post(h_check),
    )
}

/// An artist's blocked words, cleaned: trimmed, at most 20 characters with
/// at least one letter, duplicates (as folded) dropped; `None` when one is
/// invalid or there are more than `max`.
pub fn clean_words(words: &[String], max: usize) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let w = w.trim();
        if w.is_empty() {
            continue;
        }
        if w.chars().count() > 20
            || fold_word(w).is_empty()
            || !crate::text_policy::is_clean(w, false)
        {
            return None;
        }
        if !out.iter().any(|o| fold_word(o) == fold_word(w)) {
            out.push(w.to_string());
        }
    }
    (out.len() <= max).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_profanity_and_its_disguises() {
        for s in [
            "씨발",
            "씨 발 진짜",
            "시.발",
            "씨1발",
            "ㅅㅂ",
            "ㅆ ㅂ",
            "병 신",
            "ㅂㅅ같네",
            "존나 별로",
            "개새끼야",
            "니애미",
            "F*CK this",
            "f u c k",
            "sh1t",
            "느금마",
            "한남충",
            "지랄하네",
            "꺼져라",
            "ｓｈｉｔ",
            "씨😀발",
            "또라이네",
            "맘충",
            "꺼 져",
            "씨 발",
            "시 발 놈아",
            "tlqkf",
            "^^ㅣ발",
            "🐶새끼야",
            "씨8",
            "18놈아",
            "ㅗ",
            "ㅗㅗ 이거나 먹어",
            "🖕",
            "느그애미",
            "rotorl",
            "개같네",
            "싸가지 없네",
            "짱깨",
            "77ㅓ져",
        ] {
            assert!(contains(s, &[]), "{s}");
        }
    }

    #[test]
    fn leaves_everyday_words_alone() {
        for s in [
            "이번 앨범 미친 듯이 좋아요",
            "시발점이 된 노래",
            "병신년에 태어났어요",
            "우리 강아지 새끼 같아",
            "보지 마세요 아까워요",
            "시바견 키워요",
            "애자일하게 작업하시네요",
            "불이 꺼져도 들을게요",
            "18번째 곡이 최고",
            "Scunthorpe에서 들어요",
            "assassin creed ost 같아요",
            "공연 꼭 갈게요!",
            "언니 사랑해 ❤",
            "새벽에 듣기 좋아요",
            "6시 발매 기다려요",
            "한결씨 발표 축하해요",
            "오후 6시발매 맞죠",
            "또 라이브 해 주세요",
            "언니 미쳤다 진짜",
            "언니 엄마랑 같이 들어요",
            "맘 충전 완료",
            "5개년 계획처럼 오래 활동해요",
            "this hit is great",
            "시 바다 보러 가요",
            "한결 씨 발매 축하",
            "6 시 발표",
            "ㅁㅊ 이번 앨범 너무 좋아",
            "ㅗㅜㅑ 미쳤다",
            "새까만 밤을 노래한 곡",
            "하루 세끼 이 노래만",
            "꿈을 좇는 가사",
            "자위대 뉴스 말고 음악",
            "애널리스트도 극찬",
            "노란색 스웨터 입고 공연",
            "같은 년도에 데뷔했어요",
            "아저씨 발라드 좋아요",
            "다시 불러 주세요",
            "피시방에서 들었어요",
            "8시 발표 기다림",
            "전염병 같은 중독성",
            "우리 강아지 새끼손가락",
            "1년 기다렸어요",
            "시바견 키우는 팬",
        ] {
            assert!(!contains(s, &[]), "{s}");
        }
    }

    #[test]
    fn artist_words_are_blocked_too() {
        let words = vec!["스포".to_string(), "광고".to_string()];
        assert!(contains("스 포 하지 마세요", &words));
        assert!(contains("광고합니다 www", &words));
        assert!(!contains("좋은 노래예요", &words));
    }

    #[test]
    fn cleans_word_lists() {
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            clean_words(&w(&[" 스포 ", "", "스 포", "광고"]), 10),
            Some(w(&["스포", "광고"]))
        );
        assert_eq!(clean_words(&w(&["!!!"]), 10), None);
        assert_eq!(
            clean_words(&w(&["가나다라마바사아자차카타파하가나다라마바사"]), 10),
            None
        );
        assert_eq!(clean_words(&w(&["a", "b", "c"]), 2), None);
    }
}

#[cfg(test)]
mod fan_comments {
    /// Everyday fan comments (praise, slang, numbers, family) stay allowed.
    #[test]
    fn everyday_comments_pass() {
        let lines = [
            "언니 목소리 진짜 미쳤어요",
            "이번 노래 개좋아요",
            "개쩐다 진짜",
            "존맛탱 노래",
            "가사가 너무 슬퍼서 울었어요",
            "새벽에 들으면 딱이에요",
            "공연 언제 해요?",
            "콘서트 가고 싶어요ㅠㅠ",
            "서울 공연 꼭 와주세요",
            "부산에도 와주세요",
            "앨범 커버 너무 예뻐요",
            "뮤비 언제 나와요",
            "벌써 100번 들었어요",
            "1위 가자",
            "멜론 차트 들어가길",
            "어제 라이브 대박이었어요",
            "다음 앨범 기다릴게요",
            "목소리가 꿀이에요",
            "이 노래 듣고 힘냈어요",
            "발라드 너무 좋아요",
            "발매 축하해요!!",
            "시간 날 때마다 들어요",
            "시 같은 가사",
            "씨앗처럼 자라는 노래",
            "아가씨 노래 커버해 주세요",
            "신발 끈 묶으며 들어요",
            "시바 강아지랑 산책하며 들어요",
            "병원에서 들었어요 힘내요",
            "지난 공연 지금도 생각나요",
            "꺼진 불도 다시 보자",
            "새해 복 많이 받으세요",
            "새로운 곡 기대돼요",
            "좋은 노래 감사합니다",
            "다시 봐도 좋아요",
            "보고 싶어요 언니",
            "자기 전에 꼭 들어요",
            "잘 자요 언니",
            "자장가 같아요",
            "미친 듯이 반복재생",
            "진짜 미쳤다 이 곡",
            "18살 때부터 팬이에요",
            "10년째 팬입니다",
            "2번 트랙 최고",
            "3시 발매 맞죠?",
            "12시 발매 대기중",
            "개강해도 들을게요",
            "개발자인데 코딩할 때 들어요",
            "개나리 피는 봄에 어울려요",
            "강아지 새끼 낳았는데 같이 들어요",
            "고양이 새끼가 좋아해요",
            "에너지 넘치는 곡",
            "다시 불러주세요",
            "엄마랑 같이 들었어요",
            "아빠가 좋아하세요",
            "언니 아빠 노래 커버해주세요",
            "니가 최고야",
            "너의 목소리가 들려",
            "그 시절 우리가 좋아했던",
            "종이비행기 날리는 기분",
            "지하철에서 들어요",
            "시험 끝나고 들을게요",
            "회사에서 몰래 들어요",
            "출근길 필수곡",
            "퇴근길에 위로받아요",
            "하루 종일 반복",
            "운동할 때 들어요",
            "무대 찢었다",
            "고음 소름 돋았어요",
            "숨 막히는 고음",
            "닭살 돋았어요",
        ];
        let bad: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| super::contains(l, &[]))
            .collect();
        assert!(bad.is_empty(), "{bad:?}");
    }
}
