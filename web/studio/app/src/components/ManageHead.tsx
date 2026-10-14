// 발매 상세 '관리' 탭 카드의 머리 — 아이콘 칸 · 제목 · (있으면) 상태 배지. 홈 '확인할 작업'과 같은 AUDENIQ 줄 모양.
import type { ReactNode } from 'react';
import { Glyph, type GlyphName } from './Glyph';
import { Badge, IconTile } from '../ui';
import type { Tone } from '../ui/tone';

export function ManageHead({ id, icon, title, status }: {
  id: string; icon: GlyphName; title: ReactNode; status?: { tone: Tone; label: ReactNode } | null;
}) {
  return (
    <header className="aq-manage-head">
      <IconTile><Glyph name={icon} size={18} /></IconTile>
      <h2 id={id}>{title}</h2>
      {status && <Badge tone={status.tone}>{status.label}</Badge>}
    </header>
  );
}
