import { useEffect, useState } from 'react';

interface DocRecord {
  id: string;
  kind: 'agreements' | 'rights';
  title: string;
  releaseTitle: string;
  version: string;
  created: string;
  fileName: string;
  content: string;
  checkedAt: string;
  reviewStatus: string;
}

// mock 배급 신청·계약서 — 라이브의 자동 생성 문서와 동일한 형식
const AGREEMENT_CONTENT = [
  'AUDENIQ 디지털 음원 배급 신청·계약서',
  '',
  '1. 신청인 및 발매 정보',
  '아티스트: 서린',
  '발매명: 첫 번째 싱글',
  '발매 유형: 싱글',
  '발매 희망일: 2026-10-01',
  '레이블 표기: AUDENIQ',
  '수록곡: 1. 첫 번째 싱글 / 2. 첫 번째 싱글 (Inst.)',
  '',
  '2. 권리자 및 배급 범위',
  '마스터 권리자: 서린',
  '℗ 표기: 2026 서린',
  '© 표기: 2026 서린',
  '배급 지역: 전 세계',
  '배급 플랫폼: Spotify, Apple Music, YouTube Music',
  '추가 확인 항목: 일반 발매',
  '',
  '3. 신청인의 확인',
  '신청인은 제출한 음원, 가사, 커버아트, 크레딧 및 메타데이터를 배급할 적법한 권한을 보유하고 있으며, 제3자의 권리가 포함된 경우 필요한 허락을 확보했음을 확인합니다.',
  '',
  '4. AUDENIQ 배급 계약',
  '신청인은 위 발매 정보를 기준으로 AUDENIQ에 디지털 음원 배급을 신청하고, 계약서에 안내된 배급 범위·정산·수정·테이크다운 및 권리 보증 조항에 동의합니다. 보완이 필요한 경우 AUDENIQ는 관련 자료를 요청할 수 있습니다.',
  '',
  '위 신청 정보는 자동으로 작성됐습니다. 내용을 확인한 뒤 신청인 서명만 진행해 주세요.',
].join('\n');

const INITIAL_DOCS: DocRecord[] = [
  {
    id: 'doc1',
    kind: 'agreements',
    title: '첫 번째 싱글 · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseTitle: '첫 번째 싱글',
    version: '1.0',
    created: '2026-09-20',
    fileName: '',
    content: AGREEMENT_CONTENT,
    checkedAt: '2026-09-20 14:32',
    reviewStatus: 'review',
  },
  {
    id: 'doc2',
    kind: 'agreements',
    title: '여름 EP · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseTitle: '여름 EP',
    version: '1.0',
    created: '2026-09-22',
    fileName: '',
    content: '',
    checkedAt: '',
    reviewStatus: 'awaiting_signature',
  },
  {
    id: 'doc3',
    kind: 'rights',
    title: '피처링 이용 허락서',
    releaseTitle: '여름 EP',
    version: '1.0',
    created: '2026-09-23',
    fileName: 'feature_agreement.pdf',
    content: '',
    checkedAt: '',
    reviewStatus: 'prepared',
  },
];

function docState(c: DocRecord): string {
  if (c.reviewStatus === 'prepared') return '제출 대기';
  if (c.fileName) return '원본 등록';
  return '내용 확인';
}

function niceDate(d: string): string {
  return d;
}

