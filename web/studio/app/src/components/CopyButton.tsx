// 복사 버튼 — 누르면 버튼 자체가 잠깐 '복사됨 ✓'으로 바뀌어 눌렀는지 바로 보인다 (아래 토스트를 찾지 않아도 되게).
// 화면 읽기 프로그램에도 바뀐 글자를 읽어 준다. 복사를 막는 브라우저면 오류 토스트로 알린다.
import { useEffect, useRef, useState } from 'react';
import { Glyph, type GlyphName } from './Glyph';
import { useToast } from './Toast';

const SHOW_MS = 1600;

export function CopyButton({ text, label = '복사', className = 'ui-btn is-sm', icon }: { text: string; label?: string; className?: string; icon?: GlyphName }) {
  const toast = useToast();
  const [copied, setCopied] = useState(false);
  const timer = useRef(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    try {
      if (!navigator.clipboard) throw new Error('no clipboard');
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), SHOW_MS);
    } catch {
      toast('복사하지 못했어요. 주소를 길게 눌러 직접 복사해 주세요.', 'error');
    }
  };

  return (
    <button type="button" className={`${className} aq-copy-btn${copied ? ' is-copied' : ''}`} onClick={() => void copy()} aria-live="polite">
      {copied ? <><Glyph name="check" size={14} />복사됨</> : <>{icon && <Glyph name={icon} size={14} />}{label}</>}
    </button>
  );
}
