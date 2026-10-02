// 권리·보완 서류 — 라이브 view-rights / renderRights / openRequiredDocForm 대응
import { FilePicker } from '../components/FilePicker';
import { useState } from 'react';
import { addDoc, useDocs, type DocRecord } from '../store/docs';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { refreshDocs } from '../store/portalSync';
import { DocCard, DocEmpty } from '../components/DocCard';
import { DocumentModal } from '../components/DocumentModal';
import { SignatureModal } from '../components/SignatureModal';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { api } from '../api/client';
import { useAsync } from '../hooks/useAsync';
import { stampNow } from '../lib/date';
import { uid } from '../lib/store';
import { Glyph } from '../components/Glyph';
import { RightsDocumentModal } from '../components/RightsDocumentModal';
import { RIGHTS_DOCUMENTS, type RightsDocumentContext, type RightsDocumentKind } from '../lib/rightsDocument';

const REQUIRED_DOCS: [string, string, string][] = [
  ['master', '마스터 음원 권리 확인서', '본인은 해당 마스터 음원에 관한 배급 권한을 보유하거나 권리자로부터 적법한 이용 허락을 받았음을 확인합니다.'],
  ['artwork', '커버아트 이용 허락서', '본인은 제출하는 앨범 커버의 사진·이미지·디자인·폰트를 디지털 음원 배급 및 홍보에 사용할 권한이 있음을 확인합니다.'],
  ['performer', '피처링·실연자 이용 허락서', '본인은 참여 실연자와 피처링 아티스트의 실연 녹음 및 디지털 배급 관련 허락을 확보했음을 확인합니다.'],
  ['composition', '작사·작곡 및 커버곡 이용 허락서', '본인은 타인의 작사·작곡 저작물이나 커버곡이 포함된 경우 디지털 배급에 필요한 적법한 허락을 확보했음을 확인합니다.'],
  ['sample', '샘플·제3자 음원 이용 허락서', '본인은 샘플링된 녹음물 및 관련 저작물의 이용에 필요한 권리자 동의를 확보했음을 확인합니다.'],
  ['shared', '공동 권리자 배급 위임 확인서', '공동 권리자의 배급 위임 범위를 확인합니다.'],
  ['custom', '기타 요청 서류', '요청받은 서류의 명칭과 해당 발매에 필요한 권리 범위를 확인해 주세요.'],
];

