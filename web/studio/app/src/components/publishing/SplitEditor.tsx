// 참여자·지분 입력 — 발매 상세의 '공동작업자 수익 분배'(SplitsCard)와 같은 줄 모양(aq-splits-*, % 붙은 입력칸).
// 작가 검색으로 추가하고, 없으면 이름만으로 외부 참여자를 추가한다. 합계는 바로 계산하고 틀린 줄을 짚는다.
// 틀려도 임시 저장은 막지 않는다 — 신청할 때만 막는다 (기획안 5.2 · 5.3)
import { useDeferredValue, useEffect, useState } from 'react';
import { pubApi, type ContributorRole, type Creator, type Work, type WorkContributor } from '../../api/publishing';
import { uid } from '../../lib/store';
import { ROLE_LABEL, SOCIETIES, evenSplit, pct, societyName, splitIssues, sumShares } from '../../lib/publishing';
import { Badge, Field, IconTile } from '../../ui';
import { Glyph } from '../Glyph';
import { SplitBar, peopleLine, roleText } from './parts';
import { reuseContributors } from '../../lib/publishingPrep';
import { SocietyJoin } from './SocietyJoin';
import '../../styles/split/Publishing.css';

const ROLES = Object.keys(ROLE_LABEL) as ContributorRole[];

