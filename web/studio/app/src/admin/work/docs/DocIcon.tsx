// 문서 아이콘 — 이모지는 기기마다 모양이 달라서 쓰지 않고, AUDENIQ SVG 아이콘 이름을 저장해 그린다.
// 예전에 이모지로 저장된 문서도 비슷한 SVG로 바꿔 보여 준다 (원본 값은 그대로).
import { useEffect, useRef, useState } from 'react';
import { Glyph } from '../../../components/Glyph';

/** 문서 아이콘으로 고를 수 있는 것 */
export const DOC_ICONS = ['doc', 'template', 'book', 'truck', 'scale', 'lock', 'siren', 'antenna', 'pencil', 'checklist', 'chart', 'users', 'release', 'music', 'won', 'mail', 'globe', 'star', 'clock', 'receipt'] as const;

const EMOJI: Record<string, string> = {
  '📄': 'doc', '📃': 'doc', '📑': 'doc', '🗒': 'doc', '📋': 'template', '📘': 'book', '📕': 'book', '📗': 'book', '📙': 'book', '📚': 'book',
  '🚚': 'truck', '🚛': 'truck', '⚖': 'scale', '🔒': 'lock', '🔐': 'lock', '🚨': 'siren', '📡': 'antenna', '📝': 'pencil', '✏': 'pencil',
  '✅': 'checklist', '☑': 'checklist', '📊': 'chart', '📈': 'chart', '👥': 'users', '💿': 'release', '📀': 'release', '🎵': 'music', '🎶': 'music',
  '💰': 'won', '💵': 'won', '✉': 'mail', '📧': 'mail', '🌐': 'globe', '⭐': 'star', '⏰': 'clock', '🧾': 'receipt',
};

/** 저장된 값(아이콘 이름 또는 예전 이모지)을 그릴 아이콘 이름으로 */
export function docIconName(icon: string | null | undefined): string {
  if (!icon) return 'doc';
  if ((DOC_ICONS as readonly string[]).includes(icon)) return icon;
  const bare = icon.replace(/️/g, '').trim();
  return EMOJI[bare] ?? 'doc';
}

export function DocIcon({ icon, size = 16, className }: { icon: string | null | undefined; size?: number; className?: string }) {
  return <Glyph name={docIconName(icon)} size={size} className={className} />;
}

/** 문서 아이콘 고르기 — 이모지 입력 대신 AUDENIQ 아이콘 중에서 고른다 */
export function DocIconPicker({ value, disabled, onChange }: { value: string | null; disabled?: boolean; onChange: (icon: string) => void }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === 'Escape' : !box.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close); document.addEventListener('keydown', close);
    return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', close); };
  }, [open]);
  const current = docIconName(value);
  return (
    <div className="adm-doc-iconpick" ref={box}>
      <button type="button" className="adm-doc-icon" aria-label="문서 아이콘 바꾸기" aria-haspopup="true" aria-expanded={open} disabled={disabled} onClick={() => setOpen(o => !o)}>
        <Glyph name={current} size={22} />
      </button>
      {open && (
        <div className="adm-doc-iconpick-pop" role="listbox" aria-label="문서 아이콘">
          {DOC_ICONS.map(n => (
            <button key={n} type="button" role="option" aria-selected={n === current} className={n === current ? 'is-on' : ''}
              onClick={() => { onChange(n); setOpen(false); }}>
              <Glyph name={n} size={18} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
