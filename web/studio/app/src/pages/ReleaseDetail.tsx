import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { mockApi } from '../api/mock';
import type { ReleaseDetail as RD } from '../api/client';

const STATUS_MAP: Record<string, { chip: string; label: string }> = {
  LIVE: { chip: 'live', label: '발매 완료' },
  STAGE1_PASSED: { chip: 'review', label: '심사 통과' },
  STAGE1_CORRECTION: { chip: 'needs', label: '수정 필요' },
  DRAFT: { chip: 'draft', label: '초안' },
};

const DETAIL_TABS = [
  { value: 'overview', label: '기본 정보' },
  { value: 'tracks', label: '트랙·파일' },
  { value: 'delivery', label: '배급·권리' },
  { value: 'history', label: '변경 기록' },
];

function fmtDuration(ms: number | null): string {
  if (!ms) return '-';
  return `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;
}

export function ReleaseDetail() {
  const { id } = useParams();
  const nav = useNavigate();
  const [rel, setRel] = useState<RD | null>(null);
  const [error, setError] = useState('');
  const [tab, setTab] = useState('overview');

  useEffect(() => {
    if (!id) return;
    mockApi.getRelease(id).then(setRel).catch(e => setError(e.message));
  }, [id]);

  if (error) return <div className="notice error">{error}</div>;
  if (!rel) return <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>;

  const st = STATUS_MAP[rel.status] ?? { chip: 'draft', label: rel.status };

  return (
    <>
      <div className="spaced-actions">
        <button type="button" className="link-btn" onClick={() => nav('/releases')}>← 발매 목록</button>
        <div className="row-actions" />
      </div>

      <div className="view-title">
        <div>
          <p className="eyebrow">RELEASE DETAIL</p>
          <h1>발매 상세</h1>
          <p>{rel.title} · {rel.track_count}곡</p>
        </div>
        <div><span className={`status-chip ${st.chip}`}>{st.label}</span></div>
      </div>

      <div className="studio-album-hero" aria-label="앨범 아트워크 및 발매 정보">
        <span className="cover" aria-hidden="true">{rel.title.charAt(0)}</span>
        <div>
          <p className="eyebrow" style={{ marginBottom: 6 }}>{st.label.toUpperCase()}</p>
          <h2 style={{ fontSize: 24, letterSpacing: '-.04em' }}>{rel.title}</h2>
          <p style={{ marginTop: 8, fontSize: 13 }}>
            테스트 레이블 · {rel.track_count}곡 · {rel.release_date ?? '발매일 미정'}
          </p>
        </div>
      </div>

      <div className="tabs" role="tablist" aria-label="발매 상세 메뉴">
        {DETAIL_TABS.map(t => (
          <button
            key={t.value} type="button" className="tab" role="tab"
            aria-selected={tab === t.value}
            onClick={() => setTab(t.value)}
          >{t.label}</button>
        ))}
      </div>

      {tab === 'overview' && (
        <section className="surface">
          <div className="section-top"><h2>기본 정보</h2></div>
          <div className="data-list">
            {[
              ['발매명', rel.title],
              ['아티스트', '테스트 레이블'],
              ['발매일', rel.release_date ?? '미정'],
              ['트랙 수', `${rel.track_count}곡`],
              ['등록일', rel.created_at],
            ].map(([k, v]) => (
              <div key={k} className="track-row">
                <div><span className="row-sub">{k}</span></div>
                <div><span className="row-name">{v}</span></div>
                <div />
              </div>
            ))}
          </div>
        </section>
      )}

      {tab === 'tracks' && (
        <section className="surface">
          <div className="section-top"><h2>트랙·파일</h2></div>
          {rel.tracks.length ? (
            <div className="data-list">
              {rel.tracks.map((t, i) => (
                <div key={t.id} className="track-row">
                  <span className="cover cover-small" aria-hidden="true">{i + 1}</span>
                  <div>
                    <span className="row-name">{t.title}</span>
                    <span className="row-sub">{t.isrc ? `ISRC ${t.isrc}` : 'ISRC 미발급'} · {fmtDuration(t.duration_ms)}</span>
                  </div>
                  <div className="row-end" />
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-page">
              <h3>등록된 트랙이 없어요.</h3>
              <p>발매를 수정해서 트랙을 추가해 보세요.</p>
            </div>
          )}
        </section>
      )}

      {tab === 'delivery' && (
        <section className="surface">
          <div className="section-top"><h2>배급·권리</h2></div>
          <div className="notice">
            배급 채널과 권리 정보는 심사 단계에서 확인할 수 있어요. (테스트 화면)
          </div>
          <div className="data-list" style={{ marginTop: 16 }}>
            {['Spotify', 'Apple Music', 'Melon', 'YouTube Music'].map(p => (
              <div key={p} className="track-row">
                <div><span className="row-name">{p}</span></div>
                <div><span className="row-sub">배급 채널</span></div>
                <div className="row-end"><span className="status-chip ready">준비 중</span></div>
              </div>
            ))}
          </div>
        </section>
      )}

      {tab === 'history' && (
        <section className="surface">
          <div className="section-top"><h2>변경 기록</h2></div>
          <div className="data-list">
            <div className="track-row">
              <div><span className="row-name">발매 정보 등록</span></div>
              <div><span className="row-sub">{rel.created_at}</span></div>
              <div />
            </div>
            <div className="track-row">
              <div><span className="row-name">상태 변경: {st.label}</span></div>
              <div><span className="row-sub">{rel.created_at}</span></div>
              <div />
            </div>
          </div>
        </section>
      )}

      <div style={{ marginTop: 24 }}>
        <Link className="link-btn" to="/releases">← 발매 목록으로 돌아가기</Link>
      </div>
    </>
  );
}
