// 발매 신청서 — 부가서비스·특별한 경우 선택, 배급 지역, 확인 배너 (배급 설정 단계부터 쓰여 따로 받는다)
import { Fragment, useRef, useState } from 'react';
import { FileCard } from '../../components/FileCard';
import { Modal } from '../../components/Modal';
import { SignaturePad, type SignaturePadHandle } from '../../components/SignaturePad';
import { MOCK } from '../../api/client';
import { useDocs } from '../../store/docs';
import { RIGHTS_DOCUMENTS, type RightsDocumentKind } from '../../lib/rightsDocument';
import { RERELEASE_KINDS, RERELEASE_AVAILABILITY, rereleaseSummary, type RereleaseData } from '../../lib/rerelease';
import { artistWarning, mmssToSeconds, titleWarning } from '../../lib/dsp';
import { CheckIcon } from '../../components/Check';
import { ChipPicker, OTHER } from '../../components/ChipPicker';
import { LyricDetail, MvDetail } from '../../components/AddonOptions';
import { Glyph } from '../../components/Glyph';
import { Segmented } from '../../components/Segmented';
import { TERRITORY_GROUPS, WORLD, parseTerritories, territoryName, territorySummary, worldExcept } from '../../lib/territories';
import { AI_TOOLS, AI_USES, type CoverTrackInfo, EXPRESS_REASONS, RIGHTS_OPTIONS, type ReleaseOptions, SERVICE_OPTIONS, type UploadState, type WizardForm, aiSummary, isContentId, selectedOptions } from './model';
import { KoreanDateField } from './fields';


export function DocAttach({ id, label, fileName, assetId, busy, onSelect, required, help, kind, releaseId, onElectronic, onOpenDoc }: {
  id: string; label: React.ReactNode; fileName: string;
  assetId?: string; busy?: boolean;
  onSelect: (file: File) => void; required?: boolean; help?: string;
  kind: RightsDocumentKind; releaseId?: string; onElectronic: (kind: RightsDocumentKind) => void; onOpenDoc: (id: string) => void;
}) {
  const [method, setMethod] = useState<'electronic' | 'upload'>(fileName ? 'upload' : 'electronic');
  const docs = useDocs().filter(d => d.releaseId === releaseId && d.electronic?.document_kind === kind);
  return (
    <div className="ui-field doc-attach aq-rights-evidence">
      <strong>{label}{required && <> <span className="required">*</span></>}</strong>
      <Segmented className="aq-method" label="서류 준비 방법" value={method} onChange={setMethod} options={[{ value: 'electronic', label: 'AUDENIQ에서 작성' }, { value: 'upload', label: '보유한 서류 첨부' }] as const} />
      {method === 'electronic' ? (
        <button type="button" className="aq-dropzone aq-filecard" onClick={() => onElectronic(kind)}>
          <span className="aq-dropzone-icon" aria-hidden="true"><Glyph name="sign" size={26} /></span>
          <span className="aq-dropzone-text">
            <strong>{docs.length ? '권리자 추가로 서명받기' : `${RIGHTS_DOCUMENTS[kind].title} 작성하기`}</strong>
            <small>발매 정보로 문서를 만들고 권리자가 직접 서명해요</small>
          </span>
        </button>
      ) : <><label className="sr-only" htmlFor={id}>{label}</label><FileCard
        id={id} fileName={fileName} busy={busy}
        accept=".pdf,.png,.jpg,.jpeg,application/pdf,image/png,image/jpeg"
        hint={help || 'PDF · JPG · PNG'}
        status={fileName ? (assetId ? '올렸어요 · 다시 누르면 바꿀 수 있어요' : '올린 기록이 없어요. 다시 첨부해 주세요') : undefined}
        onFile={f => onSelect(f)}
      /></>}
      {docs.map(d => <button key={d.id} type="button" className="aq-rights-receipt" onClick={() => onOpenDoc(d.id)}><CheckIcon size={16} /><span>{d.electronic?.rights_holder} · {d.reviewStatus === 'needs' ? '보완 요청' : '서명 완료'}<small>{d.reviewStatus === 'approved' ? 'AUDENIQ 검토 승인' : d.reviewStatus === 'needs' ? d.reviewNote : 'AUDENIQ 검토 중'} · 문서 보기</small></span></button>)}
    </div>
  );
}

export const certSvg = (d: string) => (
  <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>
);

export const CERT_METHODS: { key: string; label: string; sub: string; icon: React.ReactNode }[] = [
  { key: 'camera', label: '직접 촬영', sub: '카메라로 찍기', icon: certSvg('M4 8h3l2-3h6l2 3h3a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1Zm8 9a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z') },
  { key: 'pdf', label: 'PDF 선택', sub: '파일에서 고르기', icon: certSvg('M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Zm0 0v5h5M9 13h6M9 17h4') },
  { key: 'wallet', label: '전자문서지갑', sub: '연동 준비 중', icon: certSvg('M3 7a2 2 0 0 1 2-2h13v4M3 7v10a2 2 0 0 0 2 2h15V9H5a2 2 0 0 1-2-2Zm13 7h.01') },
];

