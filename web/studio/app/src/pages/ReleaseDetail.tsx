import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { mockApi } from '../api/mock';
import type { ReleaseDetail as RD } from '../api/client';
import { useToast } from '../components/Toast';
import { Modal } from '../components/Modal';

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

  const st = STATUS_MAP[rel.status] ?? { chip: 'draft', label: rel.status };
  const isDraft = rel.status === 'DRAFT';

  const handleDelete = () => {
    // mock 삭제
    setDeleteOpen(false);
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
            {isDraft ? '계속 작성' : '수정하기'}
          </button>
          <button
            type="button"
            className="button danger"
            onClick={() => setDeleteOpen(true)}
          >
            삭제
          </button>
        </div>
      </div>

      <div className="view-title">
        <div>
          <p className="eyebrow">RELEASE DETAIL</p>
          <h1>{rel.title || '제목 없는 발매'}</h1>
          <p>테스트 레이블 · {rel.track_count}곡 · {rel.release_date || '발매일 미정'}</p>
        </div>
        <div><span className={`status-chip ${st.chip}`}>{st.label}</span></div>
      </div>

      <div className="studio-album-hero" aria-label="앨범 아트워크 및 발매 정보">
        <div className="studio-album-art">
          <span className="cover" aria-hidden="true">{rel.title.charAt(0)}</span>
        </div>
        <div className="studio-album-summary">
          <span className="eyebrow">싱글</span>
          <h2>{rel.title || '제목 없는 발매'}</h2>
          <p>테스트 레이블</p>
          <div className="studio-album-facts">
            <span>{rel.track_count}곡</span>
            <span>장르 미등록</span>
            <span>{rel.release_date || '발매일 미정'}</span>
          </div>
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

      <div id="releaseDetailBody">
        {tab === 'overview' && (
          <div className="split">
            <div>
              <h2 className="subhead">발매 정보</h2>
              <dl className="information">
                <div><dt>아티스트</dt><dd>테스트 레이블</dd></div>
                <div><dt>발매 유형</dt><dd>싱글</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
                <div><dt>UPC / EAN</dt><dd>등록 전</dd></div>
                <div><dt>장르</dt><dd>미입력</dd></div>
                <div><dt>레이블</dt><dd>미입력</dd></div>
              </dl>
              <h2 className="subhead">앨범 소개</h2>
              <p className="muted small break">등록된 소개가 없어요.</p>
            </div>
            <div className="studio-album-aside">
              <h2 className="subhead">커버아트 정보</h2>
              <p className="small muted break">커버아트 없음</p>
              <h2 className="subhead">진행 상태</h2>
              <span className={`status-chip ${st.chip}`}>{st.label}</span>
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
                    <span className="row-name">{t.title || '곡명 없음'}</span>
                    <span className="row-sub">
                      ISRC {t.isrc || '등록 전'} · 작곡 미입력 · 작사 없음
                    </span>
                    <span className="row-sub">
                      음원: 미등록 · 원본 파일 없음
                    </span>
                  </div>
                  <span className="status-chip draft">파일 없음</span>
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
                <div><dt>희망 배급 지역</dt><dd>전 세계</dd></div>
                <div><dt>발매 예정일</dt><dd>{rel.release_date || '미정'}</dd></div>
              </dl>
              <div className="tag-row">
                {['Spotify', 'Apple Music', 'YouTube Music', 'Melon'].map(p => (
                  <span key={p} className="tag">{p}</span>
                ))}
              </div>
              <h2 className="subhead">권리 확인</h2>
              <dl className="information">
                <div><dt>마스터 권리자</dt><dd>미입력</dd></div>
                <div><dt>℗ 표기</dt><dd>미입력</dd></div>
                <div><dt>© 표기</dt><dd>미입력</dd></div>
              </dl>
            </div>
            <div>
              <section className="surface">
                <div className="section-top">
                  <h2>계약·증빙</h2>
                </div>
                <p className="small muted">
                  배급 신청서와 권리 증빙은 계약서 메뉴에서 확인할 수 있어요.
                </p>
                <button
                  type="button"
                  className="link-btn"
                  style={{ marginTop: 12 }}
                  onClick={() => nav('/contracts')}
                >
                  문서 관리 ↗
                </button>
              </section>
            </div>
          </div>
        )}

        {tab === 'history' && (
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
        )}
      </div>

      {deleteOpen && (
        <Modal
          title="발매 삭제"
          onClose={() => setDeleteOpen(false)}
        >
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
