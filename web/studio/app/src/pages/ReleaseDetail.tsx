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

const RIGHTS_KEYS = ['rightsMaster', 'rightsComposition', 'rightsArtwork', 'rightsSamples', 'rightsConsent'];

const DETAIL_TABS = [
  { value: 'overview', label: '기본 정보' },
  { value: 'tracks', label: '트랙·파일' },
  { value: 'delivery', label: '배급·권리' },
  { value: 'history', label: '변경 기록' },
];

function rightsOk(checks: Record<string, boolean> | undefined): boolean {
  return RIGHTS_KEYS.every(k => checks?.[k]);
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
    <>
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

      <div className="view-title">
        <div>
          <p className="eyebrow">RELEASE DETAIL</p>
          <h1>{rel.title || '제목 없는 발매'}</h1>
          <p>{rel.artist || '아티스트 미입력'} · {rel.tracks.length}곡 · {rel.release_date || '발매일 미정'}</p>
        </div>
        <div><span className={`status-chip ${rel.status}`}>{STATUS_LABEL[rel.status] || rel.status}</span></div>
      </div>

      <div className="studio-album-hero" aria-label="앨범 아트워크 및 발매 정보">
        <div className="studio-album-art">
          <span className="cover" aria-hidden="true">♫</span>
        </div>
        <div className="studio-album-summary">
          <span className="eyebrow">{kindLabel}</span>
          <h2>{rel.title || '제목 없는 발매'}</h2>
          <p>{rel.artist || '아티스트 미입력'}</p>
          <div className="studio-album-facts">
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
            <div className="data-list">
              {rel.tracks.map((t, i) => (
                <div key={t.id} className="track-row">
                  <div className="document-icon">{i + 1}</div>
                  <div>
                    <span className="row-name">{t.title || '곡명 없음'}{t.version ? ` (${t.version})` : ''}</span>
                    <span className="row-sub">
                      ISRC {t.isrc || '등록 전'} · 작곡 {t.composers || '미입력'} · 작사 {t.lyricists || '없음'}
                    </span>
                    <span className="row-sub">
                      음원: {t.audioName || '미등록'} · {t.sample ? '원본 파일 확인 필요' : t.audioName ? '파일 재첨부가 필요할 수 있어요' : '원본 파일 없음'}
                    </span>
                  </div>
                  <span className={`status-chip ${t.audioName ? 'ready' : 'draft'}`}>{t.audioName ? '파일명 등록' : '파일 없음'}</span>
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
              <p className="small muted">{rightsOk(d?.rightsChecks) ? '신청서 권리 확인 항목 작성 완료' : '권리 확인 항목을 보완해 주세요.'}</p>
            </div>
            <div className="surface white">
              <h2 className="subhead">계약·증빙</h2>
              <p className="small muted">발매 관련 권리 증빙과 계약 문서를 관리해 보세요.</p>
              <button className="button secondary" type="button" onClick={() => nav('/contracts')}>문서 관리 ↗</button>
            </div>
          </div>
        )}

        {tab === 'history' && (
          <>
            <h2 className="subhead">변경 기록</h2>
            <div className="data-list">
              {d?.history?.length ? d.history.map((h, i) => (
                <div key={i} className="statement-row">
                  <div className="document-icon">↗</div>
                  <div>
                    <span className="row-name">{h.text}</span>
                    <span className="row-sub">{new Date(h.time).toLocaleString('ko-KR')}</span>
                  </div>
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
    </>
  );
}
