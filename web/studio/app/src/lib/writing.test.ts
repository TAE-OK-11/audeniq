// 문구 검사 — 토스 UX 라이팅 규칙(해요체·능동형·캐주얼 경어)에서 벗어난 표현이 아티스트 화면에 다시 들어오지 않게.
// 법적 문서(약관·계약서·신청서 동의 문구)와 관리자 화면은 뺀다. 주석은 지우고 본다.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..');
const SKIP = [/^admin\//, /\.test\./, /^store\/docs\.ts$/, /^lib\/application\.ts$/, /^lib\/terms/, /legal/i, /agreementDoc/, /rightsDocument/, /privacy/i, /^content\//, /^pages\/Rights\.tsx$/];

const RULES: [RegExp, string][] = [
  [/님께|분께/, "'께' 대신 '에게'"],
  [/되었|되어요/, "'되었'·'되어요' 대신 능동형이나 '됐어요'·'돼요'"],
  [/시겠어요|하시겠|하십시오/, "'~시겠어요' 같은 과한 경어 대신 해요체"],
  [/해주세요|해주시/, "'해 주세요'로 띄어 쓰기"],
  [/미입력|미등록/, "'미입력'·'미등록' 대신 '없음'·'입력 전'·'등록 전'"],
  // ‘어데닉’은 받침(ㄱ)으로 끝난다 — AUDENIQ은·이·을·으로·이에요 (AUDENIQ STUDIO는 ‘스튜디오’라 는·가·를·로)
  [/AUDENIQ(는|가|를|로|예요)(?![가-힣])/, "AUDENIQ 뒤 조사는 받침 있는 말처럼(은·이·을·으로·이에요)"],
];

function files(dir: string): string[] {
  return readdirSync(dir).flatMap(n => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : /\.tsx?$/.test(n) ? [p] : [];
  });
}

const stripComments = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`])\/\/.*$/gm, '$1');

describe('UX 라이팅 규칙', () => {
  const targets = files(ROOT).map(p => relative(ROOT, p)).filter(p => !SKIP.some(re => re.test(p)));
  for (const [re, why] of RULES) {
    it(why, () => {
      const hits = targets.flatMap(p => stripComments(readFileSync(join(ROOT, p), 'utf8')).split('\n')
        .map((line, i) => (re.test(line) ? `${p}:${i + 1} ${line.trim().slice(0, 80)}` : ''))
        .filter(Boolean));
      expect(hits).toEqual([]);
    });
  }
});
