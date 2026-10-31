// 계정 정보 '퍼블리싱' 카드 — 아티스트와 수익 받기 사이. 한 계정이 아티스트·작곡가·작사가·레이블 담당 역할을 함께 가진다 (기획안 5.1).
// 따로 만든 모양 없이 계정 정보와 같은 줄(SetRow)·같은 창(한 항목만 고치는 창, 고를 것이 적으면 목록을 바로)을 쓴다.
// 활동명·활동 국가는 위 '아티스트', 법적 이름은 '본인 확인'의 실명을 그대로 쓴다 — 같은 정보를 두 번 받지 않는다.
// 관리 단체·IPI는 곡을 만들 때 참여자 칸에 자동으로 들어간다
import { useEffect, useState } from 'react';
import { pubApi, type Creator, type CreatorRole } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { SOCIETIES, societyName } from '../../lib/publishing';
import { useToast } from '../Toast';
import { Modal, useModalClose } from '../Modal';
import { Glyph } from '../Glyph';
import { SetRow } from '../SetRow';
import { SocietyJoin } from './SocietyJoin';
import '../../styles/split/Publishing.css';

const ROLES: [CreatorRole, string][] = [['artist', '아티스트'], ['composer', '작곡가'], ['lyricist', '작사가'], ['arranger', '편곡가'], ['producer', '프로듀서'], ['label', '레이블 담당자']];
type Field = 'roles' | 'society' | 'ipi' | 'org' | 'pen';
const TITLE: Record<Field, string> = { roles: '역할', society: '저작권 관리 단체', ipi: 'IPI/CAE 번호', org: '소속 조직', pen: '작가명 (필명)' };
const key = (v: string) => v.replace(/\s+/g, '').toLowerCase();

/** 필명 — 활동명과 다른 이름으로 곡에 올라가는 경우. 크레딧·참여자에 이 이름이 있으면 다른 사람이 아니라 나로 본다 (0176) */
function PenNameEditor({ c, onSave }: { c: Creator; onSave: (next: Creator) => Promise<boolean> }) {
  const close = useModalClose();
  const [list, setList] = useState<string[]>(c.penNames ?? []);
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const add = () => {
    const v = text.trim();
    if (!v || list.some(x => key(x) === key(v)) || key(v) === key(c.name)) { setText(''); return; }
    setList(l => [...l, v].slice(0, 5));
    setText('');
  };
  const same = JSON.stringify(list) === JSON.stringify(c.penNames ?? []);
  return (
    <div className="aq-set-editor aq-pen-editor">
      <p className="ui-field-help">발매 크레딧이나 관리 단체에 활동명(‘{c.name}’)과 다른 이름으로 올라간 적이 있으면 적어 주세요. 그 이름으로 적힌 작사·작곡은 다른 사람이 아니라 내 지분으로 처리해요. 관리 단체에 필명으로 등록했다면 그 이름 그대로 적어 주세요.</p>
      {list.length > 0 && (
        <ul className="aq-pen-list">
          {list.map(n => (
            <li key={n}><span>{n}</span>
              <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => setList(l => l.filter(x => x !== n))} aria-label={`${n} 빼기`}>빼기</button>
            </li>
          ))}
        </ul>
      )}
      {list.length < 5 && (
        <form className="aq-pen-add" onSubmit={e => { e.preventDefault(); add(); }}>
          <label htmlFor="creator-pen" className="sr-only">필명 더하기</label>
          <input id="creator-pen" className="ui-input" maxLength={100} placeholder="예) 하람, HARAM" value={text} onChange={e => setText(e.target.value)} />
          <button type="submit" className="ui-btn is-soft" disabled={!text.trim()}>더하기</button>
        </form>
      )}
      <button type="button" className={`ui-btn is-primary is-block${saving ? ' is-busy' : ''}`} disabled={saving || same}
        onClick={async () => { setSaving(true); const ok = await onSave({ ...c, penNames: list }); setSaving(false); if (ok) close(); }}>
        {saving ? '저장하는 중' : '저장'}
      </button>
    </div>
  );
}

/** 한 항목만 고치는 창 — 계정 정보의 FieldEditor와 같은 틀(aq-set-editor) */
function CreatorFieldEditor({ field, c, onSave }: { field: Field; c: Creator; onSave: (next: Creator) => Promise<boolean> }) {
  if (field === 'pen') return <PenNameEditor c={c} onSave={onSave} />;
  return <PlainFieldEditor field={field} c={c} onSave={onSave} />;
}

