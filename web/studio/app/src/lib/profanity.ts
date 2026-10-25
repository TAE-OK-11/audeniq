// 욕설 확인 — 서버 crates/core/src/profanity (팬 응원·가사·제목·문의가 같은 필터). 화면은 입력을 멈추면 서버에 물어 알려 주기만 하고,
// 정하는 것은 서버다: 가사·곡 제목·앨범 제목에 욕설이 있으면 저장할 때 그 곡을 19세 이용가로 바꾼다.
import { useEffect, useRef, useState } from 'react';
import { orgPath, req } from '../api/http';
import { MOCK } from './mode';

/** 체험 모드 — 서버 대신 몇 낱말만 (띄어쓰기·기호는 지우고 본다) */
const MOCK_WORDS = ['씨발', '시발', 'ㅅㅂ', '병신', 'ㅂㅅ', '개새끼', '좆', '존나', 'fuck', 'shit'];
const mockCheck = (t: string) => {
  const s = t.toLowerCase().replace(/[^0-9a-z가-힣ㄱ-ㅎㅏ-ㅣ]/g, '');
  return MOCK_WORDS.some(w => s.includes(w));
};

export async function checkTexts(texts: string[]): Promise<boolean[]> {
  if (MOCK) return texts.map(mockCheck);
  const r = await req<{ flags: boolean[] }>(orgPath('/text/profanity'), { method: 'POST', body: { texts }, quiet401: true });
  return r.flags;
}

/** 입력을 멈추고 0.6초 뒤에 묻는다 — 빈 칸은 묻지 않는다. 못 물어보면(연결 끊김) 아무것도 표시하지 않는다 */
export function useProfanity(texts: string[]): boolean[] {
  const [flags, setFlags] = useState<boolean[]>(() => texts.map(() => false));
  const key = JSON.stringify(texts);
  const seq = useRef(0);
  useEffect(() => {
    const list: string[] = JSON.parse(key);
    const n = ++seq.current;
    if (!list.some(t => t.trim())) { setFlags(list.map(() => false)); return; }
    const t = window.setTimeout(() => {
      checkTexts(list).then(f => { if (n === seq.current) setFlags(f); }, () => {});
    }, 600);
    return () => window.clearTimeout(t);
  }, [key]);
  return flags;
}
