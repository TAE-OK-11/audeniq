import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';

interface RightsDoc {
  id: string;
  title: string;
  releaseTitle: string;
  kind: string;
  reviewStatus: 'awaiting_documents' | 'review' | 'prepared' | 'approved' | 'needs';
  created: string;
  fileName: string;
  reviewNote: string;
  version: string;
}

const REQUIRED_DOCS: [string, string, string][] = [
  ['master', '마스터 음원 권리 확인서', '본인은 해당 마스터 음원에 관한 배급 권한을 보유하거나 권리자로부터 적법한 이용 허락을 받았음을 확인합니다.'],
  ['artwork', '커버아트 이용 허락서', '본인은 제출하는 앨범 커버의 사진·이미지·디자인·폰트를 디지털 음원 배급 및 홍보에 사용할 권한이 있음을 확인합니다.'],
  ['performer', '피처링·실연자 이용 허락서', '본인은 참여 실연자와 피처링 아티스트의 실연 녹음 및 디지털 배급 관련 허락을 확보했음을 확인합니다.'],
  ['composition', '작사·작곡 및 커버곡 이용 허락서', '본인은 타인의 작사·작곡 저작물이나 커버곡이 포함된 경우 디지털 배급에 필요한 적법한 허락을 확보했음을 확인합니다.'],
  ['sample', '샘플·제3자 음원 이용 허락서', '본인은 샘플링된 녹음물 및 관련 저작물의 이용에 필요한 권리자 동의를 확보했음을 확인합니다.'],
  ['custom', '기타 요청 서류', '요청받은 서류의 명칭과 해당 발매에 필요한 권리 범위를 확인해 주세요.'],
];

const MOCK_RELEASES = [
  { id: 'r1', title: '첫 번째 싱글' },
  { id: 'r2', title: '여름 EP' },
  { id: 'r3', title: '데모 트랙' },
];

const INITIAL: RightsDoc[] = [
  { id: 'd1', title: '첫 번째 싱글 · 마스터 음원 권리 확인서', releaseTitle: '첫 번째 싱글', kind: 'master', reviewStatus: 'review', created: '2026-09-22', fileName: '', reviewNote: '', version: '1.0' },
  { id: 'd2', title: '여름 EP · 작사·작곡 및 커버곡 이용 허락서', releaseTitle: '여름 EP', kind: 'composition', reviewStatus: 'needs', created: '2026-09-23', fileName: 'credit_proof.pdf', reviewNote: '서명란에 서명자 이름이 빠져 있어요. 보완 후 다시 제출해 주세요.', version: '1.0' },
  { id: 'd3', title: '데모 트랙 · 커버아트 이용 허락서', releaseTitle: '데모 트랙', kind: 'artwork', reviewStatus: 'awaiting_documents', created: '2026-09-24', fileName: '', reviewNote: '', version: '1.0' },
];

function docState(c: RightsDoc): string {
  if (c.reviewStatus === 'approved') return '검토 완료';
  if (c.reviewStatus === 'needs') return '보완 요청';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return '검토 중';
  return '서류 접수 대기';
}

