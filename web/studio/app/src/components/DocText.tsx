// 전자 문서 본문(줄글 원문)을 읽기 좋게 — 제목·서식 줄, [당사자]·[대상] 같은 묶음은 '항목 / 값' 줄로, 제n조는 굵은 머리로.
// 원문 문자열은 그대로 두고 보여 주는 모양만 바꾼다 (서명·해시는 원문 기준).
// 첫 줄(문서 제목)은 쓰는 화면의 머리에 이미 있으므로 빼고 시작한다.
import type { ReactNode } from 'react';

export function DocText({ text, className = '' }: { text: string; className?: string }) {
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  const out: ReactNode[] = [];
  let section = false; // [묶음] 안에서는 '항목: 값' 줄을 표처럼
  lines.forEach((raw, i) => {
    const line = raw.trim();
    if (!line) { section = false; return; }
    if (i === 0) return;
    if (/^서식\s/.test(line)) { out.push(<p key={i} className="aq-doctext-form">{line}</p>); return; }
    const group = /^\[(.+)\]$/.exec(line);
    if (group) { section = true; out.push(<h5 key={i} className="aq-doctext-group">{group[1]}</h5>); return; }
    if (/^제\d+조\s*\(/.test(line)) { section = false; out.push(<h5 key={i} className="aq-doctext-article">{line}</h5>); return; }
    const kv = section ? /^([^:：]{1,24})[:：]\s*(.+)$/.exec(line) : null;
    if (kv) { out.push(<p key={i} className="aq-doctext-kv"><span>{kv[1]}</span><span>{kv[2]}</span></p>); return; }
    out.push(<p key={i}>{line}</p>);
  });
  return <div className={`aq-doctext${className ? ` ${className}` : ''}`}>{out}</div>;
}
