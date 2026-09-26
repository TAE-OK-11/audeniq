import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { mockApi } from '../api/mock';
import type { ReleaseDetail as RD } from '../api/client';
import { useToast } from '../components/Toast';
import { Modal } from '../components/Modal';
import { STATUS_LABEL } from '../lib/format';

const KINDS: [string, string][] = [
  ['single', '싱글'], ['ep', 'EP'], ['album', '정규 앨범'], ['compilation', '컴필레이션'],
];

const DSP: [string, string][] = [
  ['melon', '멜론'], ['genie', '지니'], ['flo', 'FLO'], ['bugs', '벅스'],
  ['spotify', 'Spotify'], ['apple', 'Apple Music / iTunes'], ['youtube', 'YouTube Music'],
  ['amazon', 'Amazon Music'], ['tidal', 'TIDAL'], ['deezer', 'Deezer'], ['qobuz', 'Qobuz'],
];

const RIGHTS_KEYS = ['rightsMaster', 'rightsComposition', 'rightsArtwork', 'rightsConsent'];

const DETAIL_TABS = [
  { value: 'overview', label: '기본 정보' },
  { value: 'tracks', label: '트랙·파일' },
  { value: 'delivery', label: '배급·권리' },
  { value: 'history', label: '변경 기록' },
];

const COVER_GRADIENTS = [
  'linear-gradient(135deg,#bcd8ff,#7162db 65%,#394b83)',
  'linear-gradient(135deg,#ffd9c1,#e58a7a 65%,#8a3d4b)',
  'linear-gradient(135deg,#bdf0d9,#4dae8a 65%,#2a6b52)',
  'linear-gradient(135deg,#ecd9ff,#a97ae5 65%,#5b3d8a)',
  'linear-gradient(135deg,#fff0b8,#e0a83f 65%,#8a6a2a)',
  'linear-gradient(135deg,#b8e6ff,#5c9ae5 65%,#2a4d8a)',
  'linear-gradient(135deg,#ffcfe3,#e57aa5 65%,#8a3d63)',
];

function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

function rightsOk(checks: Record<string, boolean> | undefined, options?: { sample?: boolean; featured?: boolean; ai?: boolean; shared?: boolean; rerelease?: boolean }): boolean {
  if (!RIGHTS_KEYS.every(k => checks?.[k])) return false;
  if (options?.sample || options?.featured) { if (!checks?.['rightsSamples']) return false; }
  if (options?.ai) { if (!checks?.['rightsAi']) return false; }
  if (options?.shared) { if (!checks?.['rightsShared']) return false; }
  if (options?.rerelease) { if (!checks?.['rightsRerelease']) return false; }
  return true;
}