export function GuardianConsentModal({ guardianName, onClose, onComplete }: {
  guardianName: string;
  onClose: () => void;
  onComplete: (certName: string, certMethod: string) => void;
}) {
  const padRef = useRef<SignaturePadHandle>(null);
  const [signed, setSigned] = useState(false);
  const [certName, setCertName] = useState('');
  const [certMethod, setCertMethod] = useState('');
  const [ack, setAck] = useState(false);
  const cameraRef = useRef<HTMLInputElement>(null);
  const pdfRef = useRef<HTMLInputElement>(null);

  const pick = (method: string) => {
    if (method === 'camera') cameraRef.current?.click();
    else if (method === 'pdf') pdfRef.current?.click();
    else { setCertMethod('wallet'); setCertName('전자문서지갑에서 가져옴'); }
  };
  const onFile = (method: string) => (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) { setCertMethod(method); setCertName(f.name); }
    e.target.value = '';
  };

  const missing = !ack ? '동의 항목을 체크해 주세요.' : !signed ? '법정대리인 서명을 입력해 주세요.' : !certName ? '가족관계증명서를 제출해 주세요.' : '';

  return (
    <Modal title="법정대리인 동의" onClose={onClose} dismissible={false} modalClass="aq-guardian-mode">
      <div className="guardian-modal-body">
        <p className="guardian-consent-text">
          미성년 아티스트의 음원 발매에 대해 법정대리인{guardianName ? <strong> {guardianName}</strong> : ''}님의 동의를 확인해요.
          아래 순서대로 진행해 주세요.
        </p>

        <ol className="aq-guardian-steps">
          <li className={ack ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{ack ? <CheckIcon size={13} /> : 1}</span>
            <label className="aq-guardian-agree">
              <input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} />
              <span>미성년자의 음원 발매와 배급에 법정대리인으로서 동의해요.<small>동의 내용은 발매 심사 때 확인돼요.</small></span>
            </label>
          </li>
          <li className={signed ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{signed ? <CheckIcon size={13} /> : 2}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">법정대리인 전자서명 <span className="required">*</span></strong>
              <SignaturePad ref={padRef} id="aqGuardianPad" label="법정대리인 서명 입력" height={170} onChange={setSigned} />
            </div>
          </li>
          <li className={certName ? 'is-done' : ''}>
            <span className="aq-guardian-no" aria-hidden="true">{certName ? <CheckIcon size={13} /> : 3}</span>
            <div className="min-0">
              <strong className="aq-guardian-label">가족관계증명서 <span className="required">*</span></strong>
              <div className="aq-cert-methods" role="radiogroup" aria-label="가족관계증명서 제출 방법">
                {CERT_METHODS.map(m => (
                  <button
                    key={m.key} type="button" role="radio" aria-checked={certMethod === m.key}
                    className={`aq-cert-method${certMethod === m.key ? ' is-active' : ''}`}
                    onClick={() => pick(m.key)}
                  >
                    <span className="aq-cert-icon">{m.icon}</span>
                    <strong>{m.label}</strong>
                    <small>{m.sub}</small>
                  </button>
                ))}
              </div>
              <input ref={cameraRef} type="file" accept="image/*" capture="environment" hidden onChange={onFile('camera')} />
              <input ref={pdfRef} type="file" accept=".pdf,application/pdf" hidden onChange={onFile('pdf')} />
              {certName && (
                <p className="aq-cert-picked">
                  <span className="aq-check-badge is-sm"><CheckIcon size={10} /></span> {certName}
                  {certMethod === 'wallet' && <small>전자문서지갑 연동 후 원본을 가져와요. 지금은 선택 상태로 저장돼요.</small>}
                </p>
              )}
            </div>
          </li>
        </ol>
      </div>
      <div className="aq-modal-foot aq-guardian-foot aq-sticky-foot">
        <p className="aq-guardian-missing" aria-live="polite">{missing}</p>
        <button type="button" className="ui-btn is-soft" onClick={onClose}>닫기</button>
        <button type="button" className="ui-btn is-primary" disabled={!!missing} onClick={() => onComplete(certName, certMethod)}>동의 완료</button>
      </div>
    </Modal>
  );
}