function CreatorSearch({ onPick, onExternal, taken }: { onPick: (c: Creator) => void; onExternal: (name: string) => void; taken: Set<string> }) {
  const [q, setQ] = useState('');
  const dq = useDeferredValue(q);
  const [items, setItems] = useState<Creator[] | null>(null);
  // 칸을 누르면 '최근 함께한 사람' — 같이 쓰는 사람을 매번 이름부터 치지 않게 (지난 곡의 관리 단체·IPI까지 함께 들어간다)
  const [recent, setRecent] = useState<Creator[] | null>(null);
  const [focused, setFocused] = useState(false);
  const loadRecent = () => { if (!recent) pubApi.searchCreators('').then(r => setRecent(r.filter(c => !c.isMe))).catch(() => setRecent([])); };
  useEffect(() => {
    if (!dq.trim()) { setItems(null); return; }
    let alive = true;
    pubApi.searchCreators(dq).then(r => { if (alive) setItems(r); }).catch(() => { if (alive) setItems([]); });
    return () => { alive = false; };
  }, [dq]);
  const add = (c: Creator) => { onPick(c); setQ(''); };
  return (
    <div className="aq-pbl-search">
      <input className="ui-input" type="search" value={q} onChange={e => setQ(e.target.value)} placeholder="작가 이름·IPI로 찾아 추가" aria-label="참여자 찾기"
        onFocus={() => { setFocused(true); loadRecent(); }} onBlur={() => window.setTimeout(() => setFocused(false), 150)}
        onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); const first = items?.find(c => !taken.has(c.id)); if (first) add(first); else if (q.trim()) { onExternal(q.trim()); setQ(''); } } }} />
      {!q.trim() && focused && !!recent?.filter(c => !taken.has(c.id)).length && (
        <div className="aq-pbl-recent" aria-label="최근 함께한 사람">
          <small>최근 함께한 사람</small>
          <div className="ui-chips is-wrap">
            {recent.filter(c => !taken.has(c.id)).slice(0, 8).map(c => (
              <button key={c.id} type="button" className="ui-chip is-sm" onMouseDown={e => e.preventDefault()} onClick={() => add(c)}
                title={[societyName(c.society), c.ipi && `IPI ${c.ipi}`].filter(Boolean).join(' · ') || undefined}>
                <Glyph name="plus" size={13} />{c.name}
              </button>
            ))}
          </div>
        </div>
      )}
      {q.trim() && items && (
        <ul className="ui-list aq-pbl-search-list" aria-label="찾은 작가">
          {items.map(c => (
            <li key={c.id}>
              <button type="button" className="ui-row" disabled={taken.has(c.id)} onClick={() => add(c)}>
                <IconTile size="sm"><Glyph name="user" size={16} /></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">{c.name}{c.isMe && ' (나)'}</span>
                  <span className="ui-row-meta">{[societyName(c.society), c.ipi && `IPI ${c.ipi}`, c.workCount != null && `곡 ${c.workCount}개`].filter(Boolean).map(t => <span key={String(t)}>{t}</span>)}</span>
                </span>
                {taken.has(c.id) ? <Badge tone="gray">추가됨</Badge> : <Glyph name="plus" size={16} />}
              </button>
            </li>
          ))}
          <li>
            <button type="button" className="ui-row" onClick={() => { onExternal(q.trim()); setQ(''); }}>
              <IconTile size="sm" tone="gray"><Glyph name="plus" size={16} /></IconTile>
              <span className="min-0">
                <span className="ui-row-title">‘{q.trim()}’ 외부 참여자로 추가</span>
                <span className="ui-row-meta"><span>계정이 없어도 돼요. 나중에 가입하면 본인 확인 후 연결돼요.</span></span>
              </span>
              <span />
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}

/** 이전 곡의 참여자 구성 가져오기 — 역할·단체·IPI만 옮기고 지분은 비워 둔다 (곡마다 다르니 직접 확인해서 넣는다) */
function ReuseFrom({ works, value, onChange }: { works: readonly Work[]; value: WorkContributor[]; onChange: (next: WorkContributor[]) => void }) {
  const [open, setOpen] = useState(false);
  if (!works.length) return null;
  return (
    <div className="aq-pbl-reuse">
      <button type="button" className="ui-btn is-soft is-sm" aria-expanded={open} onClick={() => setOpen(v => !v)}>
        <Glyph name="repeat" size={14} className="aq-glyph-lead" />이전 곡 참여자 가져오기
      </button>
      {open && (
        <ul className="ui-list aq-pbl-search-list" aria-label="참여자를 가져올 곡">
          {works.slice(0, 8).map(w => (
            <li key={w.id}>
              <button type="button" className="ui-row" onClick={() => { onChange(reuseContributors(w.contributors, value)); setOpen(false); }}>
                <IconTile size="sm"><Glyph name="users" size={16} /></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">{w.title || '제목 없는 곡'}</span>
                  <span className="ui-row-meta"><span>{peopleLine(w.contributors, 3)}</span><span>{w.contributors.map(roleText).join(' / ')}</span></span>
                </span>
                <Glyph name="plus" size={16} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function SplitEditor({ value, onChange, readOnly, onRequestConfirm, onSimulate, showErrors = false, reuse, focusId }: {
  value: WorkContributor[];
  onChange: (next: WorkContributor[]) => void;
  readOnly?: boolean;
  onRequestConfirm?: (c: WorkContributor) => void;
  onSimulate?: (c: WorkContributor) => void;
  showErrors?: boolean;
  /** 참여자를 가져올 수 있는 이전 곡 */
  reuse?: readonly Work[];
  /** 이 참여자 줄을 펼치고 그 칸으로 (준비 점검에서 눌러 들어왔을 때) */
  focusId?: string;
}) {
  const total = sumShares(value);
  const issues = splitIssues(value);
  const set = (id: string, patch: Partial<WorkContributor>) => onChange(value.map(c => (c.id === id ? { ...c, ...patch } : c)));
  const rest = Math.max(0, Math.round((100 - total) * 100) / 100);
  const fromCreator = (cr: Creator) => onChange([...value, {
    id: uid(), creatorId: cr.id, name: cr.name, society: cr.society, ipi: cr.ipi, external: false, isMe: cr.isMe,
    roles: (cr.roles as string[]).filter((r): r is ContributorRole => (ROLES as string[]).includes(r)).slice(0, 2),
    share: rest, publisherShare: 0, confirm: cr.isMe ? 'confirmed' : 'pending',
  }]);
  // 지난 곡에 같은 이름이 있으면 그때 적은 역할·관리 단체·IPI를 그대로 — 같은 사람 정보를 곡마다 다시 적지 않게
  const known = (name: string) => {
    const k = name.trim();
    const hits = (reuse ?? []).flatMap(w => w.contributors).filter(c => !c.isMe && c.name.trim() === k);
    if (!hits.length) return null;
    return { society: hits.find(c => c.society)?.society ?? null, ipi: hits.find(c => c.ipi)?.ipi ?? null, roles: hits[0].roles };
  };
  const external = (name: string) => {
    const k = known(name);
    onChange([...value, { id: uid(), creatorId: null, name, roles: k?.roles ?? [], society: k?.society ?? null, ipi: k?.ipi ?? null, external: true, share: rest, publisherShare: 0, confirm: 'pending' }]);
  };
  const even = () => { const s = evenSplit(value.length); onChange(value.map((c, i) => ({ ...c, share: s[i], publisherShare: Math.min(c.publisherShare, s[i]) }))); };
  const num = (v: string) => (v === '' ? 0 : Number(v.replace(/[^0-9.]/g, '')) || 0);
  const invalid = showErrors || total > 100;
  const imported = value.some(c => c.share === 0 && !c.isMe);

  return (
    <div className="aq-pbl-split">
      <div className="aq-pbl-split-total" aria-live="polite">
        <span>창작 지분 합계 <strong className={total > 100 ? 'aq-danger-text' : ''}>{pct(total)} <small>/ 100%</small></strong></span>
        {!readOnly && value.length > 1 && <button type="button" className="ui-btn is-soft is-sm" onClick={even}>균등 분배</button>}
      </div>
      <SplitBar list={value} />
      {!readOnly && total < 100 && <p className="ui-field-help">남은 {pct(rest)}를 참여자에게 나눠 주세요. 미완성 정보도 저장할 수 있어요.</p>}
      {!readOnly && imported && <p className="ui-field-help aq-pbl-import-note"><Glyph name="info" size={14} />가져온 참여자의 지분은 비어 있어요. 이번 곡의 몫을 직접 확인해서 적어 주세요.</p>}

      {/* 확인을 기다리는 사람이 여럿이면 한 번에 — 한 사람씩 '다시 보내기'를 누르지 않게 */}
      {readOnly && onRequestConfirm && (() => {
        const waiting = value.filter(c => !c.isMe && c.confirm === 'pending');
        return waiting.length > 1 && (
          <button type="button" className="ui-btn is-soft is-sm aq-pbl-remind-all" onClick={() => waiting.forEach(onRequestConfirm)}>
            <Glyph name="mail" size={14} className="aq-glyph-lead" />{waiting.length}명에게 확인 요청 모두 보내기
          </button>
        );
      })()}
      <div className="aq-pbl-people">
        {value.map((c, i) => {
          const errors = issues.filter(x => x.contributorId === c.id && x.level === 'error');
          const prefix = `pbl-person-${c.id}`;
          return readOnly ? (
            <div key={c.id} className="aq-pbl-person-read">
              <IconTile size="sm" tone="gray"><Glyph name="user" size={17} /></IconTile>
              <div className="min-0">
                <strong>{c.name || '이름 없는 참여자'}{c.isMe && ' (나)'}</strong>
                <span className="ui-row-meta">{roleText(c)}{c.society && ` · ${societyName(c.society)}`}{c.ipi && <> · <span className="aq-nowrap">IPI {c.ipi}</span></>}</span>
                {/* 맡기기 전(0%)에는 줄을 두지 않는다 — 'AUDENIQ 관리 0%'가 참여자마다 되풀이됐다 */}
                {c.publisherShare > 0 && <span className="ui-row-meta">AUDENIQ 관리 {pct(c.publisherShare)}</span>}
              </div>
              <div className="aq-pbl-person-end">
                <b>{pct(c.share)}</b>
                <Badge tone={c.confirm === 'confirmed' ? 'green' : c.confirm === 'disputed' ? 'red' : 'amber'}>{c.confirm === 'confirmed' ? '확인 완료' : c.confirm === 'disputed' ? '이의 제기' : '확인 대기'}</Badge>
              </div>
              {(onRequestConfirm || onSimulate) && !c.isMe && c.confirm !== 'confirmed' && <div className="aq-pbl-person-actions">
                {onRequestConfirm && <button type="button" className="ui-btn is-text is-sm" onClick={() => onRequestConfirm(c)}>확인 요청 다시 보내기</button>}
                {onSimulate && <button type="button" className="ui-btn is-text is-sm" onClick={() => onSimulate(c)}>체험: 상대가 확인함</button>}
              </div>}
            </div>
          ) : (
            <fieldset key={c.id} className={`ui-card aq-pbl-person${focusId === c.id ? ' is-focus' : ''}`} id={`${prefix}`}>
              <legend className="sr-only">참여자 {i + 1}: {c.name || '이름 없음'}</legend>
              <div className="aq-pbl-person-head">
                <IconTile size="sm" tone="gray"><Glyph name="user" size={17} /></IconTile>
                <div className="min-0"><strong>{c.name || `참여자 ${i + 1}`}{c.isMe && ' (나)'}</strong><small>{c.external ? '외부 참여자' : 'AUDENIQ 작가'}</small></div>
                <button type="button" className="ui-btn is-text is-sm aq-danger-text" aria-label={`${c.name || '참여자'} 삭제`} onClick={() => onChange(value.filter(x => x.id !== c.id))}>삭제</button>
              </div>
              {c.external && <Field label="이름·활동명" htmlFor={`${prefix}-name`}>
                <input id={`${prefix}-name`} className="ui-input" maxLength={100} value={c.name} onChange={e => set(c.id, { name: e.target.value })} />
              </Field>}
              {(() => {
                // 크레딧으로 들어온 참여자 — 지난 곡에서 이 사람의 관리 단체·IPI를 적은 적이 있으면 한 번에 채운다
                const k = c.external && (!c.society || !c.ipi) ? known(c.name) : null;
                if (!k || (!k.society && !k.ipi) || (k.society === c.society && k.ipi === c.ipi)) return null;
                return (
                  <button type="button" className="aq-pbl-fill" onClick={() => set(c.id, { society: c.society || k.society, ipi: c.ipi || k.ipi })}>
                    <Glyph name="repeat" size={14} />지난 곡 정보 채우기<span>{[societyName(k.society), k.ipi && `IPI ${k.ipi}`].filter(Boolean).join(' · ')}</span>
                  </button>
                );
              })()}
              <div className="aq-pbl-person-fields">
                <Field label="참여 역할">
                  <div className="ui-chips is-wrap" role="group" aria-label={`${c.name} 역할`}>
                    {ROLES.map(r => <button key={r} type="button" className="ui-chip is-sm" aria-pressed={c.roles.includes(r)} onClick={() => set(c.id, { roles: c.roles.includes(r) ? c.roles.filter(x => x !== r) : [...c.roles, r] })}>{ROLE_LABEL[r]}</button>)}
                  </div>
                </Field>
                <Field label="창작 지분" htmlFor={`${prefix}-share`}>
                  <span className="aq-affix" data-unit="%"><input id={`${prefix}-share`} className="ui-input" inputMode="decimal" value={c.share || ''} placeholder="0" onChange={e => { const v = num(e.target.value); set(c.id, { share: v, publisherShare: Math.min(c.publisherShare, v) }); }} /></span>
                </Field>
              </div>
              <details className="aq-pbl-person-more" open={(invalid && c.publisherShare > c.share) || focusId === c.id ? true : undefined}>
                <summary>관리 범위·작가 정보 <Glyph name="chevron-down" size={15} /></summary>
                <p className="ui-field-help">관리 지분은 이 곡 전체에서 AUDENIQ에 맡기는 몫이에요. 창작 지분을 넘을 수 없으며, 수수료와는 별개예요.</p>
                <div className="form-grid">
                  <Field label="AUDENIQ 관리 지분" htmlFor={`${prefix}-managed`}>
                    <span className="aq-affix" data-unit="%"><input id={`${prefix}-managed`} className="ui-input" inputMode="decimal" value={c.publisherShare || ''} placeholder="0" onChange={e => set(c.id, { publisherShare: num(e.target.value) })} /></span>
                  </Field>
                  <Field label="저작권 관리 단체" htmlFor={`${prefix}-society`}>
                    <select id={`${prefix}-society`} className="ui-select" value={c.society ?? ''} onChange={e => set(c.id, { society: e.target.value || null })}><option value="">모름·입력 전</option>{SOCIETIES.map(s => <option key={s} value={s}>{societyName(s)}</option>)}</select>
                  </Field>
                  <Field label="IPI/CAE 번호" htmlFor={`${prefix}-ipi`} help={c.isMe ? '한 번 적으면 계정 정보에 저장돼 다음 곡부터 자동으로 채워져요.' : '모르면 비워 두세요.'}>
                    <input id={`${prefix}-ipi`} className="ui-input" inputMode="numeric" placeholder="작가 식별 번호" value={c.ipi ?? ''} readOnly={!c.external && !c.isMe} onChange={e => set(c.id, { ipi: e.target.value.replace(/\D/g, '').slice(0, 11) || null })} />
                  </Field>
                </div>
              </details>
              {c.isMe && !c.society && <SocietyJoin inline />}
              {invalid && errors.map(err => <small key={err.text} className="ui-field-help is-error" role="alert">{err.text}</small>)}
            </fieldset>
          );
        })}
      </div>
      {!readOnly && reuse && <ReuseFrom works={reuse} value={value} onChange={onChange} />}
      {!readOnly && <Field label="참여자 추가"><CreatorSearch taken={new Set(value.map(c => c.creatorId ?? '').filter(Boolean))} onPick={fromCreator} onExternal={external} /></Field>}
      {invalid && issues.filter(i => !i.contributorId && i.level === 'error').map(i => <small key={i.text} className="ui-field-help is-error" role="alert">{i.text}</small>)}
    </div>
  );
}
