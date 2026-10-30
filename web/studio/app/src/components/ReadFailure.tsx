import { useState } from 'react';

export function ReadFailure({ label, retry }: { label: string; retry: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  return <section className="ui-card" role="alert">
    <p>{label} 정보를 불러오지 못했어요.</p>
    <button type="button" className={`ui-btn${busy ? ' is-busy' : ''}`} disabled={busy} onClick={async () => {
      setBusy(true);
      try { await retry(); } finally { setBusy(false); }
    }}>{busy ? '불러오는 중' : '다시 시도'}</button>
  </section>;
}
