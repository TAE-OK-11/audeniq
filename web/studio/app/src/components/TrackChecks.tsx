// 올린 곡의 사전 품질 검사 결과 — 업로드가 끝나면 곡마다 바로 보여 준다.
// 고쳐야 하는 항목(빨강), 확인이 필요한 항목(주황)만 이유와 함께 보여 주고, 모두 통과하면 한 줄로 알린다.
import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { AssetChecks } from '../api/types';
import { correctionTarget } from '../lib/corrections';

const BAD = new Set(['CORRECTION_REQUIRED', 'BLOCKED']);
const POLL_MS = 3000;
const POLL_TRIES = 40;

export function TrackChecks({ assetId }: { assetId: string }) {
  const [data, setData] = useState<AssetChecks | null>(null);
  useEffect(() => {
    let stop = false;
    let tries = 0;
    let timer = 0;
    const tick = async () => {
      try {
        const r = await api.assetChecks(assetId);
        if (stop) return;
        setData(r);
        // 서버 분석이 끝나기 전(아직 결과 없음)이면 잠시 뒤 다시 본다
        if ((r.qc_status === 'PENDING' || !r.checks.length) && ++tries < POLL_TRIES) timer = window.setTimeout(tick, POLL_MS);
      } catch {
        if (!stop && ++tries < POLL_TRIES) timer = window.setTimeout(tick, POLL_MS * 2);
      }
    };
    void tick();
    return () => { stop = true; window.clearTimeout(timer); };
  }, [assetId]);

  if (!data) return null;
  if (!data.checks.length) {
    return data.qc_status === 'PENDING' ? <p className="ui-field-help aq-track-qc">음원 품질을 검사하고 있어요…</p> : null;
  }
  const issues = data.checks.filter(c => BAD.has(c.status) || c.status === 'REVIEW_REQUIRED');
  if (!issues.length) return <p className="ui-field-help aq-track-qc is-ok">음원 품질 검사를 통과했어요.</p>;
  return (
    <ul className="aq-track-qc-list" aria-label="음원 품질 검사 결과">
      {issues.map(c => (
        <li key={c.code} className={BAD.has(c.status) ? 'is-bad' : 'is-warn'}>
          <strong>{BAD.has(c.status) ? '고쳐야 해요' : '확인해 주세요'}</strong>
          <span>{correctionTarget(c.code).hint}</span>
          {c.detail && <small>{c.detail}</small>}
        </li>
      ))}
    </ul>
  );
}