function PlainFieldEditor({ field, c, onSave }: { field: Exclude<Field, 'pen'>; c: Creator; onSave: (next: Creator) => Promise<boolean> }) {
  const close = useModalClose();
  const [roles, setRoles] = useState<CreatorRole[]>(c.roles);
  const [text, setText] = useState(String((field === 'ipi' ? c.ipi : c.org) ?? ''));
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const save = async (next: Creator) => { setSaving(true); const ok = await onSave(next); setSaving(false); if (ok) close(); };

  // 관리 단체는 고를 것이 정해져 있다 — 활동 국가처럼 목록을 바로 보여 누르면 저장
  if (field === 'society') {
    const current = c.society ?? '';
    return (
      <div className="aq-set-editor aq-country-list" role="radiogroup" aria-label={TITLE.society}>
        {[['', '가입하지 않았어요'] as const, ...SOCIETIES.filter(s => s !== '없음').map(s => [s, societyName(s)] as const)].map(([v, label]) => (
          <button key={v || 'none'} type="button" role="radio" aria-checked={v === current} disabled={saving}
            className={`aq-picker-opt${v === current ? ' is-on' : ''}`}
            onClick={() => (v === current ? close() : void save({ ...c, society: v || null }))}>
            <span>{label}</span>
            {v === current && <Glyph name="check" size={18} />}
          </button>
        ))}
      </div>
    );
  }

  if (field === 'roles') {
    return (
      <div className="aq-set-editor">
        <div className="aq-country-list" role="group" aria-label="역할 (여러 개)">
          {ROLES.map(([r, label]) => {
            const on = roles.includes(r);
            return (
              <button key={r} type="button" role="checkbox" aria-checked={on} className={`aq-picker-opt${on ? ' is-on' : ''}`}
                onClick={() => setRoles(x => (on ? x.filter(y => y !== r) : [...x, r]))}>
                <span>{label}</span>
                {on && <Glyph name="check" size={18} />}
              </button>
            );
          })}
        </div>
        <p className="ui-field-help">여러 개를 고를 수 있어요. 역할마다 계정을 따로 만들지 않아요.</p>
        <button type="button" className={`ui-btn is-primary is-block${saving ? ' is-busy' : ''}`} disabled={saving || !roles.length}
          onClick={() => void save({ ...c, roles })}>{saving ? '저장하는 중' : '저장'}</button>
      </div>
    );
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const v = text.trim();
    if (field === 'ipi' && v && !/^\d{9,11}$/.test(v)) { setError('IPI 번호는 숫자 9~11자리예요.'); return; }
    await save({ ...c, [field]: v || null });
  };
  return (
    <form className="aq-set-editor" onSubmit={submit} noValidate>
      <div className="ui-field">
        <label htmlFor={`creator-${field}`} className="sr-only">{TITLE[field]}</label>
        <input id={`creator-${field}`} autoFocus inputMode={field === 'ipi' ? 'numeric' : undefined} maxLength={field === 'ipi' ? 11 : 120}
          placeholder={field === 'ipi' ? '00712345678' : '소속 퍼블리셔·레이블'} aria-invalid={!!error}
          value={text} onChange={e => { setText(field === 'ipi' ? e.target.value.replace(/\D/g, '') : e.target.value); setError(''); }} />
        {error ? <p className="ui-field-help aq-help-error">{error}</p>
          : field === 'ipi' && <p className="ui-field-help">관리 단체에 가입하면 받는 작가 번호예요. 모르면 비워 두세요.</p>}
      </div>
      <button type="submit" className={`ui-btn is-primary is-block${saving ? ' is-busy' : ''}`} disabled={saving || text.trim() === String((field === 'ipi' ? c.ipi : c.org) ?? '')}>
        {saving ? '저장하는 중' : '저장'}
      </button>
    </form>
  );
}

/** 퍼블리싱 카드 — 줄마다 한 항목만 고치는 창. 곡에서 처음 적은 관리 단체·IPI도 여기 저장돼 다음 곡에 채워진다 */
export function CreatorProfile({ openEmpty = false, onOpened }: { openEmpty?: boolean; onOpened?: () => void } = {}) {
  const toast = useToast();
  const { data, error } = useAsync(() => pubApi.myCreator(), [], { key: 'my-creator' });
  const [local, setLocal] = useState<Creator | null>(null);
  const [editing, setEditing] = useState<Field | null>(null);
  const c = local ?? data;
  // 홈 '작가 정보 채우기'에서 왔으면 첫 빈 칸(역할 → IPI)을 바로 연다
  useEffect(() => {
    if (!openEmpty || !c) return;
    setEditing(!c.roles.length ? 'roles' : c.society && !c.ipi ? 'ipi' : 'society');
    onOpened?.();
  }, [openEmpty, c]); // eslint-disable-line react-hooks/exhaustive-deps
  // 저작권 관리 API가 없는 서버는 이 묶음을 그리지 않는다
  if (error || !c) return null;
  const roleText = c.roles.map(r => ROLES.find(x => x[0] === r)?.[1]).filter(Boolean).join(' · ');
  const save = async (next: Creator) => {
    try { setLocal(await pubApi.saveMyCreator(next)); toast('저장했어요.'); return true; }
    catch (e) { toast(errorMessage(e, '저장하지 못했어요. 잠시 후 다시 시도해 주세요.'), 'error'); return false; }
  };
  return (
    <section className="aq-set-card" aria-labelledby="setPublishing">
      <h2 id="setPublishing">퍼블리싱</h2>
      <SetRow label="역할 (작곡·작사 등)" value={roleText} empty="고르기" onClick={() => setEditing('roles')} />
      <SetRow label="저작권 관리 단체" value={societyName(c.society) || undefined} empty="가입하지 않음" onClick={() => setEditing('society')} />
      {!c.society && <SocietyJoin inline />}
      <SetRow label="IPI/CAE 번호" value={c.ipi ?? undefined} empty="입력하기" onClick={() => setEditing('ipi')} />
      <SetRow label="작가명 (필명)" value={c.penNames?.length ? c.penNames.join(' · ') : undefined} empty="없음" onClick={() => setEditing('pen')} />
      <SetRow label="소속 조직" value={c.org ?? undefined} empty="없음" onClick={() => setEditing('org')} />
      {editing && (
        <Modal title={TITLE[editing]} onClose={() => setEditing(null)} modalClass="aq-set-modal">
          <CreatorFieldEditor field={editing} c={c} onSave={save} />
        </Modal>
      )}
    </section>
  );
}
