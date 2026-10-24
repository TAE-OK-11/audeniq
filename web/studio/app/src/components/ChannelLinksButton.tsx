// 채널별 링크 버튼 — 같은 스마트링크에 채널 표시(?src=)를 붙인 주소들. 최근 30일 채널별 방문 수와 함께.
import { useState } from 'react';
import { Modal } from './Modal';
import { Glyph } from './Glyph';
import { CopyButton } from './CopyButton';
import { CHANNELS, channelUrl } from '../lib/channels';

export function ChannelLinksButton({ url, views, days, className = 'ui-btn is-sm' }: { url: string; views: Record<string, number>; days: number; className?: string }) {
  const [open, setOpen] = useState(false);
  const total = Object.values(views).reduce((a, b) => a + b, 0);
  // 방문이 있는 채널부터
  const list = [...CHANNELS].sort((a, b) => (views[b.id] ?? 0) - (views[a.id] ?? 0));
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)}><Glyph name="chart" size={14} />채널별 링크</button>
      {open && (
        <Modal title="채널별 링크" onClose={() => setOpen(false)} modalClass="aq-set-modal aq-channel-modal">
          <p className="ui-field-help">올리는 곳마다 다른 주소를 쓰면 어디서 많이 들어왔는지 알 수 있어요. 열리는 페이지는 같아요.</p>
          <ul className="ui-list is-inset aq-channel-list">
            {list.map(c => {
              const n = views[c.id] ?? 0;
              return (
                <li key={c.id} className="aq-channel-row">
                  <span className="min-0">
                    <strong>{c.label}</strong>
                    <code>…{channelUrl(url, c.id).slice(url.length)}</code>
                  </span>
                  <span className="aq-channel-n" aria-label={`최근 ${days}일 방문 ${n}`}>{n > 0 ? n.toLocaleString() : '–'}</span>
                  <CopyButton text={channelUrl(url, c.id)} />
                </li>
              );
            })}
          </ul>
          <p className="small muted">최근 {days}일 채널 표시로 들어온 방문 {total.toLocaleString()}회 · 표시 없이 들어온 방문은 따로 세지 않아요.</p>
        </Modal>
      )}
    </>
  );
}