export function Rights() {
  const toast = useToast();
  const docs = useDocs();
  const { data: releases = [] } = useAsync(() => api.listReleases(), []);
  const [formOpen, setFormOpen] = useState(false);
  const [openId, setOpenId] = useState<string | null>(null);
  const [signing, setSigning] = useState(false);
  const [releaseId, setReleaseId] = useState('');
  const [kind, setKind] = useState(REQUIRED_DOCS[0][0]);
  const [docName, setDocName] = useState(REQUIRED_DOCS[0][1]);
  const [file, setFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [method, setMethod] = useState<'electronic' | 'upload'>('electronic');
  const [electronic, setElectronic] = useState<{ kind: RightsDocumentKind; context: RightsDocumentContext } | null>(null);

  const openDoc = openId ? docs.find(d => d.id === openId) ?? null : null;
  const rights = docs
    .filter(c => c.kind === 'rights')
    .slice()
    .sort((a, b) => String(b.created || '').localeCompare(String(a.created || '')));

  const needed = rights.filter(c => !['review', 'prepared', 'approved'].includes(c.reviewStatus)).length;
  const review = rights.filter(c => ['review', 'prepared'].includes(c.reviewStatus)).length;
  const fix = rights.filter(c => c.reviewStatus === 'needs').length;

  const onKindChange = (v: string) => {
    setKind(v);
    const item = REQUIRED_DOCS.find(x => x[0] === v);
    if (item) setDocName(item[1]);
  };

  const submitForm = async (e: React.FormEvent) => {
    e.preventDefault();
    const r = releases.find(x => x.id === releaseId);
    if (!r) { toast('관련 발매를 선택해 주세요.'); return; }
    const name = docName.trim();
    if (!name) return;
    const item = REQUIRED_DOCS.find(x => x[0] === kind) || REQUIRED_DOCS[0];
    const at = stampNow();
    setSubmitting(true);
    try {
      if (method === 'electronic' && kind in RIGHTS_DOCUMENTS) {
        const rel = await api.getRelease(r.id);
        const d = rel.draft;
        const tracks = d?.draftTracks ?? rel.tracks;
        const source = kind === 'composition' && d?.options?.cover
          ? d.options.coverTracks.map(c => `${c.originalTitle} / ${c.originalArtist}${c.originalWriters ? ` / ${c.originalWriters}` : ''}`).join('\n')
          : kind === 'sample' ? '' : undefined;
        setElectronic({ kind: kind as RightsDocumentKind, context: { releaseId: r.id, title: r.title, artist: d?.artist ?? r.artist ?? '',
          tracks: tracks.map(t => ({ title: t.title, isrc: t.isrc ?? '' })),
          territories: d?.territories ?? ['WORLD'], platforms: d?.platforms ?? [], source } });
        setFormOpen(false);
        return;
      }
      const c: DocRecord = {
        id: uid('d'),
        kind: 'rights',
        releaseId: r.id,
        releaseTitle: r.title,
        title: r.title + ' · ' + name,
        version: '1.0',
        content: [
          '서류 종류: ' + name,
          '관련 발매: ' + r.title,
          '아티스트: ' + (r.artist || '미입력'),
          '권리 확인 내용: ' + item[2],
          '제출 서류: ' + name,
        ].join('\n'),
        fileName: file ? file.name : '',
        fileBlob: file,
        fileType: file?.type,
        uploadedAt: file ? at : undefined,
        checked: false,
        checkedAt: '',
        created: at,
        consentHistory: [],
        reviewHistory: [{ status: '서류 접수 대기', time: at, detail: name + ' 접수 준비' }],
        reviewStatus: 'awaiting_documents',
        reviewNote: '',
        signerName: '',
        localSignatureData: '',
        localSignatureAt: '',
      };
      if (!MOCK) {
        if (file && !/\.(pdf|jpe?g|png)$/i.test(file.name)) { toast('서류는 PDF, JPG, PNG 파일로 올려 주세요.'); return; }
        c.id = await portal.createDocument(r.id, c.title, c.content, file);
        await refreshDocs();
      } else {
        addDoc(c);
      }
      setFormOpen(false);
      setReleaseId('');
      setKind(REQUIRED_DOCS[0][0]);
      setDocName(REQUIRED_DOCS[0][1]);
      setFile(null);
      // 라이브: 접수 후 바로 문서 모달을 연다
      setOpenId(c.id);
    } catch (err) {
      toast(MOCK ? '원본을 첨부할 수 없어요.' : errorMessage(err, '서류를 등록하지 못했어요. 잠시 후 다시 시도해 주세요.'));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div id="view-rights" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">RIGHTS &amp; DOCUMENTS</p>
          <h1>권리·보완 서류</h1>
          <p>발매별 권리 증빙과 AUDENIQ의 보완 요청을 한곳에서 처리하세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={() => setFormOpen(true)}>
          권리 서류 작성·제출 <Glyph name="arrow-up-right" size={14} />
        </button>
      </div>

      <div className="aq-rights-summary">
        <div><span>제출할 서류</span><strong id="rightsNeededCount">{needed}</strong></div>
        <div><span>검토 중</span><strong id="rightsReviewCount">{review}</strong></div>
        <div><span>보완 요청</span><strong id="rightsFixCount">{fix}</strong></div>
      </div>

      <div id="rightsList">
        {rights.length ? (
          <div className="aq-doc-grid aq-stagger">
            {rights.map(c => <DocCard key={c.id} c={c} onOpen={setOpenId} />)}
          </div>
        ) : (
          <DocEmpty
            title="제출할 권리 서류가 없어요."
            desc="보완 자료가 필요하면 요청 내용과 제출 항목이 여기에 표시돼요."
          />
        )}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        보완 요청이 있으면 요청 사유와 필요한 서류를 확인한 뒤 새 원본을 제출해 주세요.
      </div>

      {formOpen && (
        <Modal title="권리 서류 작성·접수" onClose={() => setFormOpen(false)} dismissible={false}>
          <p className="small muted">
            AUDENIQ에서 문서를 자동 작성하고 권리자가 서명하거나, 보유한 허락서를 첨부할 수 있어요.
          </p>
          <form id="aqRequiredDocForm" onSubmit={submitForm}>
            <div className="field">
              <label htmlFor="aqRequiredRelease">관련 발매</label>
              <select
                id="aqRequiredRelease" required
                value={releaseId} onChange={e => setReleaseId(e.target.value)}
              >
                <option value="">발매를 선택해 주세요</option>
                {releases.map(r => (
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
            {kind in RIGHTS_DOCUMENTS && <div className="aq-chips" role="group" aria-label="권리 서류 준비 방법">
              <button type="button" className={`aq-chip${method === 'electronic' ? ' is-on' : ''}`} aria-pressed={method === 'electronic'} onClick={() => setMethod('electronic')}>AUDENIQ에서 작성</button>
              <button type="button" className={`aq-chip${method === 'upload' ? ' is-on' : ''}`} aria-pressed={method === 'upload'} onClick={() => setMethod('upload')}>보유한 서류 첨부</button>
            </div>}
            {(method === 'upload' || !(kind in RIGHTS_DOCUMENTS)) && <div className="field">
              <label htmlFor="aqRequiredFile">증빙 원본 (필요 시)</label>
              <FilePicker
                id="aqRequiredFile" fileName={file?.name}
                accept=".pdf,.png,.jpg,.jpeg,application/pdf,image/png,image/jpeg"
                onChange={e => setFile(e.target.files?.[0] || null)}
              />
              <p className="help">원본 첨부 전에는 ‘서류 접수 대기’로 표시돼요.</p>
            </div>}
            <div className="aq-sticky-foot">
              <button type="submit" className="button studio-submit-wide" disabled={submitting}>
                {submitting ? '준비 중…' : method === 'electronic' && kind in RIGHTS_DOCUMENTS ? '전자 문서 작성하기' : '서류 접수하기'}
              </button>
            </div>
          </form>
        </Modal>
      )}

      {electronic && <RightsDocumentModal kind={electronic.kind} context={electronic.context} onClose={() => setElectronic(null)} onComplete={doc => { setElectronic(null); setOpenId(doc.id); toast('서명 문서가 완성됐어요. AUDENIQ 검토가 이어져요.', 'success'); }} />}

      {openDoc && !signing && (
        <DocumentModal
          doc={openDoc}
          onClose={() => setOpenId(null)}
          onOpenSignature={() => setSigning(true)}
        />
      )}

      {openDoc && signing && (
        <SignatureModal
          doc={openDoc}
          onBack={() => setSigning(false)}
          onSaved={() => {}}
        />
      )}
    </div>
  );
}