export function Contracts() {
  const [tab, setTab] = useState<'agreements' | 'rights'>('agreements');
  const [docs, setDocs] = useState<DocRecord[]>(INITIAL_DOCS);
  const [openId, setOpenId] = useState<string | null>(null);
  const [checked, setChecked] = useState(false);

  const records = docs.filter(c => c.kind === tab);
  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;

  useEffect(() => {
    if (openDoc) {
      setChecked(!!openDoc.checkedAt);
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = '';
    }
    return () => {
      document.body.style.overflow = '';
    };
  }, [openId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpenId(null);
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  const saveCheck = () => {
    if (!openDoc) return;
    if (!checked) return;
    const stamp = new Date().toLocaleString('ko-KR', {
      year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit',
    });
    setDocs(ds => ds.map(d => (d.id === openDoc.id ? { ...d, checkedAt: stamp } : d)));
    setOpenId(null);
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ AGREEMENTS</p>
          <h1>계약서</h1>
          <p>AUDENIQ과 체결하는 배급 계약과 발매 신청서를 확인하고 서명하세요.</p>
        </div>
      </div>

      <div className="studio-doc-path">
        <div>
          <span className="studio-path-n">01</span>
          <strong>신청 정보 확인</strong>
          <small>발매·권리자 정보 자동 정리</small>
        </div>
        <div>
          <span className="studio-path-n">02</span>
          <strong>계약 내용 확인</strong>
          <small>배급 범위와 필수 조항 확인</small>
        </div>
        <div>
          <span className="studio-path-n">03</span>
          <strong>서명·접수</strong>
          <small>서명 후 AUDENIQ에 즉시 접수</small>
        </div>
      </div>

      <div className="tabs" role="tablist" aria-label="계약서 메뉴">
        <button
          type="button"
          className="tab"
          data-contract-tab="agreements"
          aria-selected={tab === 'agreements'}
          onClick={() => setTab('agreements')}
        >
          계약서
        </button>
        <button
          type="button"
          className="tab"
          data-contract-tab="rights"
          aria-selected={tab === 'rights'}
          onClick={() => setTab('rights')}
        >
          권리 증빙
        </button>
      </div>

      <div id="contractList">
        {records.length ? (
          <div className="data-list">
            {records.map(c => (
              <div key={c.id} className="doc-row studio-document-row">
                <span className="document-icon" aria-hidden="true">▤</span>
                <div className="min-0">
                  <button className="row-name" type="button" onClick={() => setOpenId(c.id)}>
                    {c.title}
                  </button>
                  <span className="row-sub">
                    {c.releaseTitle || '공통 문서'} · {c.version ? `v${c.version} · ` : ''}{niceDate(c.created)}
                  </span>
                  <span className="row-sub">
                    {docState(c)} · {c.checkedAt ? `내용 확인: ${c.checkedAt}` : '내용 확인 전'}
                    {c.fileName ? ` · 첨부: ${c.fileName}` : ''}
                  </span>
                </div>
                <div className="row-actions">
                  <span className={`studio-doc-state${c.checkedAt ? ' is-checked' : ''}`}>
                    {c.checkedAt ? '확인 완료' : '확인 전'}
                  </span>
                  <button className="button secondary" type="button" onClick={() => setOpenId(c.id)}>
                    내용 보기
                  </button>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-page">
            <p>{tab === 'agreements' ? '등록된 계약서가 없어요.' : '등록된 권리 증빙이 없어요.'}</p>
            <span>문서를 등록하고 발매별로 분류·확인할 수 있어요.</span>
          </div>
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        발매 신청 정보는 계약서에 자동으로 정리돼요. 내용을 확인한 뒤 서명만 진행하면 돼요.
      </div>

      {openDoc && (
        <div className="modal" id="modal" onClick={e => { if (e.target === e.currentTarget) setOpenId(null); }}>
          <section className="modal-inner" role="dialog" aria-modal="true" aria-labelledby="modalTitle">
            <div className="modal-top">
              <h2 id="modalTitle">{openDoc.title}</h2>
              <button className="icon-button" type="button" aria-label="닫기" onClick={() => setOpenId(null)}>×</button>
            </div>
            <div>
              <div className="doc-meta">
                <span className="doc-pill">{openDoc.kind === 'agreements' ? '계약서' : '권리 증빙'}</span>
                <span className="doc-pill">{docState(openDoc)}</span>
              </div>
              <div className="information">
                <div><dt>관련 발매</dt><dd>{openDoc.releaseTitle || '공통'}</dd></div>
                <div><dt>등록일</dt><dd>{openDoc.created}</dd></div>
                <div><dt>문서 버전</dt><dd>{openDoc.version || '1.0'}</dd></div>
                <div><dt>첨부 파일</dt><dd>{openDoc.fileName || '없음'}</dd></div>
              </div>
              <h3 className="doc-section-title">문서 내용</h3>
              {openDoc.content ? (
                <div className="doc-content" style={{ whiteSpace: 'pre-wrap' }}>{openDoc.content}</div>
              ) : (
                <div className="doc-loading">첨부 파일을 확인하고 있어요.</div>
              )}
              <h3 className="doc-section-title">확인 및 동의 내역</h3>
              {openDoc.checkedAt ? (
                <div className="doc-audit">
                  <strong>내용 확인 기록</strong>
                  <span>{openDoc.checkedAt}</span>
                  <span>문서 버전: {openDoc.version || '1.0'}</span>
                </div>
              ) : (
                <p className="muted small">아직 내용 확인 기록이 없어요.</p>
              )}
              <label className="check-line doc-consent">
                <input
                  id="docCheck"
                  type="checkbox"
                  checked={checked}
                  onChange={e => setChecked(e.target.checked)}
                />
                <span>
                  위 문서의 내용을 읽고 확인했어요.
                  <small>확인 시간을 기록합니다. 법적 전자서명이나 AUDENIQ의 계약 승인·권리 심사를 대신하지 않아요.</small>
                </span>
              </label>
              <div className="doc-connection">
                현재 서류 접수 시스템과 연결되지 않았어요. 아래의 제출 준비는 AUDENIQ 담당자에게 전달되지 않아요.
              </div>
              <div className="doc-actions">
                <button className="button" type="button" onClick={saveCheck} disabled={!checked}>
                  확인 기록 저장
                </button>
                <button className="button secondary" type="button" disabled={!checked}>
                  검토 요청 준비
                </button>
              </div>
            </div>
          </section>
        </div>
      )}
    </>
  );
}
