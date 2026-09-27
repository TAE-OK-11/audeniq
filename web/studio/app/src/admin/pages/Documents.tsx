// 서류 검토 — 담당자가 요청한 권리 증빙을 승인하거나 보완 요청한다.
// 발매 신청서(배급 계약서)는 발매 심사에서 발매와 함께 결정한다.
import { useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { Modal, useModalClose } from '../../components/Modal';
import { useToast } from '../../components/Toast';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { staffApi, type StaffDocument } from '../api';
import { DOC_KIND, DOC_STATUS, ago, pick, when } from '../labels';
import { Empty, ErrorBox, Filters, Initial, NoDuty, PageHead, Skeleton, StatusChip, useStaff } from '../ui';
import { Glyph } from '../../components/Glyph';

const STATUSES = ['REVIEW', 'AWAITING_DOCUMENTS', 'NEEDS', 'APPROVED'];
const decidable = (d: StaffDocument) => d.kind === 'RIGHTS_PROOF' && d.status === 'REVIEW';

function ReviewForm({ doc, status, onDone }: { doc: StaffDocument; status: 'APPROVED' | 'NEEDS'; onDone: () => void }) {
  const close = useModalClose();
  const toast = useToast();
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy || (status === 'NEEDS' && !note.trim())) return;
    setBusy(true);
    try {
      await staffApi.reviewDocument(doc.id, { status, note: note.trim(), row_version: doc.row_version });
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
      <div className="adm-field" style={{ marginTop: 16 }}>
        <label htmlFor="docNote">{status === 'NEEDS' ? '보완 요청 내용' : '메모 (선택)'}{status === 'NEEDS' && <span className="required"> *</span>}</label>
        <textarea
          id="docNote" data-autofocus className="adm-textarea" rows={4} maxLength={1000} required={status === 'NEEDS'} value={note}
          placeholder={status === 'NEEDS' ? '무엇이 부족한지, 어떻게 다시 내면 되는지 적어 주세요. 아티스트 화면에 그대로 보여요.' : '내부 확인용 메모'}
          onChange={e => setNote(e.target.value)}
        />
        <small className="adm-counter">{note.length} / 1000</small>
      </div>
      <div className="adm-form-actions">
        <button type="button" className="adm-btn soft" onClick={close}>취소</button>
        <button type="submit" className={`adm-btn ${status === 'NEEDS' ? 'warn' : 'primary'}`} disabled={busy || (status === 'NEEDS' && !note.trim())}>
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
  const items = data?.items ?? [];

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="DOCUMENTS"
        title="서류 검토"
        sub="발매 심사 중 요청한 권리 증빙이에요. 승인하거나 보완을 요청하면 작업 공간에 알림이 가요. 새 발매 신청서는 발매 심사에서 결정해요."
        actions={<button type="button" className="adm-btn soft small" onClick={reload}>새로고침</button>}
      />
      {!can('DOCUMENTS') && <NoDuty duty="서류 검토" />}
      <Filters label="서류 상태" value={status} onChange={v => setParams({ status: v }, { replace: true })} options={STATUSES.map(s => ({ value: s, label: DOC_STATUS[s][0] }))} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !data ? <Skeleton rows={3} /> : items.length === 0 ? (
        <Empty icon={<Glyph name="doc" size={22} />} title="해당 상태의 서류가 없어요" />
      ) : (
        <div className="adm-list">
          {items.map(d => (
            <div key={d.id} className="adm-row">
              <Initial text={DOC_KIND[d.kind] ?? d.kind} plain />
              <span className="adm-min">
                <span className="adm-row-title">{d.title}</span>
                <span className="adm-row-meta">
                  <span>{DOC_KIND[d.kind] ?? d.kind}</span>
                  <span>{d.org_name}</span>
                  {d.release_id && <span><Link to={`/admin/reviews/${d.release_id}`}>{d.release_title ?? '발매'} <Glyph name="arrow-right" size={13} className="aq-inline-glyph" /></Link></span>}
                  {d.file_name && <span>{d.file_name}</span>}
                  <span>{ago(d.updated_at) || when(d.updated_at)}</span>
                </span>
                {d.review_note && <span className="adm-row-meta"><span>담당자 메모: {d.review_note}</span></span>}
              </span>
              <span className="adm-row-end">
                <StatusChip value={pick(DOC_STATUS, d.status)} />
                {decidable(d) && can('DOCUMENTS') && (
                  <span className="adm-codes">
                    <button type="button" className="adm-btn warn small" onClick={() => setPending({ doc: d, status: 'NEEDS' })}>보완 요청</button>
                    <button type="button" className="adm-btn primary small" onClick={() => setPending({ doc: d, status: 'APPROVED' })}>승인</button>
                  </span>
                )}
              </span>
            </div>
          ))}
        </div>
      )}
      {pending && (
        <Modal title={pending.status === 'APPROVED' ? `‘${pending.doc.title}’ 승인` : `‘${pending.doc.title}’ 보완 요청`} onClose={() => setPending(null)} dismissible={false}>
          <ReviewForm doc={pending.doc} status={pending.status} onDone={() => { reload(); refreshCounts(); }} />
        </Modal>
      )}
    </div>
  );
}
