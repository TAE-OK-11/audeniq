// 곡 첨부 자료 — 곡 화면 안에서 바로 확인한다: 데모는 재생, 가사는 본문, 악보·증빙은 이미지·PDF 미리보기.
// 올리기는 '자료 첨부' 창 하나로 — 앨범 퍼블리싱 탭과 곡 상세가 같은 창을 쓴다.
// 줄 모양은 발매 상세 '권리 서류'(ui-row)와 같다. 미리보기는 그 줄 아래로 펼친다 (Reveal)
import { useState, type ChangeEvent } from 'react';
import { api } from '../../api/client';
import { pubApi, type Work, type WorkAttachment } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { MOCK } from '../../lib/mode';
import { fileSize } from '../../lib/format';
import { Glyph, type GlyphName } from '../Glyph';
import { Modal } from '../Modal';
import { FilePicker } from '../FilePicker';
import { useToast } from '../Toast';
import { Field, IconTile, Reveal, Segmented } from '../../ui';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

export const KIND_LABEL: Record<WorkAttachment['kind'], string> = { demo: '데모 음원', lyrics: '가사', score: '악보', proof: '권리 증빙' };
const KIND_ICON: Record<WorkAttachment['kind'], GlyphName> = { demo: 'music', lyrics: 'quote', score: 'doc', proof: 'sign' };
const KIND_ACCEPT: Record<WorkAttachment['kind'], string> = {
  demo: 'audio/*,.mp3,.wav,.m4a,.flac', lyrics: '.txt,.pdf,.doc,.docx,text/plain', score: '.pdf,image/*', proof: '.pdf,image/*',
};
const KINDS = Object.keys(KIND_LABEL) as WorkAttachment['kind'][];
const isImage = (n: string) => /\.(png|jpe?g|gif|webp|heic)$/i.test(n);
const isPdf = (n: string) => /\.pdf$/i.test(n);

/** 한 자료의 미리보기 — 열 수 없는 형식이면 그렇다고 말하고 받기만 남긴다 */
function Preview({ a }: { a: WorkAttachment }) {
  const [broken, setBroken] = useState(false);
  if (a.text) return <p className="aq-track-sheet-lyrics aq-pbl-file-text">{a.text}</p>;
  if (!a.url || broken) {
    return <p className="aq-empty-val aq-pbl-file-none">{a.url ? '미리보기를 불러오지 못했어요. 새로 올리면 다시 볼 수 있어요.' : '이 파일은 아직 미리보기가 준비되지 않았어요.'}</p>;
  }
  if (a.kind === 'demo') return <audio className="aq-pbl-file-audio" controls preload="none" src={a.url} onError={() => setBroken(true)}>{a.name}</audio>;
  if (isImage(a.name)) return <img className="aq-pbl-file-img" src={a.url} alt={`${a.name} 미리보기`} onError={() => setBroken(true)} />;
  if (isPdf(a.name)) return <iframe className="aq-pbl-file-pdf" src={a.url} title={`${a.name} 미리보기`} />;
  return <p className="aq-empty-val aq-pbl-file-none">이 형식은 화면에서 바로 열 수 없어요.</p>;
}

/** 자료 목록 — 누르면 그 줄 아래에서 재생·열람 */
export function AttachmentList({ items, empty = '아직 올린 자료가 없어요. 데모·가사·악보·권리 증빙을 모아 둘 수 있어요.' }: { items: readonly WorkAttachment[]; empty?: string }) {
  const [open, setOpen] = useState<string | null>(null);
  if (!items.length) return <p className="aq-empty-val">{empty}</p>;
  return (
    <ul className="ui-list is-inset aq-pbl-files">
      {items.map(a => (
        <li key={a.id}>
          <button type="button" className="ui-row" aria-expanded={open === a.id} onClick={() => setOpen(v => (v === a.id ? null : a.id))}>
            <IconTile size="sm" tone={a.kind === 'proof' ? 'gray' : undefined}><Glyph name={KIND_ICON[a.kind]} size={16} /></IconTile>
            <span className="min-0">
              <span className="ui-row-title">{a.name}</span>
              <span className="ui-row-meta"><span>{KIND_LABEL[a.kind]}</span>{a.size ? <span>{fileSize(a.size)}</span> : null}</span>
            </span>
            <span className="aq-pbl-file-go">{a.kind === 'demo' ? '재생' : '보기'}<Glyph name="chevron-down" size={14} className={open === a.id ? 'is-flip' : undefined} /></span>
          </button>
          <Reveal open={open === a.id}>
            <div className="aq-pbl-file-preview">
              <Preview a={a} />
              {a.url && <a className="ui-btn is-text is-sm" href={a.url} target="_blank" rel="noopener noreferrer">새 창에서 열기<Glyph name="arrow-up-right" size={13} /></a>}
            </div>
          </Reveal>
        </li>
      ))}
    </ul>
  );
}

