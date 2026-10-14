// 정산·지급 > 공동작업 분배로 받는 곡 — 다른 분의 발매에서 수락한 수익 분배와 지금까지 받은 금액.
import { useEffect, useState } from 'react';
import { orgPath, req } from '../api/http';
import { MOCK } from '../lib/mode';

interface Item { id: string; title: string; artist: string | null; track_title: string; from: string; share_bps: number; received: string }

export function SplitsReceived() {
  const [items, setItems] = useState<Item[] | null>(null);
  useEffect(() => {
    if (MOCK) return;
    req<{ items: Item[] }>(orgPath('/splits/received'), { quiet401: true }).then(v => setItems(v.items), () => setItems(null));
  }, []);
  if (!items?.length) return null;
  return (
    <section className="ui-card aq-splits-received" aria-labelledby="splitsReceivedHeading">
      <h2 id="splitsReceivedHeading">공동작업 분배로 받는 곡</h2>
      <p className="ui-field-help">다른 분의 발매에서 수락한 수익 분배예요. 정산될 때마다 내 잔액에 들어와요.</p>
      <ul className="ui-list">
        {items.map(i => (
          <li key={i.id} className="ui-row">
            <div className="min-0">
              <span className="ui-row-title">{i.track_title}</span>
              <span className="ui-row-meta"><span>{i.title}{i.artist ? ` · ${i.artist}` : ''}</span><span>{i.from}</span></span>
            </div>
            <div className="aq-statement-end">
              <span className="small muted">{(i.share_bps / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}%</span>
              <strong>{Number(i.received).toLocaleString()}원</strong>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
