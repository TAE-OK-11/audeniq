// 서류 검토 — 담당자가 요청한 권리 증빙을 승인하거나 보완 요청한다.
// 발매 신청서(배급 계약서)는 발매 심사에서 발매와 함께 결정한다.
import { DocFileLink } from '../DocFileLink';
import { useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type GrantInput, type StaffDocument } from '../api';
import { DOC_KIND, DOC_STATUS, ago, pick, when } from '../labels';
import { EmptyState, ErrorBox, ChipGroup, Cover, NoDuty, PageHeader, SkeletonList, StatusBadge, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';

const STATUSES = ['REVIEW', 'AWAITING_DOCUMENTS', 'NEEDS', 'APPROVED'];
const decidable = (d: StaffDocument) => d.kind === 'RIGHTS_PROOF' && d.status === 'REVIEW';

const RIGHT_TYPES: [GrantInput['right_type'], string][] = [
  ['MASTER', '음원(마스터)'], ['COMPOSITION', '작곡'], ['LYRICS', '작사'], ['PERFORMANCE', '실연'], ['ARTWORK', '커버아트'], ['SAMPLE', '샘플'], ['OTHER', '기타'],
];
interface GrantRow { cls: GrantInput['grant_class']; type: GrantInput['right_type']; holder: string; share: string; territories: string; uses: string; start: string; end: string }
const newGrant = (holder = ''): GrantRow => ({ cls: 'DISTRIBUTION', type: 'MASTER', holder, share: '', territories: '', uses: '', start: '', end: '' });
const list = (s: string) => s.split(/[\s,]+/).filter(Boolean);
const toGrant = (g: GrantRow): GrantInput => ({
  grant_class: g.cls, right_type: g.type, rights_holder: g.holder.trim(),
  share_bps: g.cls === 'ROYALTY' ? Math.round(Number(g.share) * 100) : undefined,
  territories: list(g.territories.toUpperCase()), uses: list(g.uses), start_date: g.start || null, end_date: g.end || null,
});
const grantOk = (g: GrantRow) => !!g.holder.trim() && (g.cls !== 'ROYALTY' || (Number(g.share) > 0 && Number(g.share) <= 100));

/** 승인할 때 이 서류가 증명하는 권한을 기록한다 — 배급 권한(지역·사용·기간)과 정산 수령권(지분)은 따로 */
function GrantRows({ rows, onChange }: { rows: GrantRow[]; onChange: (r: GrantRow[]) => void }) {
  const set = (i: number, p: Partial<GrantRow>) => onChange(rows.map((r, n) => (n === i ? { ...r, ...p } : r)));
  return (
    <div className="adm-grants">
      <b>이 서류가 증명하는 권한</b>
      <p className="small muted">배급 권한은 실제 전송 지역·사용·기간에, 정산 수령권은 지분대로 정산에 쓰여요. 발매 전체에 적용돼요.</p>
      {rows.map((g, i) => (
        <fieldset key={i} className="adm-grant">
          <div className="ui-chips is-wrap" role="radiogroup" aria-label="권한 구분">
            {(['DISTRIBUTION', 'ROYALTY'] as const).map(c => (
              <button key={c} type="button" role="radio" aria-checked={g.cls === c} className={`ui-chip${g.cls === c ? ' is-on' : ''}`} onClick={() => set(i, { cls: c })}>
                {c === 'DISTRIBUTION' ? '배급 권한' : '정산 수령권'}
              </button>
            ))}
          </div>
          <div className="ui-field"><label>권리 종류</label>
            <select className="ui-input" value={g.type} onChange={e => set(i, { type: e.target.value as GrantRow['type'] })}>
              {RIGHT_TYPES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select></div>
          <div className="ui-field"><label>권리자 <span className="required">*</span></label>
            <input className="ui-input" maxLength={120} value={g.holder} onChange={e => set(i, { holder: e.target.value })} placeholder="서류에 적힌 권리자 이름" /></div>
          {g.cls === 'ROYALTY' && <div className="ui-field"><label>지분 (%) <span className="required">*</span></label>
            <input className="ui-input" inputMode="decimal" value={g.share} onChange={e => set(i, { share: e.target.value.replace(/[^\d.]/g, '') })} placeholder="예: 25" /></div>}
          {g.cls === 'DISTRIBUTION' && <>
            <div className="ui-field"><label>허락 국가 (비우면 전 세계)</label>
              <input className="ui-input" value={g.territories} onChange={e => set(i, { territories: e.target.value })} placeholder="예: KR, JP" /></div>
            <div className="ui-field"><label>허락 사용 유형 (비우면 제한 없음)</label>
              <input className="ui-input" value={g.uses} onChange={e => set(i, { uses: e.target.value })} placeholder="예: OnDemandStream, PermanentDownload" /></div>
          </>}
          <div className="ui-field"><label>시작일</label><input className="ui-input" type="date" value={g.start} onChange={e => set(i, { start: e.target.value })} /></div>
          <div className="ui-field"><label>종료일 (마지막 날)</label><input className="ui-input" type="date" value={g.end} onChange={e => set(i, { end: e.target.value })} /></div>
          {rows.length > 1 && <button type="button" className="ui-btn is-soft is-sm" onClick={() => onChange(rows.filter((_, n) => n !== i))}>삭제</button>}
        </fieldset>
      ))}
      <button type="button" className="ui-btn is-soft is-sm" onClick={() => onChange([...rows, newGrant(rows[0]?.holder)])}>+ 권한 추가</button>
    </div>
  );
}

function ReviewForm({ doc, status, onDone }: { doc: StaffDocument; status: 'APPROVED' | 'NEEDS'; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const needsGrants = status === 'APPROVED' && !!doc.release_id;
  const [grants, setGrants] = useState<GrantRow[]>([newGrant(doc.electronic_record?.rights_holder ?? '')]);
  const grantsOk = !needsGrants || grants.every(grantOk);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (status === 'NEEDS' && !note.trim()) || !grantsOk) return;
    setBusy(true);
    try {
      await staffApi.reviewDocument(doc.id, { status, note: note.trim(), row_version: doc.row_version, grants: needsGrants ? grants.map(toGrant) : undefined });
      toast(status === 'APPROVED' ? '서류를 승인했어요.' : '보완 요청을 보냈어요. 아티스트에게 알림이 갔어요.', 'success');
      onDone();
      close();
    } catch (err) {
      toast(errorMessage(err, '서류 처리에 실패했어요.'), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit}>
      <p className="small muted">{doc.org_name} · {DOC_KIND[doc.kind] ?? doc.kind}{doc.release_title ? ` · ${doc.release_title}` : ''}</p>
      <div className="ui-field" style={{ marginTop: 16 }}>
        <label htmlFor="docNote">{status === 'NEEDS' ? '보완 요청 내용' : '메모 (선택)'}{status === 'NEEDS' && <span className="required"> *</span>}</label>
        <textarea
          id="docNote" data-autofocus className="ui-textarea" rows={4} maxLength={1000} required={status === 'NEEDS'} value={note}
          placeholder={status === 'NEEDS' ? '무엇이 부족한지, 어떻게 다시 내면 되는지 적어 주세요. 아티스트 화면에 그대로 보여요.' : '내부 확인용 메모'}
          onChange={e => setNote(e.target.value)}
        />
        <small className="adm-counter">{note.length} / 1000</small>
      </div>
      {needsGrants && <GrantRows rows={grants} onChange={setGrants} />}
      <div className="adm-form-actions">
        <button type="button" className="ui-btn is-soft" onClick={close}>취소</button>
        <button type="submit" className={`ui-btn ${status === 'NEEDS' ? 'is-warn' : 'is-primary'}`} disabled={busy || (status === 'NEEDS' && !note.trim()) || !grantsOk}>
          {busy ? '저장 중…' : status === 'NEEDS' ? '보완 요청' : '승인'}
        </button>
      </div>
    </form>
  );
}

export function Documents() {
  const [params, setParams] = useSearchParams();
  const status = params.get('status') || 'REVIEW';
  const { can, refreshCounts } = useStaff();
  const { data, loading, error, reload } = useAsync(() => staffApi.documents(status), [status]);
  const [pending, setPending] = useState<{ doc: StaffDocument; status: 'APPROVED' | 'NEEDS' } | null>(null);
  const [viewing, setViewing] = useState<StaffDocument | null>(null);
  const items = data?.items ?? [];

  return (
    <div className="view-enter">
      <PageHeader
        eyebrow="발매 심사"
        title="서류 검토"
        sub="심사 중 요청한 권리 증빙을 확인해요. 결과는 아티스트에게 바로 알려져요."
        actions={<button type="button" className="ui-btn is-soft is-sm" onClick={reload}>새로고침</button>}
      />
      {!can('DOCUMENTS') && <NoDuty duty="서류 검토" />}
      <ChipGroup label="서류 상태" value={status} onChange={v => setParams({ status: v }, { replace: true })} options={STATUSES.map(s => ({ value: s, label: DOC_STATUS[s][0] }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <SkeletonList rows={3} /> : items.length === 0 ? (
        <EmptyState icon={<Glyph name="doc" size={22} />} title="해당 상태의 서류가 없어요" />
      ) : (
        <div className="ui-list">
          {items.map(d => (
            <div key={d.id} className="ui-row adm-doc-row">
              <Cover text={DOC_KIND[d.kind] ?? d.kind} plain />
              <span className="adm-min">
                <span className="ui-row-title">{d.title}</span>
                <span className="ui-row-meta">
                  <span>{DOC_KIND[d.kind] ?? d.kind}</span>
                  <span>{d.org_name}</span>
                  {d.release_id && <span><Link to={`/admin/reviews/${d.release_id}`}>{d.release_title ?? '발매'} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link></span>}
                  {d.file_name && <span>{d.file_name}</span>}
                  <span>{ago(d.updated_at) || when(d.updated_at)}</span>
                </span>
                {d.review_note && <span className="ui-row-meta"><span>담당자 메모: {d.review_note}</span></span>}
              </span>
              <span className="ui-row-end">
                <StatusBadge value={pick(DOC_STATUS, d.status)} />
              </span>
              {/* 버튼은 카드 아래 한 줄로 — 휴대폰에서 오른쪽에 몰리지 않게 */}
              <div className="adm-doc-actions">
                <DocFileLink id={d.id} assetId={d.asset_id} />
                {d.electronic_record && <button type="button" className="ui-btn is-soft is-sm" onClick={() => setViewing(d)}>서명 문서 보기</button>}
                {decidable(d) && can('DOCUMENTS') && <>
                  <button type="button" className="ui-btn is-warn is-sm" onClick={() => setPending({ doc: d, status: 'NEEDS' })}>보완 요청</button>
                  <button type="button" className="ui-btn is-primary is-sm" onClick={() => setPending({ doc: d, status: 'APPROVED' })}>승인</button>
                </>}
              </div>
            </div>
          ))}
        </div>
      )}
      {viewing && <Modal title={viewing.title} onClose={() => setViewing(null)}>
        <p className="small muted">권리자: {viewing.electronic_record?.rights_holder} · 서명자: {viewing.signer_name} ({viewing.electronic_record?.signer_role}) · {when(viewing.signed_at)}</p>
        <div className="aq-document-snapshot">{viewing.body}</div>
        {viewing.signature && <div className="aq-sign-record"><img src={viewing.signature} alt={`${viewing.signer_name} 서명`} /></div>}
        {viewing.electronic_record?.identity
          ? <p className="small break">본인확인: {viewing.electronic_record.identity.name} · {when(viewing.electronic_record.identity.verified_at)} · {viewing.electronic_record.channel === 'IN_PERSON' ? '대면(같은 기기)' : '서명 링크'}로 권리자 직접 서명</p>
          : <p className="small break">본인확인 없이 제출자 기기에서 서명한 이전 서식(1.0) 문서예요.</p>}
        <p className="small break">문서 확인 코드: {viewing.electronic_record?.content_hash}</p>
        {viewing.electronic_record?.certificate_hash && <p className="small break">서명 증명 해시: {viewing.electronic_record.certificate_hash}</p>}
      </Modal>}
      {pending && (
        <Modal title={pending.status === 'APPROVED' ? `‘${pending.doc.title}’ 승인` : `‘${pending.doc.title}’ 보완 요청`} onClose={() => setPending(null)} dismissible={false}>
          <ReviewForm doc={pending.doc} status={pending.status} onDone={() => { reload(); refreshCounts(); }} />
        </Modal>
      )}
    </div>
  );
}
