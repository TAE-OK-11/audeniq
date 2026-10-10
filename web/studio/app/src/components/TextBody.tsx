// 공지·이벤트 본문 — 빈 줄로 문단을 나누고, 문단 안 줄바꿈은 그대로. 빈 줄마다 빈 문단을 만들어 띄엄띄엄 보이지 않게.
import { Fragment } from 'react';

export function TextBody({ text, className = '' }: { text: string; className?: string }) {
  const paras = text.replace(/\r\n?/g, '\n').trim().split(/\n\s*\n+/).filter(p => p.trim());
  return (
    <div className={`aq-text-body${className ? ` ${className}` : ''}`}>
      {paras.map((p, i) => (
        <p key={i}>
          {p.split('\n').map((line, j) => <Fragment key={j}>{j > 0 && <br />}{line}</Fragment>)}
        </p>
      ))}
    </div>
  );
}