export function ReleaseDetail() {
  const { id } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const [rel, setRel] = useState<RD | null>(null);
  const [error, setError] = useState('');
  const [tab, setTab] = useState('overview');
  const [deleteOpen, setDeleteOpen] = useState(false);

  useEffect(() => {
    if (!id) return;
    mockApi.getRelease(id).then(setRel).catch(e => setError(e.message));
  }, [id]);

  if (error) return <div className="notice error">{error}</div>;
  if (!rel) return <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>;

  const d = rel.draft;
  const isDraft = rel.status === 'draft';
  const editLabel = isDraft ? '계속 작성' : rel.status === 'live' ? '수정 요청 준비' : '수정하기';
  const kindLabel = KINDS.find(k => k[0] === d?.type)?.[1] || '앨범';

  const handleDelete = async () => {
    setDeleteOpen(false);
    await mockApi.deleteRelease(rel.id);
    toast('발매를 삭제했어요.');
    nav('/releases');
  };

  return (
    <div id="view-release" className="view">
      <div className="spaced-actions">
        <button type="button" className="link-btn" onClick={() => nav('/releases')}>← 발매 목록</button>
        <div className="row-actions">
          <button
            type="button"
            className="button secondary"
            onClick={() => nav(`/upload?edit=${rel.id}`)}
          >
            {editLabel}
          </button>
          {isDraft && (
            <button
              type="button"
              className="button danger"
              onClick={() => setDeleteOpen(true)}
            >
              삭제
            </button>
          )}
        </div>
      </div>

      <div className="aq-detail-hero" aria-label="발매 정보">
        <span className="cover aq-detail-cover" aria-hidden="true" style={{ background: gradientFor(rel.id) }}>♫</span>
        <div className="aq-detail-info">
          <div className="aq-detail-top">
            <span className="eyebrow">{kindLabel}</span>
            <span className={`status-chip ${rel.status}`}>{STATUS_LABEL[rel.status] || rel.status}</span>
          </div>
          <h1>{rel.title || '제목 없는 발매'}</h1>
          <p className="aq-detail-artist">{rel.artist || '아티스트 미입력'}</p>
          <div className="aq-detail-facts">
            <span>{rel.tracks.length}곡</span>
            <span>{d?.genre || '장르 미등록'}</span>
            <span>{rel.release_date || '발매일 미정'}</span>
          </div>
        </div>
      </div>

      <div className="tabs" role="tablist" aria-label="발매 상세 메뉴">
        {DETAIL_TABS.map(t => (
          <button
            key={t.value} type="button" className="tab" data-release-tab={t.value}
            aria-selected={tab === t.value}
            onClick={() => setTab(t.value)}
          >{t.label}</button>
        ))}
      </div>

      <div id="releaseDetailBody">
        {tab === 'overview' && (
          <div className="split">
            <div>
              <h2 className="subhead">발매 정보</h2>
              <dl className="information">
                <div><dt>아티스트</dt><dd>{rel.artist || ''}</dd></div>
                <div><dt>발매 유형</dt><dd>{d?.type ? (KINDS.find(k => k[0] === d.type)?.[1] || d.type) : ''}</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
                <div><dt>UPC / EAN</dt><dd>{d?.upc || '등록 전'}</dd></div>
                <div><dt>장르</dt><dd>{d?.genre || '미입력'}</dd></div>
                <div><dt>레이블</dt><dd>{d?.label || '미입력'}</dd></div>
              </dl>
              <h2 className="subhead">앨범 소개</h2>
              <p className="muted small break">{d?.notes || '등록된 소개가 없어요.'}</p>
            </div>
            <div className="studio-album-aside">
              <h2 className="subhead">커버아트 정보</h2>
              <p className="small muted break">{d?.coverName || '커버아트 없음'}</p>
              <h2 className="subhead">진행 상태</h2>
              <span className={`status-chip ${rel.status}`}>{STATUS_LABEL[rel.status] || rel.status}</span>
              <p className="small muted" style={{ marginTop: 16 }}>
                상세 정보를 수정하려면 상단의 수정하기를 눌러 주세요.
              </p>
            </div>
          </div>
        )}

        {tab === 'tracks' && (
          <>
            <h2 className="subhead">트랙 목록 · {rel.tracks.length}곡</h2>
            <div className="aq-catalog-cards">
              {rel.tracks.map((t, i) => (
                <div key={t.id} className="aq-track-card">
                  <span className="aq-track-num" aria-hidden="true">{i + 1}</span>
                  <div className="min-0">
                    <span className="row-name">{t.title || '곡명 없음'}{t.version ? ` (${t.version})` : ''}</span>
                    <span className="row-sub">
                      ISRC {t.isrc || '등록 전'} · 작곡 {t.composers || '미입력'} · 작사 {t.lyricists || '없음'}
                    </span>
                    <span className="row-sub">
                      음원 파일: {t.audioName || '미등록'}
                    </span>
                  </div>
                  <span className={`status-chip ${t.audioName ? 'ready' : 'draft'}`}>{t.audioName ? '파일 등록' : '파일 없음'}</span>
                </div>
              ))}
            </div>
            <div className="notice" style={{ marginTop: 22 }}>
              음원 원본의 재첨부가 필요한 경우가 있어요. 제출 전 파일을 다시 확인해 주세요.
            </div>
          </>
        )}

        {tab === 'delivery' && (
          <div className="split">
            <div>
              <h2 className="subhead">배급 설정</h2>
              <dl className="information">
                <div><dt>희망 배급 지역</dt><dd>{d && d.territories.includes('WORLD') ? '전 세계' : '지정 안 함'}</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
              </dl>
              <div className="tag-row">
                {d?.platforms?.length ? (
                  d.platforms.map(p => <span key={p} className="tag">{DSP.find(x => x[0] === p)?.[1] || p}</span>)
                ) : (
                  <span className="muted small">플랫폼 미선택</span>
                )}
              </div>
              <h2 className="subhead">권리 확인</h2>
              <dl className="information">
                <div><dt>마스터 권리자</dt><dd>{d?.ownership || '미입력'}</dd></div>
                <div><dt>℗ 표기</dt><dd>{d?.phonogram || '미입력'}</dd></div>
                <div><dt>© 표기</dt><dd>{d?.copyright || '미입력'}</dd></div>
              </dl>
              <p className="small muted">{rightsOk(d?.rightsChecks, d?.options) ? '신청서 권리 확인 항목 작성 완료' : '권리 확인 항목을 보완해 주세요.'}</p>
            </div>
            <div className="surface">
              <h2 className="subhead">계약·증빙</h2>
              <p className="small muted">발매 관련 권리 증빙과 계약 문서를 관리해 보세요.</p>
              <button className="button secondary" type="button" onClick={() => nav('/contracts')}>문서 관리 ↗</button>
            </div>
          </div>
        )}

        {tab === 'history' && (
          <>
            <h2 className="subhead">변경 기록</h2>
            <div className="aq-catalog-cards">
              {d?.history?.length ? d.history.map((h, i) => (
                <div key={i} className="aq-track-card">
                  <span className="aq-track-num" aria-hidden="true">↗</span>
                  <span className="min-0">
                    <span className="row-name">{h.text}</span>
                    <span className="row-sub">{new Date(h.time).toLocaleString('ko-KR')}</span>
                  </span>
                </div>
              )) : (
                <div className="empty-note">등록된 변경 기록이 없어요.</div>
              )}
            </div>
          </>
        )}
      </div>

      {deleteOpen && (
        <Modal title="발매 삭제" onClose={() => setDeleteOpen(false)}>
          <p className="muted small">
            {rel.title} 발매를 현재 작업 공간의 카탈로그에서 삭제할까요? 이 작업은 되돌릴 수 없어요.
          </p>
          <div className="row-actions" style={{ marginTop: 24 }}>
            <button type="button" className="button danger" onClick={handleDelete}>
              삭제
            </button>
            <button
              type="button"
              className="button secondary"
              onClick={() => setDeleteOpen(false)}
            >
              취소
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