/** 자료 첨부 창 — 곡(여러 곡이면 고르기) · 자료 종류 · 파일. 올리면 그 곡의 자료 목록에 바로 보인다 */
export function AttachmentModal({ works, workId, onClose, onAdded }: {
  works: readonly Work[];
  /** 정해진 곡 — 없으면 창에서 고른다 */
  workId?: string;
  onClose: () => void;
  onAdded: (w: Work) => void;
}) {
  const toast = useToast();
  const editable = works.filter(w => w.myRole === 'owner' || w.myRole === 'editor');
  const [target, setTarget] = useState(workId ?? editable[0]?.id ?? '');
  const [kind, setKind] = useState<WorkAttachment['kind']>('demo');
  const [busy, setBusy] = useState(false);
  const work = works.find(w => w.id === target);

  const pick = async (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = '';
    if (!f || !work) return;
    setBusy(true);
    try {
      const up = await api.uploadFile(f, kind === 'demo' ? 'AUDIO' : 'DOCUMENT');
      // 가사처럼 글자 파일은 본문을 그대로 남겨 곡 화면에서 바로 읽는다. 체험 모드는 브라우저 안 주소로 미리 본다
      const text = f.size < 200_000 && (f.type.startsWith('text/') || /\.txt$/i.test(f.name)) ? await f.text() : null;
      const w = await pubApi.addAttachment(work.id, kind, f.name, f.size, { text, url: MOCK ? URL.createObjectURL(f) : null, assetId: up.assetId });
      toast(`‘${work.title}’에 ${KIND_LABEL[kind]}을(를) 올렸어요.`, 'success');
      onAdded(w);
      onClose();
    } catch (err) {
      toast(errorMessage(err, '파일을 올리지 못했어요. 다시 시도해 주세요.'), 'error');
    } finally { setBusy(false); }
  };

  return (
    <Modal title="자료 첨부" onClose={onClose} dismissible={!busy} closeDisabled={busy}>
      <div className="aq-track-sheet">
        <p className="ui-field-help">데모·가사·악보·권리 증빙을 곡에 붙여 두면, 신청할 때와 담당자 확인 때 다시 내지 않아도 돼요.</p>
        {!workId && (
          <Field label="곡" htmlFor="attachWork">
            <select id="attachWork" className="ui-select" value={target} onChange={e => setTarget(e.target.value)} disabled={busy}>
              {editable.map(w => <option key={w.id} value={w.id}>{w.title || '제목 없는 곡'}</option>)}
            </select>
          </Field>
        )}
        <Field label="자료 종류">
          <Segmented label="자료 종류" value={kind} onChange={setKind} fill size="sm" options={KINDS.map(k => ({ value: k, label: KIND_LABEL[k] }))} />
        </Field>
        <Field label="파일" htmlFor="attachFile" help={kind === 'demo' ? 'MP3·WAV·M4A — 곡 화면에서 바로 들을 수 있어요.' : kind === 'lyrics' ? 'TXT로 올리면 곡 화면에서 바로 읽을 수 있어요.' : 'PDF·이미지 — 곡 화면에서 바로 열어 볼 수 있어요.'}>
          <FilePicker id="attachFile" accept={KIND_ACCEPT[kind]} busy={busy} disabled={!work} onChange={e => void pick(e)} />
        </Field>
        {work && work.attachments.length > 0 && (
          <>
            <h3 className="aq-track-sheet-h">‘{work.title}’에 올린 자료 {work.attachments.length}개</h3>
            <AttachmentList items={work.attachments} />
          </>
        )}
      </div>
    </Modal>
  );
}
