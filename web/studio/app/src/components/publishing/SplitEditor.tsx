// 참여자·지분 입력 — 발매 상세의 '공동작업자 수익 분배'(SplitsCard)와 같은 줄 모양(aq-splits-*, % 붙은 입력칸).
// 작가 검색으로 추가하고, 없으면 이름만으로 외부 참여자를 추가한다. 합계는 바로 계산하고 틀린 줄을 짚는다.
// 틀려도 임시 저장은 막지 않는다 — 신청할 때만 막는다 (기획안 5.2 · 5.3)
import { useDeferredValue, useEffect, useState } from 'react';
import { pubApi, type ContributorRole, type Creator, type WorkContributor } from '../../api/publishing';
import { uid } from '../../lib/store';
import { ROLE_LABEL, SOCIETIES, evenSplit, pct, splitIssues, sumShares } from '../../lib/publishing';
import { Badge, IconTile } from '../../ui';
import { Glyph } from '../Glyph';
import { SplitBar, roleText } from './parts';
import '../../styles/split/Publishing.css';

const ROLES = Object.keys(ROLE_LABEL) as ContributorRole[];

function CreatorSearch({ onPick, onExternal, taken }: { onPick: (c: Creator) => void; onExternal: (name: string) => void; taken: Set<string> }) {
  const [q, setQ] = useState('');
  const dq = useDeferredValue(q);
  const [items, setItems] = useState<Creator[] | null>(null);
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
        onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); const first = items?.find(c => !taken.has(c.id)); if (first) add(first); else if (q.trim()) { onExternal(q.trim()); setQ(''); } } }} />
      {q.trim() && items && (
        <ul className="ui-list aq-pbl-search-list" aria-label="찾은 작가">
          {items.map(c => (
            <li key={c.id}>
              <button type="button" className="ui-row" disabled={taken.has(c.id)} onClick={() => add(c)}>
                <IconTile size="sm"><Glyph name="user" size={16} /></IconTile>
                <span className="min-0">
                  <span className="ui-row-title">{c.name}{c.isMe && ' (나)'}</span>
                  <span className="ui-row-meta">{[c.society, c.ipi && `IPI ${c.ipi}`, c.workCount != null && `작품 ${c.workCount}개`].filter(Boolean).map(t => <span key={String(t)}>{t}</span>)}</span>
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

export function SplitEditor({ value, onChange, readOnly, onRequestConfirm, onSimulate }: {
  value: WorkContributor[];
  onChange: (next: WorkContributor[]) => void;
  readOnly?: boolean;
  /** 저장된 작품에서만 — 참여자에게 확인 요청 */
  onRequestConfirm?: (c: WorkContributor) => void;
  /** 체험 모드에서 상대 확인 흉내 */
  onSimulate?: (c: WorkContributor) => void;
}) {
  const total = sumShares(value);
  const pubTotal = sumShares(value, 'publisherShare');
  const issues = splitIssues(value);
  const rowIssue = (id: string) => issues.find(i => i.contributorId === id);
  const set = (id: string, patch: Partial<WorkContributor>) => onChange(value.map(c => (c.id === id ? { ...c, ...patch } : c)));
  const rest = Math.max(0, Math.round((100 - total) * 100) / 100);
  const fromCreator = (cr: Creator) => onChange([...value, {
    id: uid(), creatorId: cr.id, name: cr.name, society: cr.society, ipi: cr.ipi, external: false, isMe: cr.isMe,
    roles: (cr.roles as string[]).filter((r): r is ContributorRole => (ROLES as string[]).includes(r)).slice(0, 2),
    share: rest, publisherShare: 0, confirm: cr.isMe ? 'confirmed' : 'pending',
  }]);
  const external = (name: string) => onChange([...value, { id: uid(), creatorId: null, name, roles: [], society: null, ipi: null, external: true, share: rest, publisherShare: 0, confirm: 'pending' }]);
  const even = () => { const s = evenSplit(value.length); onChange(value.map((c, i) => ({ ...c, share: s[i], publisherShare: Math.min(c.publisherShare, s[i]) }))); };
  const num = (v: string) => (v === '' ? 0 : Number(v.replace(/[^0-9.]/g, '')) || 0);

  return (
    <div className="aq-splits aq-pbl-split">
      <SplitBar list={value} />
      {value.map(c => {
        const err = rowIssue(c.id);
        return (
          <fieldset key={c.id} className="aq-splits-track">
            <legend>
              {c.name || '이름 없는 참여자'}{c.isMe && ' (나)'}
              {c.external ? <Badge tone="gray">외부 참여자</Badge> : !c.isMe && <Badge tone="blue">AUDENIQ 작가</Badge>}
              {c.confirm === 'confirmed' ? <Badge tone="green">확인 완료</Badge> : c.confirm === 'disputed' ? <Badge tone="red">이의 제기</Badge> : !c.isMe && <Badge tone="amber">확인 대기</Badge>}
            </legend>
            {!readOnly && <div className="ui-chips" role="group" aria-label={`${c.name} 역할`}>
              {ROLES.map(r => (
                <button key={r} type="button" className="ui-chip is-sm" aria-pressed={c.roles.includes(r)} disabled={readOnly}
                  onClick={() => set(c.id, { roles: c.roles.includes(r) ? c.roles.filter(x => x !== r) : [...c.roles, r] })}>
                  {ROLE_LABEL[r]}
                </button>
              ))}
            </div>}
            {readOnly ? (
              <div className="aq-splits-row">
                <span className="aq-splits-name">{roleText(c)}<span className="muted"> · {[c.society, c.ipi && `IPI ${c.ipi}`].filter(Boolean).join(' · ') || '관리 단체·IPI 없음'}</span></span>
                <span className="aq-splits-share">{pct(c.share)}</span>
                <span className="muted small">관리 {pct(c.publisherShare)}</span>
                <span className="aq-splits-money small muted">
                  {onRequestConfirm && !c.isMe && c.confirm !== 'confirmed' && <button type="button" className="ui-btn is-text is-sm" onClick={() => onRequestConfirm(c)}>확인 요청 다시 보내기</button>}
                  {onSimulate && !c.isMe && c.confirm !== 'confirmed' && <button type="button" className="ui-btn is-text is-sm" onClick={() => onSimulate(c)}>체험: 상대가 확인함</button>}
                </span>
              </div>
            ) : (
              <div className="aq-splits-row aq-pbl-split-row">
                {c.external
                  ? <input className="ui-input" aria-label="이름" maxLength={100} placeholder="이름·활동명" value={c.name} onChange={e => set(c.id, { name: e.target.value })} />
                  : <span className="aq-splits-name">{c.name}</span>}
                <span className="aq-splits-pct aq-affix" data-unit="%">
                  <input className="ui-input" inputMode="decimal" aria-label="창작 지분(%)" value={c.share || ''} placeholder="창작"
                    onChange={e => { const v = num(e.target.value); set(c.id, { share: v, publisherShare: Math.min(c.publisherShare, v) }); }} />
                </span>
                <span className="aq-splits-pct aq-affix" data-unit="%">
                  <input className="ui-input" inputMode="decimal" aria-label="퍼블리셔 관리 지분(%)" value={c.publisherShare || ''} placeholder="관리"
                    onChange={e => set(c.id, { publisherShare: num(e.target.value) })} />
                </span>
                <select className="ui-select" aria-label="관리 단체" value={c.society ?? ''} onChange={e => set(c.id, { society: e.target.value || null })}>
                  <option value="">관리 단체 모름</option>
                  {SOCIETIES.map(s => <option key={s} value={s}>{s}</option>)}
                </select>
                <input className="ui-input" inputMode="numeric" aria-label="IPI/CAE" placeholder="IPI/CAE" value={c.ipi ?? ''} readOnly={!c.external && !c.isMe}
                  onChange={e => set(c.id, { ipi: e.target.value.replace(/\D/g, '').slice(0, 11) || null })} />
                <button type="button" className="ui-btn is-text is-sm aq-danger-text" onClick={() => onChange(value.filter(x => x.id !== c.id))}>삭제</button>
              </div>
            )}
            {err && <small className={`ui-field-help${err.level === 'error' ? ' is-error' : ' is-warn'}`}>{err.text}</small>}
          </fieldset>
        );
      })}

      {!readOnly && <CreatorSearch taken={new Set(value.map(c => c.creatorId ?? '').filter(Boolean))} onPick={fromCreator} onExternal={external} />}

      <div className="aq-splits-foot">
        <span className={total > 100 ? 'aq-splits-over' : 'muted small'} aria-live="polite">
          창작 지분 합계 {pct(total)}{total === 100 ? ' · 맞아요' : total > 100 ? ' · 100%를 넘어요' : ` · ${pct(rest)} 남았어요`} · 퍼블리셔 관리 {pct(pubTotal)}
        </span>
        {!readOnly && value.length > 1 && <button type="button" className="ui-btn is-soft is-sm" onClick={even}>균등하게 나누기</button>}
      </div>
      {issues.filter(i => !i.contributorId).map(i => (
        <small key={i.text} className={`ui-field-help${i.level === 'error' ? ' is-error' : ' is-warn'}`}>{i.text}</small>
      ))}
      <p className="ui-field-help">창작 지분은 작가끼리 나눈 몫, 퍼블리셔 관리 지분은 그중 AUDENIQ이 관리를 맡는 몫이에요.</p>
    </div>
  );
}
