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

fn letter(l: char) -> bool {
    l.is_ascii_lowercase()
        || ('\u{AC00}'..='\u{D7A3}').contains(&l)
        || ('\u{3131}'..='\u{318E}').contains(&l)
        || ('\u{1100}'..='\u{11FF}').contains(&l)
}

/// The text as the filter sees it: NFKC, lowercase, everything but letters
/// dropped. `spaces` keeps whitespace as one space (the word boundary
/// [`WORD`] needs); `lookalike` reads digits and symbols as the Latin letters
/// they stand for (`sh1t`) — Korean disguises put them between syllables to
/// be dropped instead (`씨1발`), so both readings are checked.
pub fn fold(s: &str, spaces: bool, lookalike: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in normalized(s) {
        if c.is_whitespace() {
            if spaces && !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        let c = if !lookalike {
            c
        } else {
            match c {
                // Look-alikes used to dodge filters.
                '0' => 'o',
                '1' | '!' | '|' => 'i',
                '3' => 'e',
                '4' | '@' => 'a',
                '5' | '$' => 's',
                '7' => 't',
                '*' => 'u',
                _ => c,
            }
        };
        out.extend(c.to_lowercase().filter(|l| letter(*l)));
    }
    out
}

/// A word an artist typed, folded: letters only, no look-alike mapping (so
/// '18' in a word list stays out instead of becoming 'ie').
fn fold_word(w: &str) -> String {
    w.nfkc()
        .flat_map(char::to_lowercase)
        .filter(|l| letter(*l))
        .collect()
}

fn without_allowed(folded: &str) -> String {
    ALLOWED
        .iter()
        .fold(folded.to_string(), |t, a| t.replace(a, " "))
}

/// True when `text` contains profanity, or one of `extra` (an artist's own
/// blocked words, matched like [`STRICT`]).
pub fn contains(text: &str, extra: &[String]) -> bool {
    [true, false]
        .into_iter()
        .any(|lookalike| check(text, extra, lookalike))
}

fn check(text: &str, extra: &[String], lookalike: bool) -> bool {
    let joined = without_allowed(&fold(text, false, lookalike));
    let spaced = without_allowed(&fold(text, true, lookalike));
    // '씨 발'처럼 한 글자만 띄운 것: 홀로 선 씨·시를 다음 낱말과 붙여 다시 본다 ('6시 발매'는 예외 목록이 거른다)
    let tokens: Vec<&str> = spaced.split(' ').filter(|t| !t.is_empty()).collect();
    let split = tokens.windows(2).any(|p| {
        ["씨", "시", "쓰", "ㅅ", "ㅆ"].contains(&p[0]) && {
            let pair = without_allowed(&format!("{}{}", p[0], p[1]));
            WORD.iter().any(|w| pair.contains(w))
        }
    });
    split
        || STRICT.iter().any(|w| joined.contains(w))
        || WORD.iter().any(|w| spaced.contains(w))
        || extra
            .iter()
            .map(|w| fold_word(w))
            .any(|w| !w.is_empty() && joined.contains(&w))
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
