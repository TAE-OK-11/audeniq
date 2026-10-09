// 한국어 조사 고르기 — 앞말 끝소리(받침)에 맞춰 을/를 · 이/가 · 은/는 · 과/와 · 으로/로를 붙인다.
// 숫자는 읽는 소리로(1 일, 2 이, 3 삼 …), 영문은 흔한 읽기로 어림한다.

type Pair = '을/를' | '이/가' | '은/는' | '과/와' | '으로/로';

// 0 영 1 일 2 이 3 삼 4 사 5 오 6 육 7 칠 8 팔 9 구
const DIGIT_BATCHIM = [true, true, false, true, false, false, true, true, true, false];
const DIGIT_RIEUL = [false, true, false, false, false, false, false, true, true, false];

function ending(word: string): { batchim: boolean; rieul: boolean } {
  const s = word.replace(/[\s'"’‘”“)\]}.,!?…·]+$/u, '');
  const ch = s.slice(-1);
  if (!ch) return { batchim: false, rieul: false };
  const code = ch.charCodeAt(0);
  if (code >= 0xac00 && code <= 0xd7a3) {
    const jong = (code - 0xac00) % 28;
    return { batchim: jong !== 0, rieul: jong === 8 };
  }
  if (/\d/.test(ch)) {
    // 10 · 100 · 1000은 '십 · 백 · 천'
    if (/[1-9]0$/.test(s) && !/00$/.test(s)) return { batchim: true, rieul: false }; // 십
    if (/00$/.test(s) && !/000$/.test(s)) return { batchim: true, rieul: false };   // 백
    if (/000$/.test(s)) return { batchim: true, rieul: false };                       // 천·만
    const d = +ch;
    return { batchim: DIGIT_BATCHIM[d], rieul: DIGIT_RIEUL[d] };
  }
  const lower = ch.toLowerCase();
  if (lower === 'l' || lower === 'r') return { batchim: true, rieul: true };
  if ('mn'.includes(lower)) return { batchim: true, rieul: false };
  return { batchim: false, rieul: false };
}

/** 낱말에 알맞은 조사만 */
export function particle(word: string, pair: Pair): string {
  const [withB, withoutB] = pair.split('/');
  const { batchim, rieul } = ending(word);
  if (pair === '으로/로') return batchim && !rieul ? withB : withoutB;
  return batchim ? withB : withoutB;
}

/** 낱말 + 조사 — josa('커버아트', '을/를') → '커버아트를' */
export function josa(word: string | number, pair: Pair): string {
  const w = String(word);
  return w + particle(w, pair);
}