/** 배급 지역 — 기본은 ‘전 세계’ 스위치만. 끄면 배급할 지역 또는 뺄 지역을 고른다 */
export function TerritoryPicker({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const plan = parseTerritories(value);
  const plainWorld = plan.world && !plan.excluded.length;
  const [open, setOpen] = useState(!plainWorld);
  const [mode, setMode] = useState<'exclude' | 'include'>(plan.world ? 'exclude' : 'include');
  const [query, setQuery] = useState('');
  const picked = mode === 'exclude' ? plan.excluded : plan.included;
  const write = (codes: string[]) => onChange(mode === 'exclude' ? worldExcept(codes) : codes);
  const toggle = (code: string) => write(picked.includes(code) ? picked.filter(c => c !== code) : [...picked, code]);
  const q = query.trim().toLowerCase();
  const groups = TERRITORY_GROUPS
    .map(([region, codes]) => [region, codes.filter(c => !q || territoryName(c).toLowerCase().includes(q) || c.toLowerCase() === q)] as const)
    .filter(([, codes]) => codes.length > 0);
  const n = picked.length;
  const headline = !open || (mode === 'exclude' && !n) ? '전 세계에 배급해요.'
    : mode === 'exclude' ? `전 세계에서 ${n}곳을 빼고 배급해요.`
      : n ? `${n}개 국가·지역에만 배급해요.` : '배급할 지역을 골라 주세요.';
  const help = !open ? '특정 지역만 고르거나 빼려면 스위치를 꺼 주세요.'
    : mode === 'exclude' ? (n ? territorySummary(value) : '아래에서 배급하지 않을 지역을 골라 주세요.')
      : n ? picked.map(territoryName).join(', ') : '아래에서 배급할 지역을 하나 이상 골라 주세요.';

  return (
    <div id="aqTerritories" className="aq-territory">
      <div className="distribution-default">
        <div className="min-0">
          <strong>{headline}</strong>
          <p className="ui-field-help break">{help}</p>
        </div>
        <label className="aq-switch">
          <input
            type="checkbox" aria-label="전 세계 배급"
            checked={!open}
            onChange={e => {
              if (e.target.checked) { onChange([WORLD]); setOpen(false); setQuery(''); } else setOpen(true);
            }}
          />
          <span />
        </label>
      </div>
      {open && (
        <div className="aq-territory-panel aq-reveal">
          <Segmented
            className="aq-territory-mode" label="지역 선택 방식" value={mode}
            onChange={m => { setMode(m); onChange(m === 'exclude' ? [WORLD] : []); }}
            options={[{ value: 'exclude', label: '뺄 지역 고르기' }, { value: 'include', label: '배급할 지역 고르기' }] as const}
          />
          <div className="aq-territory-search">
            <label className="sr-only" htmlFor="aqTerritorySearch">국가·지역 검색</label>
            <input id="aqTerritorySearch" type="search" value={query} onChange={e => setQuery(e.target.value)} placeholder="국가·지역 이름으로 찾기" autoComplete="off" />
          </div>
          {n > 0 && (
            <div className="aq-territory-picked">
              <span>{mode === 'exclude' ? '빼는 지역' : '배급할 지역'} <b>{n}</b>곳</span>
              <button type="button" className="ui-btn is-ghost is-xs" onClick={() => write([])}>모두 해제</button>
            </div>
          )}
          {groups.length === 0 && <p className="aq-territory-empty">‘{query.trim()}’에 맞는 국가·지역이 없어요.</p>}
          {groups.map(([region, codes]) => {
            const all = codes.every(c => picked.includes(c));
            return (
              <div key={region} className="aq-territory-group">
                <div className="aq-territory-region">
                  <p>{region}</p>
                  {!q && (
                    <button type="button" className="ui-btn is-ghost is-xs" onClick={() => write(all ? picked.filter(c => !codes.includes(c)) : [...new Set([...picked, ...codes])])}>
                      {all ? '모두 해제' : '모두 선택'}
                    </button>
                  )}
                </div>
                <div className="aq-chips" role="group" aria-label={`${region} 국가·지역`}>
                  {codes.map(c => {
                    const on = picked.includes(c);
                    return (
                      <button key={c} type="button" className={`aq-chip${on ? ' is-on' : ''}${on && mode === 'exclude' ? ' is-excluded' : ''}`} aria-pressed={on} onClick={() => toggle(c)}>
                        {territoryName(c)}
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>
      )}
      <p className="aq-territory-note">
        <Glyph name="info" size={16} />
        <span>AUDENIQ 이용약관과 관련 법령에 따라 배급이 제한된 국가·지역은 전 세계 배급을 선택해도 자동으로 제외돼요. <a href="/terms" target="_blank" rel="noopener" className="aq-inline-link">약관 보기</a></span>
      </p>
    </div>
  );
}

export function OptionsSection({ form, set, group, onDocument, uploads, onElectronic, onOpenDoc, releaseId, onGuardian }: {
  form: WizardForm;
  group: 'service' | 'rights';
  set: <K extends keyof WizardForm>(key: K, value: WizardForm[K]) => void;
  onDocument: (name: keyof ReleaseOptions, asset: keyof ReleaseOptions, file: File) => void;
  uploads: Record<string, UploadState>;
  onElectronic: (kind: RightsDocumentKind) => void; onOpenDoc: (id: string) => void; releaseId?: string;
  /** 실서버: 법정대리인 동의 창 (임시 저장 후 서명 요청) */
  onGuardian: () => void;
}) {
  const o = form.options;
  const setOpt = <K extends keyof ReleaseOptions>(key: K, value: ReleaseOptions[K]) =>
    set('options', { ...o, [key]: value });
  const [showGuardianModal, setShowGuardianModal] = useState(false);
  const [expressOther, setExpressOther] = useState(false);
  // 예전 신청서(직접 입력한 aiTool만 있음)는 ‘기타’로 보여 준다
  const aiUses = o.aiUses?.length ? o.aiUses : o.aiTool ? [OTHER] : [];
  const aiUseOther = o.aiUses?.length ? o.aiUseOther ?? '' : o.aiTool;
  const setAi = (patch: Partial<ReleaseOptions>) => {
    const next = { aiUses, aiTools: o.aiTools ?? [], aiUseOther, aiToolOther: o.aiToolOther ?? '', ...patch };
    set('options', { ...o, ...next, aiTool: aiSummary(next) });
  };
  const expressPick = !o.expressReason ? (expressOther ? [OTHER] : []) : EXPRESS_REASONS.includes(o.expressReason) ? [o.expressReason] : [OTHER];

  // 선택한 카드 바로 밑에 여는 입력 (목록 맨 아래가 아니라)
  const detail: Partial<Record<keyof ReleaseOptions, React.ReactNode>> = {
    mv: <MvDetail o={o} set={patch => set('options', { ...o, ...patch })} tracks={form.tracks} />,
    lyricSync: <LyricDetail o={o} set={patch => set('options', { ...o, ...patch })} tracks={form.tracks} />,
    express: (
      <div className="aq-option-detail">
        <h3>신속 발매 · 10,000원</h3>
        <p className="aq-option-intro">신청 후 3영업일 안에 발매되도록 심사와 플랫폼 전송을 먼저 처리해요. 신청을 마치면 부가서비스에서 정산금이나 카드로 결제해요. 보완이 필요하거나 일정을 맞출 수 없으면 담당자가 연락하고, 진행하지 못하면 전액 환불해요.</p>
        <div className="ui-field">
          <label htmlFor="aqExpressReason">신속 발매가 필요한 이유<span className="aq-optional">선택</span></label>
          <ChipPicker
            id="aqExpressReason" single list options={EXPRESS_REASONS} value={expressPick}
            onChange={v => { setExpressOther(v[0] === OTHER); setOpt('expressReason', v[0] && v[0] !== OTHER ? v[0] : ''); }}
            other={EXPRESS_REASONS.includes(o.expressReason) ? '' : o.expressReason} onOther={v => setOpt('expressReason', v)}
            otherPlaceholder="신속 발매가 필요한 이유"
          />
        </div>
        {/* 위 선택지와 같은 모양의 한 줄 — 확인 표시도 오른쪽 */}
        <label className={`aq-pick aq-ack${o.expressAck ? ' is-on' : ''}`}>
          <input
            type="checkbox" id="aqExpressAck" className="aq-ack-input"
            checked={o.expressAck}
            onChange={e => setOpt('expressAck', e.target.checked)}
          />
          <span>신속 발매 비용 10,000원을 결제할게요.<small>신청을 마친 뒤 부가서비스에서 결제해요. 자동으로 결제되지는 않아요.</small></span>
          <span className="aq-pick-mark" aria-hidden="true">{o.expressAck && <CheckIcon size={12} />}</span>
        </label>
      </div>
    ),
    minor: (
      <div className="aq-option-detail">
        <h3>법정대리인 확인</h3>
        <p className="aq-option-intro">본인 및 권리자의 동의 범위를 확인할 수 있도록 보호자 정보를 입력해 주세요. 법정대리인은 2명을 입력하는 것이 원칙이지만, 1명인 경우도 접수할 수 있어요.</p>
        <h4 className="aq-guardian-head">법정대리인 1</h4>
        <div className="ui-field">
          <label htmlFor="aqGuardian">법정대리인 성명 <span className="required">*</span></label>
          <input
            id="aqGuardian" autoComplete="name" maxLength={90} value={o.guardian}
            onChange={e => setOpt('guardian', e.target.value)}
            placeholder="법정대리인 성명"
          />
        </div>
        <div className="ui-field">
          <label htmlFor="aqGuardianRelation">아티스트와의 관계 <span className="required">*</span></label>
          <select
            id="aqGuardianRelation" value={o.guardianRelation}
            onChange={e => setOpt('guardianRelation', e.target.value)}
          >
            <option value="">관계를 선택해 주세요.</option>
            {['부', '모', '기타 법정대리인'].map(x => (
              <option key={x} value={x}>{x}</option>
            ))}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="aqGuardianContact">법정대리인 연락처 또는 이메일 <span className="required">*</span></label>
          <input
            id="aqGuardianContact" maxLength={160} value={o.guardianContact}
            onChange={e => setOpt('guardianContact', e.target.value)}
            placeholder="확인이 가능한 연락처"
          />
        </div>
        <h4 className="aq-guardian-head">법정대리인 2 <small>(해당하는 경우)</small></h4>
        <div className="ui-field">
          <label htmlFor="aqGuardian2">법정대리인 성명</label>
          <input
            id="aqGuardian2" autoComplete="name" maxLength={90} value={o.guardian2}
            onChange={e => setOpt('guardian2', e.target.value)}
            placeholder="법정대리인 성명"
          />
        </div>
        <div className="ui-field">
          <label htmlFor="aqGuardian2Relation">아티스트와의 관계</label>
          <select
            id="aqGuardian2Relation" value={o.guardian2Relation}
            onChange={e => setOpt('guardian2Relation', e.target.value)}
          >
            <option value="">관계를 선택해 주세요.</option>
            {['부', '모', '기타 법정대리인'].map(x => (
              <option key={x} value={x}>{x}</option>
            ))}
          </select>
        </div>
        <div className="ui-field">
          <label htmlFor="aqGuardian2Contact">법정대리인 연락처 또는 이메일</label>
          <input
            id="aqGuardian2Contact" maxLength={160} value={o.guardian2Contact}
            onChange={e => setOpt('guardian2Contact', e.target.value)}
            placeholder="확인이 가능한 연락처"
          />
        </div>
        <div className="ui-field">
          <label>법정대리인 동의</label>
          <button
            type="button" className="ui-btn is-soft" style={{ width: '100%' }}
            onClick={() => (MOCK ? setShowGuardianModal(true) : onGuardian())}
          >
            {o.guardianConsentDone ? '법정대리인 동의를 마쳤어요 · 다시 확인하기' : '법정대리인 동의 받기'}
          </button>
          <p className="ui-field-help">
            {o.guardianConsentDone
              ? `동의 완료${o.familyCertName ? ` · 가족관계증명서: ${o.familyCertName}` : ''}`
              : MOCK ? '동의창에서 법정대리인 서명과 가족관계증명서를 제출해요.'
                : '법정대리인이 직접 본인확인 후 서명하고, 가족관계증명서를 올려요.'}
          </p>
        </div>
        <p className="aq-option-note">{MOCK
          ? '법정대리인 정보 입력만으로 본인 확인이나 동의 검증이 완료되지는 않아요. 담당자 확인 후 서명 단계를 안내해요.'
          : '서명은 법정대리인 본인만 할 수 있어요. AUDENIQ이 가족관계증명서로 관계를 확인한 뒤 심사를 진행해요.'}</p>
      </div>
    ),
    cover: (
      <div className="aq-option-detail">
        <h3>커버곡 정보</h3>
        <p className="aq-option-intro">커버한 트랙을 선택하고, 원곡 정보를 입력해 주세요.</p>
        {form.tracks.map((t, i) => {
          const info = o.coverTracks.find(c => c.trackId === t.id);
          const toggleCover = (checked: boolean) => {
            setOpt('coverTracks', checked
              ? [...o.coverTracks, { trackId: t.id, originalTitle: '', originalArtist: '', originalWriters: '' }]
              : o.coverTracks.filter(c => c.trackId !== t.id));
          };
          const setCoverInfo = (key: keyof Omit<CoverTrackInfo, 'trackId'>, value: string) => {
            setOpt('coverTracks', o.coverTracks.map(c =>
              c.trackId === t.id ? { ...c, [key]: value } : c));
          };
          return (
            <div key={t.id} className="cover-track-item">
              <label className="ui-check is-plain">
                <input
                  type="checkbox" checked={!!info}
                  onChange={e => toggleCover(e.target.checked)}
                />
                <span><strong>트랙 {i + 1} · {t.title.trim() || '(제목 없음)'}</strong></span>
              </label>
              {info && (
                <div className="cover-track-fields">
                  <div className="ui-field">
                    <label htmlFor={`cover-orig-title-${i}`}>원곡 제목 <span className="required">*</span></label>
                    <input
                      id={`cover-orig-title-${i}`} maxLength={200} value={info.originalTitle}
                      onChange={e => setCoverInfo('originalTitle', e.target.value)}
                      placeholder="원곡의 제목"
                    />
                  </div>
                  <div className="ui-field">
                    <label htmlFor={`cover-orig-artist-${i}`}>원곡 아티스트 <span className="required">*</span></label>
                    <input
                      id={`cover-orig-artist-${i}`} maxLength={200} value={info.originalArtist}
                      onChange={e => setCoverInfo('originalArtist', e.target.value)}
                      placeholder="원곡자 또는 원 아티스트"
                    />
                  </div>
                  <div className="ui-field">
                    <label htmlFor={`cover-orig-writers-${i}`}>원곡 작사 / 작곡</label>
                    <input
                      id={`cover-orig-writers-${i}`} maxLength={200} value={info.originalWriters}
                      onChange={e => setCoverInfo('originalWriters', e.target.value)}
                      placeholder="알고 있다면 입력"
                    />
                  </div>
                </div>
              )}
            </div>
          );
        })}
        <label className="ui-check is-plain" style={{ marginTop: 12 }}>
          <input
            type="checkbox" id="aqCoverRightsAck"
            checked={o.coverRightsAck}
            onChange={e => setOpt('coverRightsAck', e.target.checked)}
          />
          <span>원곡 저작권자의 이용 허락을 받았거나, 정당한 라이선스 절차를 진행할 것을 확인해요.<small>허락 없는 커버곡 배급은 저작권 침해가 될 수 있어요.</small></span>
        </label>
        <DocAttach
          kind="composition" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqCoverLicense" label="원곡 이용 허락서"
          fileName={o.coverLicenseFile} assetId={o.coverLicenseAssetId} busy={uploads.coverLicenseFile?.state === 'uploading'}
          onSelect={file => onDocument('coverLicenseFile', 'coverLicenseAssetId', file)}
          help="이용 허락서나 라이선스 계약서를 첨부해 주세요."
        />
      </div>
    ),
    sample: (
      <div className="aq-option-detail">
        <h3>샘플링·타인 음원 사용</h3>
        <p className="aq-option-intro">사용한 원본 음원과 저작물의 출처를 확인해요.</p>
        <DocAttach
          kind="sample" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqSampleLicense" label="원본 이용 허락서"
          fileName={o.sampleLicenseFile} assetId={o.sampleLicenseAssetId} busy={uploads.sampleLicenseFile?.state === 'uploading'}
          onSelect={file => onDocument('sampleLicenseFile', 'sampleLicenseAssetId', file)}
          help="샘플링 원본의 이용 허락서나 라이선스 계약서를 첨부해 주세요."
        />
        <p className="aq-option-note">허락 없는 샘플링은 저작권 침해가 될 수 있어요.</p>
      </div>
    ),
    featured: (
      <div className="aq-option-detail">
        <h3>피처링·공동 실연</h3>
        <p className="aq-option-intro">참여자의 크레딧과 이용 허락을 확인해요.</p>
        <DocAttach
          kind="performer" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqFeaturedConsent" label="참여자 동의서 (있으면)"
          fileName={o.featuredConsentFile} assetId={o.featuredConsentAssetId} busy={uploads.featuredConsentFile?.state === 'uploading'}
          onSelect={file => onDocument('featuredConsentFile', 'featuredConsentAssetId', file)}
          help="피처링 참여자의 동의서나 계약서를 첨부해 주세요."
        />
      </div>
    ),
    shared: (
      <div className="aq-option-detail">
        <h3>공동 권리자·레이블 계약</h3>
        <p className="aq-option-intro">각 권리자와의 배급 위임 범위를 확인해요.</p>
        <DocAttach
          kind="shared" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqSharedContract" label="공동 권리 계약서"
          fileName={o.sharedContractFile} assetId={o.sharedContractAssetId} busy={uploads.sharedContractFile?.state === 'uploading'}
          onSelect={file => onDocument('sharedContractFile', 'sharedContractAssetId', file)}
          help="배급 위임 범위가 명시된 계약서를 첨부해 주세요."
        />
      </div>
    ),
    rerelease: (
      <div className="aq-option-detail">
        <h3>기존 발매 이전·재발매</h3>
        <p className="aq-option-intro">현재 상황을 고르면 기존 코드와 서비스 이전에 필요한 내용을 안내해 드려요.</p>
        <div className="ui-field">
          <label>어떤 상황인가요? <span className="required">*</span></label>
          <ChipPicker id="aqRereleaseKind" single options={Object.values(RERELEASE_KINDS)} value={o.rereleaseKind ? [RERELEASE_KINDS[o.rereleaseKind]] : []}
            onChange={v => set('options', { ...o, rereleaseKind: (Object.entries(RERELEASE_KINDS).find(([, label]) => label === v[0])?.[0] ?? '') as RereleaseData['rereleaseKind'], rereleaseAck: false })} />
        </div>
        <div className="ui-field">
          <label htmlFor="aqPreviousTitle">기존 발매명 <span className="required">*</span></label>
          <input
            id="aqPreviousTitle" maxLength={180} value={o.previousTitle}
            onChange={e => setOpt('previousTitle', e.target.value)}
            placeholder="기존 앨범·싱글 제목"
          />
        </div>
        <div className="ui-field">
          <label htmlFor="aqPreviousDistributor">이전 유통사·레이블 (알고 있다면)</label>
          <input id="aqPreviousDistributor" maxLength={160} value={o.previousDistributor ?? ''} onChange={e => setOpt('previousDistributor', e.target.value)} placeholder="기존 배급을 맡은 곳" />
        </div>
        <div className="ui-field">
          <label htmlFor="aqPreviousUrl">기존 발매 링크 <span className="aq-optional">선택</span></label>
          <input id="aqPreviousUrl" type="url" maxLength={500} value={o.previousUrl ?? ''} onChange={e => setOpt('previousUrl', e.target.value)} placeholder="https://…" />
        </div>
        <KoreanDateField
          id="aqOriginalDate" label={o.rereleaseKind === 'new_version' ? '이전 버전의 최초 발매일' : '최초 발매일'} required
          value={form.originalDate}
          onChange={v => set('originalDate', v)}
        />
        <div className="ui-field"><label>기존 음원은 지금 서비스 중인가요? <span className="required">*</span></label>
          <ChipPicker id="aqPreviousAvailability" single options={Object.values(RERELEASE_AVAILABILITY)} value={o.previousAvailability ? [RERELEASE_AVAILABILITY[o.previousAvailability]] : []}
            onChange={v => set('options', { ...o, previousAvailability: (Object.entries(RERELEASE_AVAILABILITY).find(([, label]) => label === v[0])?.[0] ?? '') as RereleaseData['previousAvailability'], rereleaseAck: false })} />
        </div>
        <div className="ui-field"><label htmlFor="aqRereleaseAudio">이번 음원은 기존 녹음과 같은가요? <span className="required">*</span></label>
          <select id="aqRereleaseAudio" value={o.rereleaseAudio ?? ''} onChange={e => set('options', { ...o, rereleaseAudio: e.target.value as RereleaseData['rereleaseAudio'], rereleaseAck: false })}>
            <option value="">선택해 주세요</option><option value="same">같은 녹음 · 음원 내용 변경 없음</option><option value="changed">새 녹음·리믹스 등 음악 내용 변경</option><option value="unknown">리마스터처럼 바뀐 점이 있는지 확인이 필요해요</option>
          </select>
        </div>
        {o.rereleaseAudio && <div className="aq-option-note" role="status">
          {o.rereleaseAudio === 'same' ? '같은 녹음은 기존 ISRC를 유지해요. 제목·아티스트·길이 등 기존 정보를 맞춰 주세요. 스트리밍 수와 플레이리스트 연결 결과는 플랫폼에서 결정해요.'
            : o.rereleaseAudio === 'changed' ? '새 녹음·리믹스처럼 음악 내용이 달라지면 새로운 ISRC가 필요해요. 기존 코드는 비교용으로 보관하고 새 음원에 적용하지 않아요.'
              : '단순 음량 조정과 음악 내용 변경은 코드 처리 방식이 달라요. 변경 내용을 아래에 적고 문의 메뉴에서 확인해 주세요. 확인 전에는 임시 저장해 둘 수 있어요.'}
        </div>}
        <div className="ui-field"><label htmlFor="aqPreviousUpc">기존 음반 코드 UPC <span className="aq-optional">선택</span></label>
          <input id="aqPreviousUpc" inputMode="numeric" maxLength={14} value={o.previousUpc ?? ''} onChange={e => setOpt('previousUpc', e.target.value)} placeholder="기존 앨범·싱글의 UPC" />
          {o.rereleaseKind === 'transfer' && o.rereleaseAudio === 'same' && o.previousUpc && <button type="button" className="ui-btn is-white is-sm" onClick={() => set('upc', o.previousUpc ?? '')}>같은 음반의 UPC를 배급 설정에 넣기</button>}
          <p className="ui-field-help">트랙 구성이나 버전이 다른 음반은 새 UPC를 사용해요. 기존 UPC는 이전 유통사에서 확인할 수 있어요.</p>
        </div>
        <h4>트랙별 기존 ISRC</h4>
        {form.tracks.map((t, i) => <div key={t.id} className="ui-field">
          <label htmlFor={`aqPreviousIsrc-${i}`}>{i + 1}. {t.title || '제목 없는 트랙'} · 기존 ISRC{o.rereleaseAudio === 'same' && <> <span className="required">*</span></>}</label>
          <input id={`aqPreviousIsrc-${i}`} maxLength={15} value={o.rereleaseTracks?.find(x => x.trackId === t.id)?.previousIsrc ?? (i === 0 ? o.previousId : '')}
            onChange={e => setOpt('rereleaseTracks', form.tracks.map((track, k) => ({ trackId: track.id, previousIsrc: track.id === t.id ? e.target.value : o.rereleaseTracks?.find(x => x.trackId === track.id)?.previousIsrc ?? (k === 0 ? o.previousId : '') })))} placeholder="예: KR-ABC-26-00001" />
        </div>)}
        {o.rereleaseAudio === 'same' && <button type="button" className="ui-btn is-soft" onClick={() => {
          const previous = form.tracks.map((t, i) => ({ trackId: t.id, previousIsrc: o.rereleaseTracks?.find(x => x.trackId === t.id)?.previousIsrc ?? (i === 0 ? o.previousId : '') }));
          set('tracks', form.tracks.map(t => ({ ...t, isrc: previous.find(x => x.trackId === t.id)?.previousIsrc || t.isrc })));
          setOpt('rereleaseTracks', previous);
        }}>기존 ISRC를 트랙 등록에 적용</button>}
        <p className="ui-field-help">ISRC를 모르면 이전 유통사의 발매 내역에서 확인해 주세요. 같은 녹음에 새 코드를 발급하지 않도록 확인 후 접수해요.</p>
        <div className="ui-field"><label htmlFor="aqRereleaseRights">기존 계약과 이번 배급 권한 <span className="required">*</span></label>
          <select id="aqRereleaseRights" value={o.rereleaseRights ?? ''} onChange={e => setOpt('rereleaseRights', e.target.value as RereleaseData['rereleaseRights'])}>
            <option value="">선택해 주세요</option><option value="owned">내가 권리자이며 이전 계약의 제한을 확인했어요</option><option value="permission">권리자·레이블로부터 이번 배급 허락을 받았어요</option><option value="pending">계약 또는 이전 허락을 확인 중이에요</option>
          </select>
        </div>
        {o.rereleaseRights === 'permission' && <DocAttach kind="master" releaseId={releaseId} onElectronic={onElectronic} onOpenDoc={onOpenDoc}
          id="aqRereleasePermission" label="이번 배급 이용 허락서" fileName={o.rereleasePermissionFile ?? ''} assetId={o.rereleasePermissionAssetId} busy={uploads.rereleasePermissionFile?.state === 'uploading'} onSelect={file => onDocument('rereleasePermissionFile', 'rereleasePermissionAssetId', file)} />}
        <div className="ui-field"><label htmlFor="aqRereleaseNotes">변경 사항·이전 일정·확인이 필요한 내용</label><textarea id="aqRereleaseNotes" maxLength={1200} rows={3} value={o.rereleaseNotes ?? ''} onChange={e => setOpt('rereleaseNotes', e.target.value)} placeholder="예: 기존 서비스 종료 요청일, 리마스터 변경 내용, 이전 계약 확인 상황" /></div>
        {(o.previousAvailability === 'live' || o.previousAvailability === 'takedown_requested') && <p className="aq-option-note">기존 서비스가 남아 있어요. 이전 유통사와 종료·이전 일정을 조율하고, 새 서비스 연결을 확인한 뒤 중복 송출을 정리해 주세요. AUDENIQ 신청만으로 기존 서비스가 내려가지는 않아요.</p>}
        <label className="ui-check is-plain"><input id="aqRereleaseAck" type="checkbox" checked={!!o.rereleaseAck} onChange={e => setOpt('rereleaseAck', e.target.checked)} /><span>기존 계약·음원 코드·서비스 상태를 확인했고, 서비스 이전 일정과 플랫폼 연결 결과는 별도로 확인할게요.</span></label>
        <p className="ui-field-help">코드 안내 기준: <a href="https://isrc.ifpi.org/why-use-isrc/when-to-assign" target="_blank" rel="noreferrer">IFPI ISRC</a> · <a href="https://support.spotify.com/sc-en/artists/article/re-uploading-music/" target="_blank" rel="noreferrer">Spotify 재업로드 안내</a></p>
      </div>
    ),
    ai: (
      <div className="aq-option-detail">
        <h3>AI 활용 내역</h3>
        <div className="ui-field">
          <label htmlFor="aqAiTool">어떻게 활용했나요? <span className="required">*</span> <small className="muted">여러 개 고를 수 있어요</small></label>
          <ChipPicker
            id="aqAiTool" list options={AI_USES} value={aiUses} onChange={v => setAi({ aiUses: v })}
            other={aiUseOther} onOther={v => setAi({ aiUseOther: v })} otherPlaceholder="AI를 활용한 부분을 적어 주세요"
          />
        </div>
        <div className="ui-field">
          <label htmlFor="aqAiTools">사용한 도구<span className="aq-optional">선택</span></label>
          <ChipPicker
            id="aqAiTools" options={AI_TOOLS} value={o.aiTools ?? []} onChange={v => setAi({ aiTools: v })}
            other={o.aiToolOther ?? ''} onOther={v => setAi({ aiToolOther: v })} otherPlaceholder="도구 이름"
          />
        </div>
        <p className="ui-field-help">이용 약관과 상업적 이용 허가 자료를 요청할 수 있어요. 모든 배급 플랫폼이 AI 음악을 받는 것은 아니에요.</p>
      </div>
    ),
  };
  const list = group === 'service' ? SERVICE_OPTIONS : RIGHTS_OPTIONS;
  // 권리 단계의 '해당하는 항목' 7개는 대부분 해당이 없다 — 먼저 '특별한 경우가 있나요?'를 묻고 '있어요'일 때만 펼친다.
  // 이미 고른 항목이 있으면 처음부터 펼쳐 둔다
  const anyRights = RIGHTS_OPTIONS.some(([id]) => !!o[id]);
  // 곡에 적은 피처링 아티스트 — 권리 확인의 '피처링·공동 실연'과 이어 준다
  const featNames = group === 'rights' ? [...new Set(form.tracks.flatMap(t => t.featuring.split(',').map(x => x.trim()).filter(Boolean)))].slice(0, 3) : [];
  const [special, setSpecial] = useState(anyRights);
  const showList = group === 'service' || special || anyRights;

  return (
    <>
      {group === 'service' ? (
        <>
          <h2 className="subhead" style={{ marginTop: 25 }}>부가서비스</h2>
          <p className="aq-option-intro">필요한 서비스를 선택해 주세요.</p>
        </>
      ) : (
        <>
          <h2 className="subhead" style={{ marginTop: 25 }}>특별한 경우가 있나요?</h2>
          <p className="aq-option-intro">커버곡·샘플링·피처링·AI 제작·공동 권리·미성년·재발매 중 해당하는 게 있으면 알려 주세요.</p>
          <div className="aq-yesno" role="radiogroup" aria-label="특별한 경우가 있나요?">
            <button type="button" role="radio" aria-checked={!showList} className={`ui-chip${!showList ? ' is-on' : ''}`} disabled={anyRights} onClick={() => setSpecial(false)}>없어요</button>
            <button type="button" role="radio" aria-checked={showList} className={`ui-chip${showList ? ' is-on' : ''}`} onClick={() => setSpecial(true)}>있어요</button>
          </div>
          {featNames.length > 0 && !o.featured && (
            <div className="ui-callout is-row aq-reveal" role="status">
              <span>피처링 아티스트(<b>{featNames.join(', ')}</b>)를 적은 곡이 있어요. ‘피처링·공동 실연’으로 참여자 동의를 함께 확인해 주세요.</span>
              <button type="button" className="ui-btn is-primary is-sm" onClick={() => { setSpecial(true); setOpt('featured', true); }}>피처링 선택</button>
            </div>
          )}
          {showList && <p className="aq-option-intro">해당하는 항목을 모두 선택해 주세요. 누르면 바로 아래에 필요한 확인 사항이 열려요.</p>}
        </>
      )}
      {showList && <div className="aq-options aq-reveal">
        {list.map(([id, title, sub]) => (
          <Fragment key={id}>
            <label className={`aq-option${o[id] ? ' is-on' : ''}`}>
              <span className="aq-option-text"><strong>{title}</strong><small>{sub}</small></span>
              <input
                type="checkbox" aria-label={title}
                checked={!!o[id]}
                onChange={e => setOpt(id, e.target.checked as ReleaseOptions[typeof id])}
              />
            </label>
            {!!o[id] && detail[id]}
          </Fragment>
        ))}
      </div>}
      {group === 'rights' && form.platforms.some(p => p === 'youtube-cid' || p === 'D-27') && (
        <div className="aq-option-detail" id="aqContentIdRights">
          <h3>YouTube Content ID 권리 확인</h3>
          <p>다른 이용자의 영상에 권리 주장을 하는 서비스예요. 일반 YouTube Music 배급과 별도로, 참조 음원의 독점 권리와 고유성을 확인해 주세요.</p>
          <label className="ui-check is-plain">
            <input type="checkbox" checked={!!o.contentIdExclusiveRightsAck} onChange={e => setOpt('contentIdExclusiveRightsAck', e.target.checked)} />
            <span>신청한 모든 지역에서 참조 음원 전체에 대한 독점 권리가 있어요.</span>
          </label>
          <label className="ui-check is-plain">
            <input type="checkbox" checked={!!o.contentIdOriginalRecordingAck} onChange={e => setOpt('contentIdOriginalRecordingAck', e.target.checked)} />
            <span>직접 제작한 고유한 녹음이며, 비독점 루프·무료 이용 음원·공공저작물 등을 포함하지 않아요.</span>
          </label>
          <p className="hint">직접 만든 녹음이 아니면 Content ID를 꺼 주세요. 커버·샘플·리믹스는 권리와 참조 제외 구간을 한 번 더 확인해요.</p>
          <button type="button" className="ui-btn is-soft is-sm" onClick={() => set('platforms', form.platforms.filter(p => !isContentId(p)))}>Content ID 사용 안 함</button>
        </div>
      )}
      {showGuardianModal && (
        <GuardianConsentModal
          guardianName={o.guardian}
          onClose={() => setShowGuardianModal(false)}
          onComplete={(certName, certMethod) => {
            // 단일 업데이트로 통합 — 연속 setOpt는 stale closure로 마지막만 남음
            set('options', { ...o, guardianConsentDone: true, familyCertName: certName, familyCertMethod: certMethod });
            setShowGuardianModal(false);
          }}
        />
      )}
      {group === 'rights' && <p className="aq-option-note">선택한 내용에 따라 권리·계약 서류가 별도로 준비돼요.</p>}
    </>
  );
}

export function OptionDocsBanner({ options }: { options: ReleaseOptions }) {
  const chosen = selectedOptions(options);
  if (!chosen.length) return null;
  return (
    <div className="aq-req-banner">
      <h3>추가 확인이 필요한 항목</h3>
      <p>허락서·동의서는 신청 화면에서 자동 작성하고 권리자가 직접 서명할 수 있어요. 완성된 문서는 권리·보완 서류에 보관돼요.</p>
      <div>{chosen.map(([id, title]) => <span key={id} className="aq-req-tag">{title}</span>)}</div>
      {options.minor && (
        <p style={{ marginTop: 13 }}>법정대리인 동의서는 아티스트 본인의 확인과 별도로 관리돼요.</p>
      )}
    </div>
  );
}

export function FinalReviewBanner({ form }: { form: WizardForm }) {
  const o = form.options;
  const chosen = selectedOptions(o);
  const warnings: string[] = [];
  if (o.express) warnings.push('신속 발매: 원하는 날짜와 이용 조건을 확인한 뒤 진행');
  if (o.minor) warnings.push('미성년: 법정대리인 동의서와 자격 확인 필요');
  if (o.ai) warnings.push('AI 음원: 이용 권한과 플랫폼별 허용 기준 확인 필요');
  if (chosen.length) warnings.push('필요한 허락서·동의서는 AUDENIQ 전자 문서 작성 또는 보유 서류 첨부로 준비');
  if (o.rerelease) warnings.push(rereleaseSummary(o));
  // 배급은 되지만 플랫폼 검수에서 걸릴 수 있는 표기
  const named = form.tracks.filter(t => t.title.trim());
  for (const w of [titleWarning(form.title), artistWarning(form.artist)]) if (w) warnings.push(w);
  named.forEach((t, k) => {
    const w = titleWarning(t.title);
    if (w) warnings.push(`트랙 ${k + 1}: ${w}`);
    if (t.audioSpec.includes('모노')) warnings.push(`트랙 ${k + 1}: 모노 음원이에요. 스테레오 마스터가 있다면 교체해 주세요.`);
    const secs = mmssToSeconds(t.duration);
    if (secs && secs < 30) warnings.push(`트랙 ${k + 1}: 30초 미만 곡은 일부 플랫폼에서 재생 수익이 집계되지 않아요.`);
  });
  const noLyrics = named.filter(t => !t.instrumental && !t.lyrics.trim()).length;
  if (noLyrics) warnings.push(`가사가 없는 보컬 곡이 ${noLyrics}곡 있어요. 국내 플랫폼 가사 노출을 원하면 곡 상세 정보에 가사를 입력해 주세요.`);
  // 단계 목록의 '입력 필요'와 같은 기준 (작사는 연주곡이면 없어도 된다)
  const incomplete = form.tracks.filter(t => !t.audioName || !t.title.trim() || !t.composers.trim() || (!t.instrumental && !t.lyricists.trim())).length;
  const withAudio = form.tracks.filter(t => t.audioName).length;
  return (
    <div className="aq-req-banner">
      <h3>접수 전 확인해 주세요.</h3>
      <div className="aq-review-list">
        <div className="aq-review-item">
          <span>발매 옵션</span>
          <strong>{chosen.length ? chosen.map(([, title]) => title).join(' · ') : '일반 발매'}</strong>
        </div>
        <div className="aq-review-item">
          <span>트랙 필수 정보</span>
          <strong>{incomplete ? `${incomplete}곡 보완 필요` : '입력 완료'}</strong>
        </div>
        <div className="aq-review-item">
          <span>원본 음원</span>
          <strong>{withAudio} / {form.tracks.length}곡 선택</strong>
        </div>
        <div className="aq-review-item">
          <span>커버아트</span>
          <strong>{form.coverName || '등록 전'}</strong>
        </div>
      </div>
      {warnings.map(w => <p key={w} className="aq-option-note">{w}</p>)}
    </div>
  );
}
