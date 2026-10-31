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
import { Field, IconTile, Reveal } from '../../ui';
import '../../styles/split/ReleaseDetail.css';
import '../../styles/split/Publishing.css';

// 작업 파일(DAW 프로젝트·MIDI·스템·버전별 음원)과 공동 작곡 합의서는 '내가 만들었다'는 근거 (0176) — 관리 단체가 창작 과정을 물을 때 낸다
export const KIND_LABEL: Record<WorkAttachment['kind'], string> = {
  demo: '데모 음원', lyrics: '가사', score: '악보', project: '작업 파일', split_sheet: '공동 작곡 합의서', proof: '기타 증빙',
};
const KIND_HELP: Record<WorkAttachment['kind'], string> = {
  demo: 'MP3·WAV·M4A — 곡 화면에서 바로 들을 수 있어요.',
  lyrics: 'TXT로 올리면 곡 화면에서 바로 읽을 수 있어요.',
  score: 'PDF·이미지 — 곡 화면에서 바로 열어 볼 수 있어요.',
  project: 'DAW 화면 캡처, 버전별 음원, MIDI·스템 — 날짜가 보이면 더 좋아요. 파일이 없으면 작업 기록을 글로 남겨도 돼요.',
  split_sheet: '공동 작가 모두가 서명한 지분 합의서(PDF·사진) — 분쟁 때 가장 강한 근거예요.',
  proof: '계약서·기존 등록 확인서 등 그 밖의 근거 — PDF·이미지.',
};
const KIND_ICON: Record<WorkAttachment['kind'], GlyphName> = { demo: 'music', lyrics: 'quote', score: 'doc', project: 'layers', split_sheet: 'users', proof: 'sign' };
const KIND_ACCEPT: Record<WorkAttachment['kind'], string> = {
  demo: 'audio/*,.mp3,.wav,.m4a,.flac', lyrics: '.txt,.pdf,.doc,.docx,text/plain', score: '.pdf,image/*',
  project: 'audio/*,image/*,.pdf,.mid,.midi', split_sheet: '.pdf,image/*', proof: '.pdf,image/*',
};
/** 파일 대신 글로 남길 수 있는 자료 — 가사 본문, 작업 기록 */
const TEXT_KINDS: Partial<Record<WorkAttachment['kind'], { label: string; help: string; placeholder: string; name: string }>> = {
  lyrics: { label: '가사 붙여 넣기', help: '여기에 붙여 넣으면 곡 화면에서 바로 읽을 수 있어요. 파일이 있으면 아래에서 올려도 돼요.', placeholder: '1절\n…\n\n후렴\n…', name: '가사' },
  project: { label: '작업 기록 남기기', help: '언제, 어떤 프로그램으로, 어떻게 만들었는지 — 버전별 날짜가 있으면 함께 적어 주세요.', placeholder: '2026-03-02 Logic에서 피아노 스케치\n2026-03-10 가사 붙이고 1차 편곡\n2026-04-01 최종 믹스', name: '작업 기록' },
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
export function AttachmentList({ items, empty = '아직 올린 자료가 없어요. 데모·가사·악보·작업 파일·합의서를 모아 둘 수 있어요.' }: { items: readonly WorkAttachment[]; empty?: string }) {
  const [open, setOpen] = useState<string | null>(null);
  if (!items.length) return <p className="ui-field-help aq-pbl-files-empty">{empty}</p>;
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
  const [text, setText] = useState('');
  const work = works.find(w => w.id === target);

  // 가사·작업 기록은 붙여 넣어도 된다 — 메모장에 저장해 파일로 올리는 수고 없이, 곡 화면에서 바로 읽히는 본문으로
  const textKind = TEXT_KINDS[kind];
  const saveText = async () => {
    const body = text.trim();
    if (!body || !work || busy || !textKind) return;
    setBusy(true);
    try {
      const w = await pubApi.addAttachment(work.id, kind, `${work.title || '곡'} ${textKind.name}`, new Blob([body]).size, { text: body });
      toast(`‘${work.title}’에 ${textKind.name}을(를) 남겼어요.`, 'success');
      onAdded(w);
      onClose();
    } catch (err) {
      toast(errorMessage(err, '저장하지 못했어요. 다시 시도해 주세요.'), 'error');
    } finally { setBusy(false); }
  };

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
        <p className="ui-field-help">데모·가사·악보·작업 파일·합의서를 곡에 붙여 두면 내가 만든 곡이라는 근거가 돼요. 신청할 때와 담당자 확인 때 다시 내지 않아도 돼요.</p>
        {!workId && (
          <Field label="곡" htmlFor="attachWork">
            <select id="attachWork" className="ui-select" value={target} onChange={e => setTarget(e.target.value)} disabled={busy}>
              {editable.map(w => <option key={w.id} value={w.id}>{w.title || '제목 없는 곡'}</option>)}
            </select>
          </Field>
        )}
        {/* 종류가 여섯 가지라 버튼 줄은 휴대폰에서 넘친다 — 고르기 칸으로 */}
        <Field label="자료 종류" htmlFor="attachKind">
          <select id="attachKind" className="ui-select" value={kind} onChange={e => { setKind(e.target.value as WorkAttachment['kind']); setText(''); }} disabled={busy}>
            {KINDS.map(k => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
          </select>
        </Field>
        {textKind && (
          <Field label={textKind.label} htmlFor="attachText" help={textKind.help}>
            <textarea id="attachText" className="ui-textarea" rows={kind === 'lyrics' ? 8 : 5} maxLength={20000} value={text} onChange={e => setText(e.target.value)} disabled={busy} placeholder={textKind.placeholder} />
            <button type="button" className={`ui-btn is-primary is-block${busy ? ' is-busy' : ''}`} disabled={!text.trim() || !work || busy} onClick={() => void saveText()}>{textKind.name} 저장</button>
          </Field>
        )}
        <Field label="파일" htmlFor="attachFile" help={KIND_HELP[kind]}>
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