function DocCard({ c, onOpen }: { c: RightsDoc; onOpen: (id: string) => void }) {
  const state = docState(c);
  const tone = c.reviewStatus === 'needs' ? 'is-needs'
    : c.reviewStatus === 'approved' ? 'is-approved'
    : (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') ? 'is-review' : '';
  return (
    <article className={`aq-doc-card ${tone}`}>
      <span className="document-icon" aria-hidden="true">▤</span>
      <div className="aq-doc-copy">
        <button type="button" className="row-name" onClick={() => onOpen(c.id)}>{c.title}</button>
        <span className="row-sub">{c.releaseTitle || '공통 문서'} · {c.created}</span>
        <div className="aq-doc-state-line">
          <span className="aq-doc-state-pill">{state}</span>
          {c.fileName && <span>{c.fileName}</span>}
          {c.reviewNote && <span>{c.reviewNote}</span>}
        </div>
      </div>
      <button className="button secondary" type="button" onClick={() => onOpen(c.id)}>
        {c.reviewStatus === 'needs' ? '보완하기' : '자세히 보기'}
      </button>
    </article>
  );
}

export function Rights() {
  const navigate = useNavigate();
  const toast = useToast();
  const [docs, setDocs] = useState<RightsDoc[]>(INITIAL);
  const [formOpen, setFormOpen] = useState(false);
  const [releaseId, setReleaseId] = useState('');
  const [kind, setKind] = useState(REQUIRED_DOCS[0][0]);
  const [docName, setDocName] = useState(REQUIRED_DOCS[0][1]);
  const [fileName, setFileName] = useState('');

  const needed = docs.filter(c => !['review', 'prepared', 'approved'].includes(c.reviewStatus)).length;
  const review = docs.filter(c => ['review', 'prepared'].includes(c.reviewStatus)).length;
  const fix = docs.filter(c => c.reviewStatus === 'needs').length;

  const onKindChange = (v: string) => {
    setKind(v);
    const item = REQUIRED_DOCS.find(x => x[0] === v);
    if (item) setDocName(item[1]);
  };

  const submitForm = (e: React.FormEvent) => {
    e.preventDefault();
    const r = MOCK_RELEASES.find(x => x.id === releaseId);
    if (!r) { toast('관련 발매를 선택해 주세요.'); return; }
    if (!docName.trim()) return;
    const item = REQUIRED_DOCS.find(x => x[0] === kind) || REQUIRED_DOCS[0];
    const c: RightsDoc = {
      id: 'd' + Date.now(),
      title: r.title + ' · ' + docName.trim(),
      releaseTitle: r.title,
      kind,
      reviewStatus: 'awaiting_documents',
      created: new Date().toISOString().slice(0, 10),
      fileName,
      reviewNote: '',
      version: '1.0',
    };
    setDocs(ds => [c, ...ds]);
    setFormOpen(false);
    setReleaseId('');
    setKind(REQUIRED_DOCS[0][0]);
    setDocName(REQUIRED_DOCS[0][1]);
    setFileName('');
    toast('서류를 접수했어요.');
    void item;
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">RIGHTS &amp; DOCUMENTS</p>
          <h1>권리·보완 서류</h1>
          <p>발매별 권리 증빙과 AUDENIQ의 보완 요청을 한곳에서 처리하세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={() => setFormOpen(true)}>
          권리 서류 제출 ↗
        </button>
      </div>

      <div className="aq-rights-summary">
        <div><span>제출할 서류</span><strong id="rightsNeededCount">{needed}</strong></div>
        <div><span>검토 중</span><strong id="rightsReviewCount">{review}</strong></div>
        <div><span>보완 요청</span><strong id="rightsFixCount">{fix}</strong></div>
      </div>

      <div id="rightsList">
        {docs.length ? (
          <div className="aq-doc-grid">
            {docs.map(c => (
              <DocCard key={c.id} c={c} onOpen={() => navigate('/contracts')} />
            ))}
          </div>
        ) : (
          <div className="empty-page">
            <p>제출할 권리 서류가 없어요.</p>
            <span>보완 자료가 필요하면 요청 내용과 제출 항목이 여기에 표시돼요.</span>
          </div>
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        보완 요청이 있으면 요청 사유와 필요한 서류를 확인한 뒤 새 원본을 제출해 주세요.
      </div>

      {formOpen && (
        <Modal title="권리 서류 접수" onClose={() => setFormOpen(false)}>
          <p className="small muted">
            발매를 선택하고 필요한 서류를 등록해 주세요. 확인할 내용과 첨부한 원본을 하나의 서류로 관리할 수 있어요.
          </p>
          <form onSubmit={submitForm} style={{ marginTop: 16 }}>
            <div className="field">
              <label htmlFor="aqRequiredRelease">관련 발매</label>
              <select
                id="aqRequiredRelease" required
                value={releaseId} onChange={e => setReleaseId(e.target.value)}
              >
                <option value="">발매를 선택해 주세요</option>
                {MOCK_RELEASES.map(r => (
                  <option key={r.id} value={r.id}>{r.title}</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor="aqRequiredKind">서류 종류</label>
              <select
                id="aqRequiredKind"
                value={kind} onChange={e => onKindChange(e.target.value)}
              >
                {REQUIRED_DOCS.map(x => (
                  <option key={x[0]} value={x[0]}>{x[1]}</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor="aqRequiredTitle">서류 이름</label>
              <input
                id="aqRequiredTitle" maxLength={160} required
                value={docName} onChange={e => setDocName(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="aqRequiredFile">증빙 원본 (필요 시)</label>
              <input
                type="file" id="aqRequiredFile"
                accept=".pdf,.txt,image/png,image/jpeg,image/webp,application/pdf,text/plain"
                onChange={e => setFileName(e.target.files?.[0]?.name || '')}
              />
              <p className="help">원본 첨부 전에는 '서류 접수 대기'로 표시돼요.</p>
            </div>
            <button type="submit" className="button studio-submit-wide">서류 접수하기</button>
          </form>
        </Modal>
      )}
    </>
  );
}
