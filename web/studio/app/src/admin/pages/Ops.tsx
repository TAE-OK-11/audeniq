// 운영 도구 — 예전에 audeniq-admin CLI로만 하던 관리 작업 (admin/ops.ts, crates/core/src/staff_ops.rs).
// 화면마다 내 레벨로 할 수 있는 것만 보여 주고, 서버도 레벨을 다시 확인한다. 위험한 작업은 인증 앱 코드를 한 번 더 받는다.
import { DateField } from '../../components/Picker';
import { useEffect, useRef, useState, type ChangeEvent, type FormEvent, type ReactNode } from 'react';
import { useAsync } from '../../hooks/useAsync';
import { ApiError, errorMessage } from '../../api/errors';
import { useToast } from '../../components/Toast';
import { useConfirm } from '../../components/Confirm';
import { Modal } from '../../components/Modal';
import { Money } from '../../components/Money';
import { Glyph } from '../../components/Glyph';
import { Badge, Callout, Field, Tabs, inputClass, selectClass } from '../../ui';
import { EmptyState, ErrorBox, PageHeader, Section, SkeletonList, useStaff } from '../ui';
import { ROLE_LABEL, ago, when } from '../labels';
import { ROLE_LEVEL, levelOf, type StaffRole } from '../api';
import {
  can, entry, opsApi,
  type AgentRow, type DevAppRow, type IssuerRow, type LimitRow, type OpsArea, type OpsIndex, type PartnerRow,
  type PartnerStatus, type PayoutRow, type ProtectedRow, type RoyaltyReport, type StaffRow, type BalanceAccount,
} from '../ops';

export const LEVEL_NAME: Record<number, string> = { 1: '고객 지원', 2: '실무 담당', 3: '관리자', 4: '최고 관리자' };

// ---------------------------------------------------------------------------
// 공통: 운영 도구 상태 · 실행(확인 → 인증 코드 → 결과 알림)
// ---------------------------------------------------------------------------

function useOpsIndex() {
  return useAsync(() => opsApi.index(), []);
}

/** 운영 도구 오류 문구 — CLI가 돌려준 설명은 그대로 덧붙인다 */
export function opsError(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.code === 'OPS_REJECTED' && e.server?.message) return `처리하지 못했어요. ${e.server.message}`;
    if (e.code === 'STAFF_LEVEL_REQUIRED') {
      const d = e.server?.details ?? {};
      if (d.duty === 'FINANCE') return '정산 업무(배급 운영·관리자·최고 관리자)만 할 수 있어요.';
      if (typeof d.level === 'number') return `${LEVEL_NAME[d.level]}(레벨 ${d.level}) 이상만 할 수 있어요.`;
    }
    if (e.code === 'OPS_ARGUMENT_INVALID' && typeof e.server?.details?.field === 'string') return `입력값을 확인해 주세요. (${e.server.details.field})`;
  }
  return errorMessage(e);
}

interface RunOptions { confirm?: { title: string; message?: ReactNode; confirmLabel?: string; danger?: boolean }; done?: string }

/** 작업 실행 — 확인 창, 필요하면 인증 코드, 성공·실패 알림까지. 실패하면 undefined. */
function useRun(index: OpsIndex | undefined) {
  const toast = useToast();
  const confirm = useConfirm();
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState<{ resolve: (code: string | null) => void; error: string } | null>(null);
  const askCode = (error = '') => new Promise<string | null>(resolve => setAsk({ resolve, error }));

  async function run<T = unknown>(area: OpsArea, op: string, body: Record<string, unknown>, o: RunOptions = {}): Promise<T | undefined> {
    if (o.confirm && !(await confirm(o.confirm))) return undefined;
    const stepUp = entry(index, area, op)?.step_up;
    let code: string | null = null;
    if (stepUp) { code = await askCode(); if (code === null) return undefined; }
    setBusy(true);
    try {
      for (;;) {
        try {
          const out = await opsApi.write<T>(area, op, code ? { ...body, code } : body);
          if (o.done) toast(o.done, 'success');
          return out;
        } catch (e) {
          // 코드가 틀렸거나 서버가 코드를 요구하면 다시 묻는다 (취소하면 끝)
          if (e instanceof ApiError && (e.code === 'STAFF_MFA_CODE_INVALID' || e.code === 'STAFF_STEP_UP_REQUIRED')) {
            code = await askCode(e.code === 'STAFF_MFA_CODE_INVALID' ? '코드가 맞지 않아요. 새 코드를 입력해 주세요.' : '');
            if (code === null) return undefined;
            continue;
          }
          throw e;
        }
      }
    } catch (e) {
      toast(opsError(e), 'error');
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  const modal = ask && (
    <StepUp error={ask.error} onDone={code => { ask.resolve(code); setAsk(null); }} />
  );
  return { run, busy, modal };
}

function StepUp({ error, onDone }: { error: string; onDone: (code: string | null) => void }) {
  const [code, setCode] = useState('');
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => { ref.current?.focus(); }, []);
  const ok = /^\d{6}$/.test(code);
  return (
    <Modal title="인증 코드 확인" onClose={() => onDone(null)} dismissible={false}>
      <form onSubmit={e => { e.preventDefault(); if (ok) onDone(code); }} className="adm-ops-stepup">
        <p>중요한 작업이라 인증 앱의 6자리 코드를 한 번 더 확인해요.</p>
        <Field label="인증 코드" htmlFor="ops-code" error={error || undefined}>
          <input id="ops-code" ref={ref} className={inputClass('lg', 'is-mono')} inputMode="numeric" autoComplete="one-time-code"
            maxLength={6} value={code} onChange={e => setCode(e.target.value.replace(/\D/g, ''))} placeholder="000000" />
        </Field>
        <div className="adm-ops-actions">
          <button type="button" className="ui-btn is-ghost" onClick={() => onDone(null)}>닫기</button>
          <button type="submit" className="ui-btn is-primary" disabled={!ok}>확인</button>
        </div>
      </form>
    </Modal>
  );
}

/** 운영 도구 화면 틀 — 서버에서 꺼져 있으면 CLI 안내, 내 레벨 표시 */
function OpsPage({ eyebrow, title, sub, children, index, loading, error, reload }: {
  eyebrow: string; title: string; sub: string; children: ReactNode;
  index: OpsIndex | undefined; loading: boolean; error: string; reload: () => void;
}) {
  const { me } = useStaff();
  const level = levelOf(me);
  return (
    <div className="view-enter adm-ops">
      <PageHeader eyebrow={eyebrow} title={title} sub={sub}
        actions={<Badge tone="violet" title="관리자 레벨">{ROLE_LABEL[me.role]} · 레벨 {level}</Badge>} />
      {error && <ErrorBox message={error} onRetry={reload} />}
      {loading && !index ? <SkeletonList rows={4} /> : index && !index.available ? (
        <Callout tone="warn">
          이 서버에서는 운영 도구가 아직 꺼져 있어요. 운영 담당자가 <code>AUDENIQ_STAFF_OPS_DATABASE_URL</code>을 설정하면 여기서 처리할 수 있어요.
          그전에는 <code>audeniq-admin</code> CLI로 처리해 주세요.
        </Callout>
      ) : index && children}
    </div>
  );
}

function Need({ level, finance }: { level?: number; finance?: boolean }) {
  return (
    <Callout tone="neutral">
      {finance ? '정산 업무(배급 운영·관리자·최고 관리자)만 볼 수 있어요.' : `${LEVEL_NAME[level ?? 3]}(레벨 ${level}) 이상만 볼 수 있어요.`}
    </Callout>
  );
}

function Empty({ title, sub }: { title: string; sub?: string }) {
  return <EmptyState icon={<Glyph name="layers" size={22} />} title={title} compact>{sub}</EmptyState>;
}

const trim = (v: string) => v.trim();

// ---------------------------------------------------------------------------
// 직원·권한 (최고 관리자)
// ---------------------------------------------------------------------------

const GRANTABLE: StaffRole[] = ['ADMIN', 'OPERATOR', 'REVIEWER', 'SUPPORT'];

export function OpsStaff() {
  const { me } = useStaff();
  const ix = useOpsIndex();
  const index = ix.data;
  const list = useAsync(() => (can(index, 'staff', 'list') ? opsApi.read<StaffRow[]>('staff', 'list') : Promise.resolve(null)), [index?.available, index?.level]);
  const { run, busy, modal } = useRun(index);
  const owner = can(index, 'staff', 'grant');
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<StaffRole>('SUPPORT');

  async function grant(target: string, to: StaffRole, from?: string) {
    const out = await run('staff', 'grant', { email: target, role: to }, {
      confirm: {
        title: from ? '레벨을 바꿀까요?' : '직원으로 등록할까요?',
        message: `${target} 계정을 ${ROLE_LABEL[to]}(레벨 ${ROLE_LEVEL[to]})${from ? `으로 바꿔요. 지금은 ${ROLE_LABEL[from as StaffRole] ?? from}이에요.` : '로 등록해요.'} 처리 기록은 내 계정으로 남아요.`,
        confirmLabel: from ? '레벨 바꾸기' : '등록',
      },
      done: from ? '레벨을 바꿨어요.' : '직원으로 등록했어요.',
    });
    if (out) { setEmail(''); list.reload(); }
  }

  const rows = list.data ?? [];
  return (
    <OpsPage eyebrow="운영 도구" title="직원·권한" sub="관리자 콘솔을 쓰는 직원과 레벨이에요. 레벨이 높으면 아래 레벨의 일을 모두 할 수 있어요."
      index={index} loading={ix.loading} error={ix.error} reload={ix.reload}>
      <Section title="레벨">
        <div className="adm-ops-levels">
          {[1, 2, 3, 4].map(n => (
            <div key={n} className={`ui-card is-tight${levelOf(me) === n ? ' is-current' : ''}`}>
              <b>레벨 {n} · {LEVEL_NAME[n]}</b>
              <small>{[
                '문의 답변, 실명인증',
                '심사 담당: 발매 심사·서류 / 배급 운영: 배급 승인·지급 처리, AI 사용 한도, 보호 아티스트·파트너 보기',
                '보호 아티스트, AI 에이전트, 개발자 앱 검토, 파트너 설정, 공지·이벤트',
                '직원 레벨 관리, 식별코드 범위 등록, 파트너 라이브 전환',
              ][n - 1]}</small>
            </div>
          ))}
        </div>
      </Section>
      {!can(index, 'staff', 'list') ? <Need level={3} /> : (
        <>
          {owner && (
            <Section title="직원 등록·레벨 바꾸기">
              <form className="ui-card adm-ops-form" onSubmit={(e: FormEvent) => { e.preventDefault(); if (email.trim()) void grant(email.trim(), role, rows.find(r => r.email === email.trim() && r.status === 'ACTIVE')?.role); }}>
                <Field label="AUDENIQ 계정 이메일" htmlFor="ops-staff-email" help="먼저 AUDENIQ에 가입한 계정이어야 해요.">
                  <input id="ops-staff-email" className={inputClass()} type="email" value={email} onChange={e => setEmail(e.target.value)} placeholder="name@audeniq.com" />
                </Field>
                <Field label="레벨" htmlFor="ops-staff-role">
                  <select id="ops-staff-role" className={selectClass()} value={role} onChange={e => setRole(e.target.value as StaffRole)}>
                    {GRANTABLE.map(r => <option key={r} value={r}>{ROLE_LABEL[r]} (레벨 {ROLE_LEVEL[r]})</option>)}
                  </select>
                </Field>
                <button type="submit" className="ui-btn is-primary" disabled={busy || !email.trim()}>등록</button>
              </form>
              <p className="adm-ops-note">최고 관리자는 CLI(<code>audeniq-admin staff grant EMAIL OWNER</code>)에서만 정해요. 내 권한은 다른 최고 관리자가 바꿔요.</p>
            </Section>
          )}
          <Section title="직원" meta={`${rows.filter(r => r.status === 'ACTIVE').length}명`}>
            {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
            {list.loading && !list.data ? <SkeletonList rows={3} /> : rows.length === 0 ? <Empty title="등록된 직원이 없어요" /> : (
              <div className="ui-card adm-table-wrap">
                <table className="adm-table">
                  <thead><tr><th>계정</th><th>레벨</th><th>상태</th><th>등록</th>{owner && <th aria-label="관리" />}</tr></thead>
                  <tbody>
                    {rows.map(r => {
                      const self = r.user_id === me.user_id;
                      const locked = self || r.role === 'OWNER' || r.status !== 'ACTIVE';
                      return (
                        <tr key={r.user_id}>
                          <td><b>{r.email}</b>{self && <Badge tone="blue">나</Badge>}</td>
                          <td>
                            {owner && !locked ? (
                              <select aria-label={`${r.email} 레벨`} className={selectClass('sm')} value={r.role} disabled={busy}
                                onChange={e => void grant(r.email, e.target.value as StaffRole, r.role)}>
                                {GRANTABLE.map(x => <option key={x} value={x}>{ROLE_LABEL[x]} · {ROLE_LEVEL[x]}</option>)}
                              </select>
                            ) : <>{ROLE_LABEL[r.role as StaffRole] ?? r.role} <small className="adm-ops-dim">레벨 {ROLE_LEVEL[r.role as StaffRole] ?? '-'}</small></>}
                          </td>
                          <td>{r.status === 'ACTIVE' ? <Badge tone="green">사용 중</Badge> : <Badge>해제됨</Badge>}</td>
                          <td className="small" title={when(r.granted_at)}>{ago(r.granted_at)}</td>
                          {owner && (
                            <td className="adm-ops-row-actions">
                              {!locked && <>
                                <button type="button" className="ui-btn is-text is-sm" disabled={busy} onClick={async () => {
                                  if (await run('staff', 'reset-mfa', { user_id: r.user_id }, { confirm: { title: '인증 앱을 초기화할까요?', message: `${r.email}의 인증 앱 등록과 로그인이 모두 끝나요. 다음 로그인 때 인증 앱을 새로 등록해요.`, confirmLabel: '초기화', danger: true }, done: '인증 앱을 초기화했어요.' })) list.reload();
                                }}>인증 앱 초기화</button>
                                <button type="button" className="ui-btn is-text is-sm is-danger" disabled={busy} onClick={async () => {
                                  if (await run('staff', 'revoke', { user_id: r.user_id }, { confirm: { title: '관리자 권한을 해제할까요?', message: `${r.email}은 더 이상 관리자 콘솔을 쓸 수 없어요.`, confirmLabel: '해제', danger: true }, done: '권한을 해제했어요.' })) list.reload();
                                }}>해제</button>
                              </>}
                            </td>
                          )}
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </Section>
        </>
      )}
      {modal}
    </OpsPage>
  );
}

// ---------------------------------------------------------------------------
// 보호 아티스트 (보기 레벨 2, 바꾸기 레벨 3)
// ---------------------------------------------------------------------------

const MODE_LABEL: Record<string, string> = { CONTAINS: '이름에 포함', TOKEN: '단어로 일치' };
const ACTION_LABEL: Record<string, [string, 'red' | 'amber']> = { BLOCK: ['차단', 'red'], REVIEW: ['심사로', 'amber'] };

export function OpsProtected() {
  const ix = useOpsIndex();
  const index = ix.data;
  const list = useAsync(() => (can(index, 'protected', 'list') ? opsApi.read<ProtectedRow[]>('protected', 'list') : Promise.resolve(null)), [index?.available, index?.level]);
  const { run, busy, modal } = useRun(index);
  const edit = can(index, 'protected', 'add');
  const [f, setF] = useState({ name: '', mode: 'CONTAINS', action: 'BLOCK', note: '' });
  const [q, setQ] = useState('');
  const rows = (list.data ?? []).filter(r => !q.trim() || r.name.toLowerCase().includes(q.trim().toLowerCase()) || r.aliases.some(a => a.alias.toLowerCase().includes(q.trim().toLowerCase())));
  const act = async (op: string, body: Record<string, unknown>, done: string, confirm?: RunOptions['confirm']) => {
    if (await run('protected', op, body, { done, confirm })) list.reload();
  };

  return (
    <OpsPage eyebrow="운영 도구" title="보호 아티스트" sub="유명 아티스트 이름을 사칭한 발매를 1차 검사에서 막거나 심사로 보내요. 본인·소속사는 예외로 등록해요."
      index={index} loading={ix.loading} error={ix.error} reload={ix.reload}>
      {!can(index, 'protected', 'list') ? <Need level={2} /> : (
        <>
          {edit && (
            <Section title="이름 추가">
              <form className="ui-card adm-ops-form" onSubmit={async e => {
                e.preventDefault();
                if (!f.name.trim()) return;
                if (await run('protected', 'add', { name: trim(f.name), mode: f.mode, action: f.action, note: trim(f.note) || undefined }, { done: '보호 이름을 추가했어요.' })) { setF({ ...f, name: '', note: '' }); list.reload(); }
              }}>
                <Field label="아티스트 이름" htmlFor="ops-p-name"><input id="ops-p-name" className={inputClass()} value={f.name} onChange={e => setF({ ...f, name: e.target.value })} /></Field>
                <Field label="찾는 방법" htmlFor="ops-p-mode">
                  <select id="ops-p-mode" className={selectClass()} value={f.mode} onChange={e => setF({ ...f, mode: e.target.value })}>
                    <option value="CONTAINS">이름에 포함되면</option><option value="TOKEN">단어로 일치하면</option>
                  </select>
                </Field>
                <Field label="찾으면" htmlFor="ops-p-action">
                  <select id="ops-p-action" className={selectClass()} value={f.action} onChange={e => setF({ ...f, action: e.target.value })}>
                    <option value="BLOCK">발매 차단</option><option value="REVIEW">심사로 보내기</option>
                  </select>
                </Field>
                <Field label="메모" htmlFor="ops-p-note"><input id="ops-p-note" className={inputClass()} value={f.note} onChange={e => setF({ ...f, note: e.target.value })} placeholder="요청한 곳, 근거" /></Field>
                <button type="submit" className="ui-btn is-primary" disabled={busy || !f.name.trim()}>추가</button>
              </form>
            </Section>
          )}
          <Section title="보호 이름" meta={`${(list.data ?? []).filter(r => r.active).length}개 사용 중`}
            action={<input aria-label="이름 찾기" className={inputClass('sm')} value={q} onChange={e => setQ(e.target.value)} placeholder="이름 찾기" />}>
            {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
            {list.loading && !list.data ? <SkeletonList rows={3} /> : rows.length === 0 ? <Empty title={q ? '찾는 이름이 없어요' : '보호 이름이 없어요'} /> : (
              <div className="adm-ops-list">
                {rows.map(r => <ProtectedCard key={r.id} r={r} edit={edit} busy={busy} act={act} />)}
              </div>
            )}
          </Section>
        </>
      )}
      {modal}
    </OpsPage>
  );
}

function ProtectedCard({ r, edit, busy, act }: { r: ProtectedRow; edit: boolean; busy: boolean; act: (op: string, body: Record<string, unknown>, done: string, confirm?: RunOptions['confirm']) => Promise<void> }) {
  const [alias, setAlias] = useState('');
  const [phrase, setPhrase] = useState(false);
  const [org, setOrg] = useState('');
  const [reason, setReason] = useState('');
  const [open, setOpen] = useState(false);
  const live = r.exceptions.filter(x => !x.revoked);
  return (
    <article className={`ui-card adm-ops-item${r.active ? '' : ' is-off'}`}>
      <header>
        <b>{r.name}</b>
        <Badge tone={ACTION_LABEL[r.action]?.[1] ?? 'gray'}>{ACTION_LABEL[r.action]?.[0] ?? r.action}</Badge>
        <Badge>{MODE_LABEL[r.match_mode] ?? r.match_mode}</Badge>
        {!r.active && <Badge>꺼짐</Badge>}
        {edit && (
          <span className="adm-ops-item-actions">
            <button type="button" className="ui-btn is-text is-sm" onClick={() => setOpen(!open)} aria-expanded={open}>{open ? '닫기' : '별칭·예외 관리'}</button>
            <button type="button" className="ui-btn is-text is-sm" disabled={busy} onClick={() => act(r.active ? 'deactivate' : 'activate', { name: r.name }, r.active ? '보호를 껐어요. 기록은 남아요.' : '보호를 다시 켰어요.')}>{r.active ? '끄기' : '켜기'}</button>
          </span>
        )}
      </header>
      {r.note && <p className="adm-ops-dim">{r.note}</p>}
      <div className="adm-ops-chips">
        {r.aliases.map(a => (
          <span key={a.alias} className="ui-tag">
            {a.kind === 'PHRASE' ? '문구 · ' : ''}{a.alias}
            {edit && open && <button type="button" className="adm-ops-x" aria-label={`${a.alias} 별칭 빼기`} disabled={busy} onClick={() => act('remove-alias', { name: r.name, alias: a.alias }, '별칭을 뺐어요.')}>×</button>}
          </span>
        ))}
        {live.map(x => (
          <span key={x.org_id} className="ui-tag is-green" title={x.reason}>
            예외 · {x.org_id.slice(0, 8)}
            {edit && open && <button type="button" className="adm-ops-x" aria-label="예외 취소" disabled={busy} onClick={() => act('revoke-exception', { name: r.name, org_id: x.org_id }, '예외를 취소했어요.', { title: '예외를 취소할까요?', message: '이 작업 공간도 다시 검사 대상이 돼요.', confirmLabel: '취소하기', danger: true })}>×</button>}
          </span>
        ))}
      </div>
      {edit && open && (
        <div className="adm-ops-sub">
          <form className="adm-ops-inline" onSubmit={async e => { e.preventDefault(); if (!alias.trim()) return; await act('alias', { name: r.name, alias: trim(alias), phrase }, '별칭을 추가했어요.'); setAlias(''); }}>
            <input aria-label="별칭" className={inputClass('sm')} value={alias} onChange={e => setAlias(e.target.value)} placeholder="별칭·영문명·대표곡 문구" />
            <label className="adm-ops-check"><input type="checkbox" checked={phrase} onChange={e => setPhrase(e.target.checked)} />문구</label>
            <button type="submit" className="ui-btn is-soft is-sm" disabled={busy || !alias.trim()}>별칭 추가</button>
          </form>
          <form className="adm-ops-inline" onSubmit={async e => { e.preventDefault(); if (!org.trim() || !reason.trim()) return; await act('grant-exception', { name: r.name, org_id: trim(org), reason: trim(reason) }, '예외를 등록했어요.'); setOrg(''); setReason(''); }}>
            <input aria-label="작업 공간 ID" className={inputClass('sm', 'is-mono')} value={org} onChange={e => setOrg(e.target.value)} placeholder="작업 공간 ID (본인·소속사)" />
            <input aria-label="예외 사유" className={inputClass('sm')} value={reason} onChange={e => setReason(e.target.value)} placeholder="사유" />
            <button type="submit" className="ui-btn is-soft is-sm" disabled={busy || !org.trim() || !reason.trim()}>예외 등록</button>
          </form>
        </div>
      )}
    </article>
  );
}

// ---------------------------------------------------------------------------
// AI·개발자 앱
// ---------------------------------------------------------------------------

const VIS: Record<string, [string, 'gray' | 'amber' | 'green']> = { PRIVATE: ['나만 연결', 'gray'], REVIEW: ['검토 중', 'amber'], PUBLIC: ['공개', 'green'] };

export function OpsAi() {
  const ix = useOpsIndex();
  const index = ix.data;
  const [tab, setTab] = useState<'apps' | 'agents' | 'limits'>('apps');
  return (
    <OpsPage eyebrow="운영 도구" title="AI·개발자 앱" sub="개발자 앱 공개 검토, 운영자가 등록하는 AI 에이전트, 아티스트별 AI 사용 한도를 관리해요."
      index={index} loading={ix.loading} error={ix.error} reload={ix.reload}>
      <Tabs label="AI·개발자 앱" value={tab} onChange={setTab} items={[
        { value: 'apps', label: '개발자 앱 검토' }, { value: 'agents', label: 'AI 에이전트' }, { value: 'limits', label: 'AI 사용 한도' },
      ]} />
      {tab === 'apps' && <DevApps index={index} />}
      {tab === 'agents' && <Agents index={index} />}
      {tab === 'limits' && <Limits index={index} />}
    </OpsPage>
  );
}

function DevApps({ index }: { index: OpsIndex | undefined }) {
  const [status, setStatus] = useState('REVIEW');
  const list = useAsync(() => (can(index, 'developer-apps', 'list') ? opsApi.read<{ items: DevAppRow[] }>('developer-apps', 'list', { status: status || undefined }) : Promise.resolve(null)), [index?.available, status]);
  const { run, busy, modal } = useRun(index);
  const [notes, setNotes] = useState<Record<string, string>>({});
  if (!can(index, 'developer-apps', 'list')) return <Need level={3} />;
  const items = list.data?.items ?? [];
  return (
    <Section title="개발자 앱" action={
      <select aria-label="상태" className={selectClass('sm')} value={status} onChange={e => setStatus(e.target.value)}>
        <option value="REVIEW">검토 요청</option><option value="PUBLIC">공개</option><option value="PRIVATE">나만 연결</option><option value="">전체</option>
      </select>
    }>
      {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
      {list.loading && !list.data ? <SkeletonList rows={3} /> : items.length === 0 ? <Empty title={status === 'REVIEW' ? '검토를 기다리는 앱이 없어요' : '앱이 없어요'} /> : (
        <div className="adm-ops-list">
          {items.map(a => (
            <article key={a.client_id} className="ui-card adm-ops-item">
              <header><b>{a.name}</b><Badge tone={VIS[a.visibility][1]}>{VIS[a.visibility][0]}</Badge></header>
              <p>{a.description || <span className="adm-ops-dim">설명 없음</span>}</p>
              <dl className="adm-ops-kv">
                <div><dt>만든 사람</dt><dd>{a.owner_email ?? a.owner_user_id}</dd></div>
                <div><dt>웹사이트</dt><dd>{a.website ? <a href={a.website} target="_blank" rel="noreferrer noopener">{a.website}</a> : '—'}</dd></div>
                <div><dt>돌아갈 주소</dt><dd className="is-mono">{a.redirect_uris.join(', ')}</dd></div>
                <div><dt>검토 요청</dt><dd>{a.review_requested_at ? when(a.review_requested_at) : '—'}</dd></div>
                {a.review_note && <div><dt>지난 검토 메모</dt><dd>{a.review_note}</dd></div>}
              </dl>
              {a.visibility !== 'PRIVATE' && can(index, 'developer-apps', 'approve') && (
                <div className="adm-ops-inline">
                  <input aria-label="검토 메모" className={inputClass('sm')} value={notes[a.client_id] ?? ''} onChange={e => setNotes({ ...notes, [a.client_id]: e.target.value })}
                    placeholder={a.visibility === 'PUBLIC' ? '공개를 거둘 때 만든 사람에게 보낼 사유' : '메모 (돌려보낼 때는 꼭 적어 주세요)'} />
                  {a.visibility === 'REVIEW' && (
                    <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={async () => {
                      if (await run('developer-apps', 'approve', { client_id: a.client_id, note: notes[a.client_id]?.trim() || undefined }, { confirm: { title: '이 앱을 공개할까요?', message: '모든 아티스트가 이 앱을 연결할 수 있게 돼요. 이름·웹사이트·돌아갈 주소가 같은 개발자의 것인지 확인해 주세요.', confirmLabel: '공개' }, done: '앱을 공개했어요.' })) list.reload();
                    }}>공개 승인</button>
                  )}
                  <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !notes[a.client_id]?.trim()} onClick={async () => {
                    if (await run('developer-apps', 'return', { client_id: a.client_id, note: notes[a.client_id].trim() }, { confirm: { title: '만든 사람에게 돌려보낼까요?', message: '앱은 다시 만든 사람만 연결할 수 있고, 다른 아티스트의 연결은 끝나요.', confirmLabel: '돌려보내기', danger: true }, done: '앱을 돌려보냈어요.' })) list.reload();
                  }}>돌려보내기</button>
                </div>
              )}
            </article>
          ))}
        </div>
      )}
      {modal}
    </Section>
  );
}

function Agents({ index }: { index: OpsIndex | undefined }) {
  const list = useAsync(() => (can(index, 'ai-clients', 'list') ? opsApi.read<{ items: AgentRow[] }>('ai-clients', 'list') : Promise.resolve(null)), [index?.available]);
  const { run, busy, modal } = useRun(index);
  const [f, setF] = useState({ name: '', redirects: '', password: false, post: false });
  const [secret, setSecret] = useState<{ client_id: string; client_secret: string } | null>(null);
  if (!can(index, 'ai-clients', 'list')) return <Need level={3} />;
  const items = list.data?.items ?? [];
  return (
    <>
      <Section title="AI 에이전트 등록">
        <Callout tone="neutral">스스로 등록하지 못하는 AI 에이전트를 운영자가 등록해요. 시크릿은 한 번만 보여 줘요.</Callout>
        <form className="ui-card adm-ops-form" onSubmit={async e => {
          e.preventDefault();
          const out = await run<{ client_id: string; client_secret: string }>('ai-clients', 'add', {
            name: trim(f.name), password_grant: f.password, secret_post: f.post,
            redirect_uris: f.redirects.split(/\s+/).map(trim).filter(Boolean),
          }, { done: 'AI 에이전트를 등록했어요.' });
          if (out) { setSecret(out); setF({ name: '', redirects: '', password: false, post: false }); list.reload(); }
        }}>
          <Field label="이름" htmlFor="ops-a-name"><input id="ops-a-name" className={inputClass()} value={f.name} onChange={e => setF({ ...f, name: e.target.value })} /></Field>
          <Field label="돌아갈 주소" htmlFor="ops-a-redirect" help="여러 개면 줄을 바꿔 적어요. 없으면 비워 둬요.">
            <textarea id="ops-a-redirect" className="ui-textarea" rows={2} value={f.redirects} onChange={e => setF({ ...f, redirects: e.target.value })} />
          </Field>
          <label className="adm-ops-check"><input type="checkbox" checked={f.post} onChange={e => setF({ ...f, post: e.target.checked })} />시크릿을 요청 본문으로 보내요 (client_secret_post)</label>
          <label className="adm-ops-check"><input type="checkbox" checked={f.password} onChange={e => setF({ ...f, password: e.target.checked })} />비밀번호 방식 허용 — 모델이 이메일·비밀번호를 보지 않는 입력창이 있는 에이전트만</label>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !f.name.trim()}>등록</button>
        </form>
        {secret && (
          <Callout tone="success" action={<button type="button" className="ui-btn is-soft is-sm" onClick={() => setSecret(null)}>저장했어요</button>}>
            <b>지금만 볼 수 있어요.</b> 클라이언트 ID <code>{secret.client_id}</code> · 시크릿 <code>{secret.client_secret}</code>
          </Callout>
        )}
      </Section>
      <Section title="등록한 에이전트">
        {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
        {list.loading && !list.data ? <SkeletonList rows={2} /> : items.length === 0 ? <Empty title="등록한 에이전트가 없어요" /> : (
          <div className="ui-card adm-table-wrap">
            <table className="adm-table">
              <thead><tr><th>이름</th><th>클라이언트 ID</th><th>방식</th><th>상태</th><th aria-label="관리" /></tr></thead>
              <tbody>
                {items.map(a => (
                  <tr key={a.client_id}>
                    <td><b>{a.name}</b></td>
                    <td className="is-mono small">{a.client_id}</td>
                    <td className="small">{a.auth}{a.password_grant ? ' · 비밀번호' : ''}</td>
                    <td>{a.disabled_at ? <Badge>꺼짐</Badge> : <Badge tone="green">사용 중</Badge>}</td>
                    <td>{!a.disabled_at && <button type="button" className="ui-btn is-text is-sm is-danger" disabled={busy} onClick={async () => {
                      if (await run('ai-clients', 'disable', { client_id: a.client_id }, { confirm: { title: '이 에이전트를 끌까요?', message: '이 에이전트의 모든 연결이 끝나요.', confirmLabel: '끄기', danger: true }, done: '에이전트를 껐어요.' })) list.reload();
                    }}>끄기</button>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>
      {modal}
    </>
  );
}

function Limits({ index }: { index: OpsIndex | undefined }) {
  const list = useAsync(() => (can(index, 'ai-limits', 'list') ? opsApi.read<{ items: LimitRow[] }>('ai-limits', 'list') : Promise.resolve(null)), [index?.available]);
  const { run, busy, modal } = useRun(index);
  const [f, setF] = useState({ email: '', limit: '', note: '' });
  if (!can(index, 'ai-limits', 'list')) return <Need level={2} />;
  const items = list.data?.items ?? [];
  const n = Number(f.limit);
  return (
    <>
      <Section title="한도 바꾸기">
        <Callout tone="neutral">계정 없이 쓰는 AI 도구의 하루 사용 횟수예요. 문의를 받고 늘릴 때 써요. 기본값은 서버 설정을 따라요.</Callout>
        <form className="ui-card adm-ops-form" onSubmit={async e => {
          e.preventDefault();
          if (await run('ai-limits', 'set', { email: trim(f.email), limit: n, note: trim(f.note) || undefined }, { done: '한도를 바꿨어요.' })) { setF({ email: '', limit: '', note: '' }); list.reload(); }
        }}>
          <Field label="아티스트 계정 이메일" htmlFor="ops-l-email"><input id="ops-l-email" className={inputClass()} type="email" value={f.email} onChange={e => setF({ ...f, email: e.target.value })} /></Field>
          <Field label="하루 한도" htmlFor="ops-l-limit"><input id="ops-l-limit" className={inputClass()} inputMode="numeric" value={f.limit} onChange={e => setF({ ...f, limit: e.target.value.replace(/\D/g, '') })} /></Field>
          <Field label="메모" htmlFor="ops-l-note"><input id="ops-l-note" className={inputClass()} value={f.note} onChange={e => setF({ ...f, note: e.target.value })} placeholder="문의 번호 등" /></Field>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !f.email.trim() || !f.limit}>저장</button>
        </form>
      </Section>
      <Section title="따로 정한 한도">
        {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
        {list.loading && !list.data ? <SkeletonList rows={2} /> : items.length === 0 ? <Empty title="따로 정한 한도가 없어요" sub="모두 기본 한도를 써요." /> : (
          <div className="ui-card adm-table-wrap">
            <table className="adm-table">
              <thead><tr><th>계정</th><th>하루 한도</th><th>메모</th><th>바꾼 때</th><th aria-label="관리" /></tr></thead>
              <tbody>
                {items.map(l => (
                  <tr key={l.user_id}>
                    <td><b>{l.email ?? l.user_id}</b></td>
                    <td>{l.daily_limit.toLocaleString('ko-KR')}회</td>
                    <td className="small">{l.note ?? '—'}</td>
                    <td className="small" title={when(l.updated_at)}>{ago(l.updated_at)}</td>
                    <td>{l.email && <button type="button" className="ui-btn is-text is-sm" disabled={busy} onClick={async () => {
                      if (await run('ai-limits', 'clear', { email: l.email }, { done: '기본 한도로 돌렸어요.' })) list.reload();
                    }}>기본값으로</button>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>
      {modal}
    </>
  );
}

// ---------------------------------------------------------------------------
// 파트너·식별코드
// ---------------------------------------------------------------------------

export function OpsPartners() {
  const ix = useOpsIndex();
  const index = ix.data;
  const [tab, setTab] = useState<'partners' | 'routes' | 'codes'>('partners');
  return (
    <OpsPage eyebrow="운영 도구" title="파트너·식별코드" sub="DSP 직접 연결 파트너의 온보딩과 계약 경로, UPC·ISRC 발급 범위를 관리해요."
      index={index} loading={ix.loading} error={ix.error} reload={ix.reload}>
      <Tabs label="파트너·식별코드" value={tab} onChange={setTab} items={[
        { value: 'partners', label: '파트너' }, { value: 'routes', label: 'DSP 계약 경로' }, { value: 'codes', label: '식별코드' },
      ]} />
      {tab === 'partners' && <Partners index={index} />}
      {tab === 'routes' && <Routes index={index} />}
      {tab === 'codes' && <Issuers index={index} />}
    </OpsPage>
  );
}

const STAGE: Record<string, [string, 'gray' | 'blue' | 'green' | 'red']> = {
  PROSPECT: ['준비', 'gray'], ONBOARDING: ['온보딩', 'blue'], TESTING: ['테스트', 'blue'], LIVE: ['라이브', 'green'], SUSPENDED: ['중지', 'red'],
};

function Partners({ index }: { index: OpsIndex | undefined }) {
  const list = useAsync(() => (can(index, 'partners', 'list') ? opsApi.read<PartnerRow[]>('partners', 'list') : Promise.resolve(null)), [index?.available]);
  const [pick, setPick] = useState('');
  if (!can(index, 'partners', 'list')) return <Need level={2} />;
  const items = list.data ?? [];
  return (
    <Section title="파트너">
      {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
      {list.loading && !list.data ? <SkeletonList rows={3} /> : items.length === 0 ? <Empty title="등록된 파트너가 없어요" /> : (
        <div className="adm-ops-split">
          <div className="ui-card adm-ops-pick" role="list">
            {items.map(p => (
              <button key={p.partner_id} type="button" role="listitem" aria-current={pick === p.partner_id} className={pick === p.partner_id ? 'is-on' : ''} onClick={() => setPick(p.partner_id)}>
                <b>{p.display_name}</b>
                <small>{p.partner_id} · {p.transport}{p.delivery_enabled ? ' · 전송 중' : ''}</small>
              </button>
            ))}
          </div>
          {pick ? <PartnerDetail key={pick} index={index} id={pick} onChanged={list.reload} /> : <Empty title="파트너를 골라 주세요" sub="온보딩 상태와 남은 일을 보여 줘요." />}
        </div>
      )}
    </Section>
  );
}

function PartnerDetail({ index, id, onChanged }: { index: OpsIndex | undefined; id: string; onChanged: () => void }) {
  const st = useAsync(() => opsApi.read<PartnerStatus>('partners', 'status', { partner_id: id }), [id]);
  const { run, busy, modal } = useRun(index);
  const [out, setOut] = useState<unknown>(null);
  const [v, setV] = useState({ dpid: '', contract: '', dsp: '', reason: '', caps: '', ern: '', ack: '' });
  const edit = can(index, 'partners', 'probe');
  const s = st.data;
  const go = async (op: string, body: Record<string, unknown>, done: string, confirm?: RunOptions['confirm']) => {
    const r = await run('partners', op, { partner_id: id, ...body }, { done, confirm });
    if (r !== undefined) { setOut(r); st.reload(); onChanged(); }
  };
  const file = (key: 'ern' | 'ack') => async (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) { const text = await f.text(); setV(x => ({ ...x, [key]: text })); }
  };
  return (
    <div className="ui-card adm-ops-detail">
      {st.error && <ErrorBox message={st.error} onRetry={st.reload} />}
      {!s ? <SkeletonList rows={3} /> : (
        <>
          <header><b>{s.partner_id}</b><Badge tone={STAGE[s.stage]?.[1] ?? 'gray'}>{STAGE[s.stage]?.[0] ?? s.stage}</Badge></header>
          <dl className="adm-ops-kv">
            <div><dt>DDEX 수신자 ID</dt><dd>{s.dpid_registered ? '등록됨' : '없음'}</dd></div>
            <div><dt>연결 주소</dt><dd className="is-mono">{s.endpoint_url ?? '—'} · {s.endpoint_health}</dd></div>
            <div><dt>인증 정보</dt><dd>{s.credential_kind ?? '—'} · {s.credential_status}</dd></div>
            <div><dt>ERN 테스트</dt><dd>{s.test_ern_validated_at ? when(s.test_ern_validated_at) : '—'}</dd></div>
            <div><dt>ACK 테스트</dt><dd>{s.test_ack_parsed_at ? when(s.test_ack_parsed_at) : '—'}</dd></div>
            <div><dt>계약</dt><dd>{s.contract_ref ? `${s.contract_ref} · ${when(s.contract_signed_at)}` : '—'}</dd></div>
          </dl>
          {s.gaps.length > 0
            ? <Callout tone="warn">라이브 전에 남은 일: {s.gaps.join(', ')}</Callout>
            : <Callout tone="success">남은 일이 없어요.</Callout>}
          {edit && (
            <div className="adm-ops-sub">
              <div className="adm-ops-inline">
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => go('probe', {}, '연결을 확인했어요.', { title: '연결을 확인할까요?', message: '파트너 서버로 실제 요청을 한 번 보내요.', confirmLabel: '확인 요청' })}>연결 확인</button>
              </div>
              <div className="adm-ops-inline">
                <input aria-label="DDEX 수신자 ID" className={inputClass('sm', 'is-mono')} value={v.dpid} onChange={e => setV({ ...v, dpid: e.target.value })} placeholder="PADPIDA…" />
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !v.dpid.trim()} onClick={() => go('dpid', { dpid: trim(v.dpid) }, '수신자 ID를 저장했어요.')}>수신자 ID 저장</button>
              </div>
              <div className="adm-ops-inline">
                <input aria-label="계약 문서 번호" className={inputClass('sm')} value={v.contract} onChange={e => setV({ ...v, contract: e.target.value })} placeholder="서명한 계약 문서 번호" />
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !v.contract.trim()} onClick={() => go('contract', { reference: trim(v.contract) }, '계약을 기록했어요.')}>계약 기록</button>
              </div>
              <div className="adm-ops-inline">
                <input aria-label="DSP ID" className={inputClass('sm', 'is-mono')} value={v.dsp} onChange={e => setV({ ...v, dsp: e.target.value })} placeholder="DSP UUID (비우면 자동)" />
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => go('set-dsp', { dsp_id: trim(v.dsp) || undefined }, 'DSP를 연결했어요.')}>DSP 연결</button>
              </div>
              <div className="adm-ops-inline">
                <label className="ui-btn is-soft is-sm">ERN 파일<input type="file" accept=".xml" hidden onChange={file('ern')} /></label>
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !v.ern} onClick={() => go('test-ern', { xml: v.ern }, 'ERN을 검사했어요.')}>ERN 검사</button>
                <label className="ui-btn is-soft is-sm">ACK 파일<input type="file" accept=".xml,.json,.txt" hidden onChange={file('ack')} /></label>
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !v.ack} onClick={() => go('test-ack', { content: v.ack }, 'ACK를 읽었어요.')}>ACK 검사</button>
              </div>
              <div className="adm-ops-inline">
                <input aria-label="기능 JSON" className={inputClass('sm', 'is-mono')} value={v.caps} onChange={e => setV({ ...v, caps: e.target.value })} placeholder='{"send_or_publish":true}' />
                <button type="button" className="ui-btn is-soft is-sm" disabled={busy || !v.caps.trim()} onClick={() => {
                  let flags: unknown; try { flags = JSON.parse(v.caps); } catch { flags = null; }
                  if (flags && typeof flags === 'object') void go('capabilities', { flags }, '기능을 저장했어요.');
                }}>기능 저장</button>
              </div>
              <div className="adm-ops-inline">
                <input aria-label="중지 사유" className={inputClass('sm')} value={v.reason} onChange={e => setV({ ...v, reason: e.target.value })} placeholder="중지 사유" />
                <button type="button" className="ui-btn is-danger is-sm" disabled={busy || !v.reason.trim()} onClick={() => go('suspend', { reason: trim(v.reason) }, '파트너 전송을 중지했어요.', { title: '전송을 중지할까요?', message: '이 파트너로 가는 전송이 멈춰요.', confirmLabel: '중지', danger: true })}>전송 중지</button>
                {can(index, 'partners', 'go-live') && (
                  <button type="button" className="ui-btn is-primary is-sm" disabled={busy || s.gaps.length > 0} onClick={() => go('go-live', {}, '라이브로 바꿨어요.', { title: '라이브로 바꿀까요?', message: '실제 발매가 이 파트너로 전송되기 시작해요.', confirmLabel: '라이브 전환' })}>라이브 전환</button>
                )}
              </div>
            </div>
          )}
          {out != null && <pre className="adm-ops-out">{JSON.stringify(out, null, 2)}</pre>}
        </>
      )}
      {modal}
    </div>
  );
}

function Routes({ index }: { index: OpsIndex | undefined }) {
  const { run, busy, modal } = useRun(index);
  const [f, setF] = useState({ code: '', route: 'DIRECT', merlin: true });
  const [out, setOut] = useState<unknown>(null);
  if (!can(index, 'partners', 'route')) return <Need level={3} />;
  return (
    <Section title="DSP 계약 경로">
      <Callout tone="neutral">DSP(D-1 ~ D-11)마다 직접 계약으로 보낼지 Merlin을 거칠지 정해요. Merlin 계약 자체는 파트너 탭의 merlin 파트너에 기록해요.</Callout>
      <form className="ui-card adm-ops-form" onSubmit={async e => {
        e.preventDefault();
        const r = await run('partners', 'route', { dsp_code: trim(f.code).toUpperCase(), route: f.route }, { done: '계약 경로를 저장했어요.' });
        if (r !== undefined) setOut(r);
      }}>
        <Field label="DSP 코드" htmlFor="ops-r-code"><input id="ops-r-code" className={inputClass('md', 'is-mono')} value={f.code} onChange={e => setF({ ...f, code: e.target.value })} placeholder="D-5" /></Field>
        <Field label="경로" htmlFor="ops-r-route">
          <select id="ops-r-route" className={selectClass()} value={f.route} onChange={e => setF({ ...f, route: e.target.value })}>
            <option value="DIRECT">직접 계약</option><option value="MERLIN">Merlin</option>
          </select>
        </Field>
        <button type="submit" className="ui-btn is-primary" disabled={busy || !f.code.trim()}>경로 저장</button>
        <label className="adm-ops-check"><input type="checkbox" checked={f.merlin} onChange={e => setF({ ...f, merlin: e.target.checked })} />Merlin으로 보낼 수 있는 DSP</label>
        <button type="button" className="ui-btn is-soft" disabled={busy || !f.code.trim()} onClick={async () => {
          if (await run('partners', 'merlin-eligible', { dsp_code: trim(f.code).toUpperCase(), eligible: f.merlin }, { done: 'Merlin 가능 여부를 저장했어요.' })) setOut(null);
        }}>Merlin 가능 여부 저장</button>
      </form>
      {out != null && <pre className="adm-ops-out">{JSON.stringify(out, null, 2)}</pre>}
      {modal}
    </Section>
  );
}

function Issuers({ index }: { index: OpsIndex | undefined }) {
  const list = useAsync(() => (can(index, 'identifiers', 'list') ? opsApi.read<IssuerRow[]>('identifiers', 'list') : Promise.resolve(null)), [index?.available]);
  const { run, busy, modal } = useRun(index);
  const [f, setF] = useState({ kind: 'ISRC', prefix: '' });
  if (!can(index, 'identifiers', 'list')) return <Need level={3} />;
  const items = list.data ?? [];
  return (
    <>
      {can(index, 'identifiers', 'register') && (
        <Section title="실제 발급 범위 등록">
          <Callout tone="warn">등록하면 그 종류의 새 코드는 이 범위에서 발급돼요. 이미 발급한 코드는 바뀌지 않고, 되돌릴 수 없어요.</Callout>
          <form className="ui-card adm-ops-form" onSubmit={async e => {
            e.preventDefault();
            if (await run('identifiers', 'register', { kind: f.kind, prefix: trim(f.prefix) }, { confirm: { title: `${f.kind} 범위를 등록할까요?`, message: `${trim(f.prefix)} 범위가 새 ${f.kind} 발급에 쓰여요. 지금 쓰는 범위는 더 이상 발급하지 않아요.`, confirmLabel: '등록', danger: true }, done: '발급 범위를 등록했어요.' })) { setF({ ...f, prefix: '' }); list.reload(); }
          }}>
            <Field label="종류" htmlFor="ops-i-kind">
              <select id="ops-i-kind" className={selectClass()} value={f.kind} onChange={e => setF({ ...f, kind: e.target.value })}>
                <option value="ISRC">ISRC (등록자 코드)</option><option value="UPC">UPC (GS1 업체 코드)</option>
              </select>
            </Field>
            <Field label="범위" htmlFor="ops-i-prefix" help={f.kind === 'ISRC' ? '예: KR-A1B' : 'GS1에서 받은 업체 코드'}>
              <input id="ops-i-prefix" className={inputClass('md', 'is-mono')} value={f.prefix} onChange={e => setF({ ...f, prefix: e.target.value })} />
            </Field>
            <button type="submit" className="ui-btn is-primary" disabled={busy || !f.prefix.trim()}>등록</button>
          </form>
        </Section>
      )}
      <Section title="발급 범위">
        {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
        {list.loading && !list.data ? <SkeletonList rows={2} /> : items.length === 0 ? <Empty title="발급 범위가 없어요" /> : (
          <div className="ui-card adm-table-wrap">
            <table className="adm-table">
              <thead><tr><th>종류</th><th>범위</th><th>방식</th><th>상태</th><th>발급 현황</th></tr></thead>
              <tbody>
                {items.map(i => (
                  <tr key={`${i.kind}-${i.prefix}`}>
                    <td><b>{i.kind}</b></td>
                    <td className="is-mono">{i.prefix}</td>
                    <td>{i.mode === 'VIRTUAL' ? '테스트 범위' : '실제 범위'}</td>
                    <td>{i.active ? <Badge tone="green">발급 중</Badge> : <Badge>멈춤</Badge>}</td>
                    <td className="small is-mono">{Object.entries(i.issued).map(([k, n]) => `${k}: ${n}`).join(' · ') || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>
      {modal}
    </>
  );
}

// ---------------------------------------------------------------------------
// 정산·지급 (정산 업무)
// ---------------------------------------------------------------------------

export function OpsFinance() {
  const ix = useOpsIndex();
  const index = ix.data;
  const [tab, setTab] = useState<'payouts' | 'royalty' | 'balance'>('payouts');
  // 정산금 추가는 CLI로만 주는 최고 관리자(레벨 4)에게만 보인다 — 서버와 DB가 다시 확인한다
  const owner = can(index, 'balance', 'credit');
  return (
    <OpsPage eyebrow="운영 도구" title="정산·지급 처리" sub="지급 요청을 지시·승인·실행하고, DSP 정산 리포트를 가져와 장부에 반영해요. 승인한 사람과 실행하는 사람은 달라야 해요."
      index={index} loading={ix.loading} error={ix.error} reload={ix.reload}>
      {!can(index, 'payouts', 'list') ? <Need finance /> : (
        <>
          <Tabs label="정산·지급" value={tab} onChange={setTab} items={[
            { value: 'payouts', label: '지급 처리' }, { value: 'royalty', label: '로열티 정산' },
            ...(owner ? [{ value: 'balance' as const, label: '정산금 추가' }] : []),
          ]} />
          {tab === 'payouts' ? <PayoutDesk index={index} /> : tab === 'royalty' ? <RoyaltyDesk index={index} /> : <BalanceDesk index={index} />}
        </>
      )}
    </OpsPage>
  );
}

const ORDER_STATUS: Record<string, [string, 'gray' | 'blue' | 'amber' | 'green' | 'red' | 'violet']> = {
  PENDING_APPROVAL: ['승인 대기', 'violet'], APPROVED: ['실행 대기', 'blue'], SUBMITTED: ['은행 처리 중', 'blue'],
  SETTLED: ['지급 완료', 'green'], FAILED: ['실패', 'red'], RETURNED: ['반환됨', 'amber'], UNKNOWN: ['결과 확인 필요', 'amber'], CANCELLED: ['취소', 'gray'],
};

function PayoutDesk({ index }: { index: OpsIndex | undefined }) {
  const [status, setStatus] = useState('REQUESTED');
  const list = useAsync(() => opsApi.read<PayoutRow[]>('payouts', 'list', { status }), [status]);
  const { run, busy, modal } = useRun(index);
  const [act, setAct] = useState<{ row: PayoutRow; kind: 'submit' | 'result' | 'manual' | 'return' } | null>(null);
  const items = list.data ?? [];
  const simple = async (op: string, row: PayoutRow, done: string, confirm: RunOptions['confirm']) => {
    if (await run('payouts', op, op === 'order' ? { request_id: row.request_id } : { order_id: row.order_id }, { done, confirm })) list.reload();
  };
  return (
    <Section title="지급 요청" action={
      <select aria-label="요청 상태" className={selectClass('sm')} value={status} onChange={e => setStatus(e.target.value)}>
        <option value="REQUESTED">요청됨</option><option value="ORDERED">지시됨</option><option value="REJECTED">거절됨</option><option value="CANCELLED">취소됨</option>
      </select>
    }>
      {list.error && <ErrorBox message={list.error} onRetry={list.reload} />}
      {list.loading && !list.data ? <SkeletonList rows={3} /> : items.length === 0 ? <Empty title="이 상태의 지급 요청이 없어요" /> : (
        <div className="adm-ops-list">
          {items.map(p => {
            const os = p.order_status ?? '';
            const money = (label: string) => `${p.account.holder_name ?? ''} ${p.account.bank_name ?? ''} ${p.account.account_masked ?? ''} 계좌로 ${Number(p.net).toLocaleString('ko-KR')}원(신청 ${Number(p.amount).toLocaleString('ko-KR')}원, 수수료 ${Number(p.fee).toLocaleString('ko-KR')}원)을 ${label}`;
            return (
              <article key={p.request_id} className="ui-card adm-ops-item">
                <header>
                  <b><Money value={p.net} mark /></b>
                  <small className="adm-ops-dim">신청 <Money value={p.amount} /> · 수수료 <Money value={p.fee} /></small>
                  {p.order_id ? <Badge tone={ORDER_STATUS[os]?.[1] ?? 'gray'}>{ORDER_STATUS[os]?.[0] ?? os}</Badge> : <Badge tone="violet">지시 전</Badge>}
                </header>
                <p className="small">{p.account.holder_name} · {p.account.bank_name} {p.account.account_masked} · <span title={when(p.created_at)}>{ago(p.created_at)}</span> · <span className="is-mono">{p.org_id.slice(0, 8)}</span></p>
                <div className="adm-ops-inline">
                  {!p.order_id && p.status === 'REQUESTED' && <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => simple('order', p, '지급을 지시했어요. 다른 담당자의 승인이 필요해요.', { title: '지급을 지시할까요?', message: money('보내도록 지시해요.'), confirmLabel: '지시' })}>지급 지시</button>}
                  {os === 'PENDING_APPROVAL' && <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => simple('approve', p, '지급을 승인했어요.', { title: '지급을 승인할까요?', message: money('보내는 지시를 승인해요. 실행은 다른 담당자가 해요.'), confirmLabel: '승인' })}>승인</button>}
                  {os === 'APPROVED' && <>
                    <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => setAct({ row: p, kind: 'submit' })}>은행 전송 기록</button>
                    <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => setAct({ row: p, kind: 'manual' })}>직접 이체 기록</button>
                  </>}
                  {(os === 'SUBMITTED' || os === 'UNKNOWN') && <button type="button" className="ui-btn is-primary is-sm" disabled={busy} onClick={() => setAct({ row: p, kind: 'result' })}>은행 결과 기록</button>}
                  {os === 'SETTLED' && <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => setAct({ row: p, kind: 'return' })}>반환 기록</button>}
                  {(os === 'RETURNED' || os === 'FAILED') && <button type="button" className="ui-btn is-soft is-sm" disabled={busy} onClick={() => simple('reissue', p, '다시 지시했어요. 승인이 필요해요.', { title: '다시 지시할까요?', message: '지금 등록된 계좌로 새 지급 지시를 만들어요.', confirmLabel: '다시 지시' })}>다시 지시</button>}
                  {(os === 'PENDING_APPROVAL' || os === 'APPROVED') && <button type="button" className="ui-btn is-text is-sm is-danger" disabled={busy} onClick={() => simple('cancel', p, '지급 지시를 취소했어요.', { title: '지급 지시를 취소할까요?', message: '은행으로 보내기 전이라 돈은 움직이지 않아요.', confirmLabel: '지시 취소', danger: true })}>지시 취소</button>}
                </div>
              </article>
            );
          })}
        </div>
      )}
      {act && <PayoutAction act={act} busy={busy} onClose={() => setAct(null)} onRun={async (op, body, done) => {
        if (await run('payouts', op, { order_id: act.row.order_id, ...body }, { done })) { setAct(null); list.reload(); }
      }} />}
      {modal}
    </Section>
  );
}

function PayoutAction({ act, busy, onClose, onRun }: {
  act: { row: PayoutRow; kind: 'submit' | 'result' | 'manual' | 'return' }; busy: boolean; onClose: () => void;
  onRun: (op: string, body: Record<string, unknown>, done: string) => void;
}) {
  const [bank, setBank] = useState('');
  const [outcome, setOutcome] = useState('settled');
  const [evidence, setEvidence] = useState('');
  const [amount, setAmount] = useState(act.row.net.replace(/\.0+$/, ''));
  const [refund, setRefund] = useState(false);
  const title = { submit: '은행 전송 기록', result: '은행 결과 기록', manual: '직접 이체 기록', return: '반환 기록' }[act.kind];
  const ok = act.kind === 'result' ? true : act.kind === 'submit' ? !!bank.trim() : !!bank.trim() && !!evidence.trim() && (act.kind !== 'return' || !!amount);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!ok) return;
    if (act.kind === 'submit') onRun('submit', { bank_tx: bank.trim() }, '은행 전송을 기록했어요.');
    if (act.kind === 'result') onRun('result', { outcome, bank_tx: bank.trim() || undefined }, '은행 결과를 기록했어요.');
    if (act.kind === 'manual') onRun('manual', { bank_tx: bank.trim(), evidence: { transfer_receipt: evidence.trim() } }, '직접 이체를 기록했어요. 지급 완료로 바뀌어요.');
    if (act.kind === 'return') onRun('return', { bank_reference: bank.trim(), amount, refund_fee: refund, evidence: { statement: evidence.trim() } }, '반환을 기록했어요.');
  };
  return (
    <Modal title={title} onClose={onClose} dismissible={false}>
      <form onSubmit={submit} className="adm-ops-form is-modal">
        <p className="small">{act.row.account.holder_name} · {act.row.account.bank_name} {act.row.account.account_masked} · 실지급 <Money value={act.row.net} mark /></p>
        <Field label={act.kind === 'return' ? '반환 거래 번호' : '은행 거래 번호'} htmlFor="ops-pay-bank" help={act.kind === 'result' ? '결과를 받은 거래 번호 (있으면)' : undefined}>
          <input id="ops-pay-bank" className={inputClass('md', 'is-mono')} value={bank} onChange={e => setBank(e.target.value)} />
        </Field>
        {act.kind === 'result' && (
          <Field label="결과" htmlFor="ops-pay-outcome" help="결과를 모르면 '확인 필요'로 두세요. 확인 필요인 지급은 자동으로 다시 보내지 않아요.">
            <select id="ops-pay-outcome" className={selectClass()} value={outcome} onChange={e => setOutcome(e.target.value)}>
              <option value="settled">지급 완료</option><option value="failed">실패</option><option value="returned">반환됨</option><option value="unknown">확인 필요</option>
            </select>
          </Field>
        )}
        {act.kind === 'return' && (
          <>
            <Field label="반환된 금액(원)" htmlFor="ops-pay-amount"><input id="ops-pay-amount" className={inputClass()} inputMode="numeric" value={amount} onChange={e => setAmount(e.target.value.replace(/[^\d.]/g, ''))} /></Field>
            <label className="adm-ops-check"><input type="checkbox" checked={refund} onChange={e => setRefund(e.target.checked)} />수수료도 돌려줘요</label>
          </>
        )}
        {(act.kind === 'manual' || act.kind === 'return') && (
          <Field label={act.kind === 'manual' ? '이체 증빙' : '반환 증빙'} htmlFor="ops-pay-evidence" help="이체확인증 번호, 은행 안내 등 확인한 근거를 적어요.">
            <textarea id="ops-pay-evidence" className="ui-textarea" rows={3} value={evidence} onChange={e => setEvidence(e.target.value)} />
          </Field>
        )}
        <div className="adm-ops-actions">
          <button type="button" className="ui-btn is-ghost" onClick={onClose}>닫기</button>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !ok}>기록</button>
        </div>
      </form>
    </Modal>
  );
}

const BALANCE_MAX = 10_000_000;
const ORG_KIND: Record<string, string> = { PERSONAL: '개인', LABEL: '레이블', COMPANY: '회사' };

/** 정산금 직접 추가 — 최고 관리자만. 계정 이메일로 작업 공간을 찾고, 금액·내역 이름·사유를 남긴다.
 *  장부 기록이라 고치거나 지울 수 없어서, 확인 창과 인증 코드를 거친다 (crates/core/src/staff_ops.rs balance.credit, 0159) */
function BalanceDesk({ index }: { index: OpsIndex | undefined }) {
  const { run, busy, modal } = useRun(index);
  const [email, setEmail] = useState('');
  const [found, setFound] = useState<BalanceAccount | null>(null);
  const [looking, setLooking] = useState(false);
  const [lookErr, setLookErr] = useState('');
  const [org, setOrg] = useState('');
  const [f, setF] = useState({ amount: '', title: '', reason: '' });
  if (!can(index, 'balance', 'credit')) return <Need level={4} />;
  const lookup = async (to = email.trim()) => {
    if (!to) return;
    setLooking(true); setLookErr('');
    try {
      const r = await opsApi.read<BalanceAccount>('balance', 'show', { email: to });
      setFound(r);
      setOrg(o => (r.orgs.some(x => x.org_id === o) ? o : r.orgs[0]?.org_id ?? ''));
    } catch (e) { setFound(null); setLookErr(opsError(e)); } finally { setLooking(false); }
  };
  const target = found?.orgs.find(o => o.org_id === org);
  const amount = Number(f.amount || 0);
  const amountErr = f.amount && (amount <= 0 || amount > BALANCE_MAX) ? `1원부터 ${BALANCE_MAX.toLocaleString('ko-KR')}원까지 넣을 수 있어요.` : '';
  const ready = !!target && amount > 0 && amount <= BALANCE_MAX && !!f.title.trim() && !!f.reason.trim();
  const won = (n: number | string) => `${Number(n).toLocaleString('ko-KR')}원`;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready || !target) return;
    const res = await run('balance', 'credit', { org_id: target.org_id, amount: String(amount), title: f.title.trim(), reason: f.reason.trim() }, {
      confirm: {
        title: '정산금을 추가할까요?',
        message: `${target.name}(${found?.email})의 정산금에 ${won(amount)}을 더해요. 아티스트 정산 내역에 ‘${f.title.trim()}’으로 보이고 바로 출금할 수 있게 돼요. 장부 기록이라 고치거나 지울 수 없어요.`,
        confirmLabel: '추가', danger: true,
      },
      done: '정산금을 추가했어요. 아티스트에게 알림이 갔어요.',
    });
    if (res !== undefined) { setF({ amount: '', title: '', reason: '' }); await lookup(found?.email); }
  };
  return (
    <>
      <Section title="정산금 추가">
        <Callout tone="warn">최고 관리자만 할 수 있어요. 추가한 금액은 아티스트가 바로 출금하거나 부가서비스 결제에 쓸 수 있고, 장부에 남아 고치거나 지울 수 없어요. 누가 언제 왜 넣었는지 감사 기록에 남아요.</Callout>
        <form className="ui-card adm-ops-form" onSubmit={e => { e.preventDefault(); void lookup(); }}>
          <Field label="아티스트 계정 이메일" htmlFor="ops-b-email" error={lookErr || undefined}>
            <input id="ops-b-email" type="email" className={inputClass()} autoComplete="off" value={email} onChange={e => setEmail(e.target.value)} placeholder="artist@example.com" />
          </Field>
          <button type="submit" className="ui-btn is-soft" disabled={looking || !email.trim()}>{looking ? '찾는 중…' : '찾기'}</button>
        </form>
      </Section>
      {found && (found.orgs.length === 0 ? <Empty title="이 계정에 속한 작업 공간이 없어요" /> : (
        <Section title="넣을 곳">
          {target && (
            <dl className="ui-card adm-ops-kv">
              <div><dt>작업 공간</dt><dd>{target.name} · {ORG_KIND[target.kind] ?? target.kind} <span className="is-mono adm-ops-dim">{target.org_id.slice(0, 8)}</span></dd></div>
              <div><dt>지금 받을 수 있는 금액</dt><dd><Money value={target.withdrawable} /></dd></div>
              {amount > 0 && !amountErr && <div><dt>추가한 뒤</dt><dd><b><Money value={Number(target.withdrawable) + amount} /></b></dd></div>}
            </dl>
          )}
          <form className="ui-card adm-ops-form" onSubmit={submit}>
            {found.orgs.length > 1 && (
              <Field label="작업 공간" htmlFor="ops-b-org">
                <select id="ops-b-org" className={selectClass()} value={org} onChange={e => setOrg(e.target.value)}>
                  {found.orgs.map(o => <option key={o.org_id} value={o.org_id}>{o.name} · {ORG_KIND[o.kind] ?? o.kind}{o.role === 'OWNER' ? '' : ` (${o.role})`}</option>)}
                </select>
              </Field>
            )}
            <Field label="금액 (원)" htmlFor="ops-b-amount" error={amountErr || undefined} help={`한 번에 ${won(BALANCE_MAX)}까지`}>
              <input id="ops-b-amount" className={inputClass()} inputMode="numeric" value={f.amount ? Number(f.amount).toLocaleString('ko-KR') : ''}
                onChange={e => setF({ ...f, amount: e.target.value.replace(/\D/g, '').replace(/^0+/, '').slice(0, 9) })} placeholder="0" />
            </Field>
            <Field label="정산 내역에 보일 이름" htmlFor="ops-b-title" help="아티스트에게 그대로 보여요. 예: 9월 정산금">
              <input id="ops-b-title" className={inputClass()} maxLength={100} value={f.title} onChange={e => setF({ ...f, title: e.target.value })} />
            </Field>
            <Field label="사유 (내부 기록)" htmlFor="ops-b-reason" help="아티스트에게는 보이지 않아요. 근거 문서·문의 번호를 남겨 주세요.">
              <input id="ops-b-reason" className={inputClass()} maxLength={500} value={f.reason} onChange={e => setF({ ...f, reason: e.target.value })} />
            </Field>
            <button type="submit" className="ui-btn is-primary" disabled={busy || !ready}>{amount > 0 && !amountErr ? `${won(amount)} 추가` : '추가'}</button>
          </form>
          {target && target.credits.length > 0 && (
            <div className="ui-card adm-table-wrap">
              <table className="adm-table">
                <thead><tr><th>최근 추가</th><th>금액</th><th>사유</th><th>시간</th></tr></thead>
                <tbody>
                  {target.credits.map(c => (
                    <tr key={c.id}>
                      <td><b>{c.title}</b></td>
                      <td><Money value={c.amount} /></td>
                      <td className="small">{c.reason ?? '—'}</td>
                      <td className="small" title={when(c.created_at)}>{ago(c.created_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>
      ))}
      {modal}
    </>
  );
}

function RoyaltyDesk({ index }: { index: OpsIndex | undefined }) {
  const { run, busy, modal } = useRun(index);
  const [org, setOrg] = useState('');
  const [report, setReport] = useState('');
  const [f, setF] = useState({ dsp: '', start: '', end: '', currency: 'USD', file: null as File | null });
  const [r, setR] = useState({ bank: '', amount: '', currency: 'USD', evidence: '', fx: '' });
  const [shown, setShown] = useState<RoyaltyReport | null>(null);
  const [out, setOut] = useState<unknown>(null);
  const toast = useToast();
  const ids = { org_id: org.trim(), report_id: report.trim() };
  const show = async () => {
    try { setShown(await opsApi.read<RoyaltyReport>('royalty', 'show', ids)); } catch (e) { setShown(null); toast(opsError(e), 'error'); }
  };
  const after = async (res: unknown) => { if (res !== undefined) { setOut(res); if (ids.report_id) await show(); } };
  return (
    <>
      <Section title="리포트 가져오기">
        <form className="ui-card adm-ops-form" onSubmit={async e => {
          e.preventDefault();
          if (!f.file) return;
          const content = await f.file.text();
          const res = await run<{ report_id: string }>('royalty', 'import', { org_id: org.trim(), dsp_id: f.dsp.trim(), period_start: f.start, period_end: f.end, currency: f.currency.trim().toUpperCase(), filename: f.file.name, content }, { done: '리포트를 가져왔어요.' });
          if (res) { setReport(res.report_id); setOut(res); }
        }}>
          <Field label="작업 공간 ID" htmlFor="ops-r-org"><input id="ops-r-org" className={inputClass('md', 'is-mono')} value={org} onChange={e => setOrg(e.target.value)} /></Field>
          <Field label="DSP" htmlFor="ops-r-dsp"><input id="ops-r-dsp" className={inputClass()} value={f.dsp} onChange={e => setF({ ...f, dsp: e.target.value })} placeholder="spotify" /></Field>
          <Field label="기간 시작" htmlFor="ops-r-start"><DateField id="ops-r-start" value={f.start} onChange={v => setF({ ...f, start: v })} /></Field>
          <Field label="기간 끝" htmlFor="ops-r-end"><DateField id="ops-r-end" value={f.end} onChange={v => setF({ ...f, end: v })} /></Field>
          <Field label="통화" htmlFor="ops-r-cur"><input id="ops-r-cur" className={inputClass('md', 'is-mono')} maxLength={3} value={f.currency} onChange={e => setF({ ...f, currency: e.target.value })} /></Field>
          <Field label="CSV 파일" htmlFor="ops-r-file"><input id="ops-r-file" type="file" accept=".csv,text/csv" onChange={e => setF({ ...f, file: e.target.files?.[0] ?? null })} /></Field>
          <button type="submit" className="ui-btn is-primary" disabled={busy || !org.trim() || !f.dsp.trim() || !f.start || !f.end || !f.file}>가져오기</button>
        </form>
      </Section>
      <Section title="리포트 처리">
        <div className="ui-card adm-ops-form">
          <Field label="작업 공간 ID" htmlFor="ops-r-org2"><input id="ops-r-org2" className={inputClass('md', 'is-mono')} value={org} onChange={e => setOrg(e.target.value)} /></Field>
          <Field label="리포트 ID" htmlFor="ops-r-report"><input id="ops-r-report" className={inputClass('md', 'is-mono')} value={report} onChange={e => setReport(e.target.value)} /></Field>
          <div className="adm-ops-inline">
            <button type="button" className="ui-btn is-soft" disabled={!ids.org_id || !ids.report_id} onClick={show}>보기</button>
            <button type="button" className="ui-btn is-soft" disabled={busy || !ids.org_id || !ids.report_id} onClick={async () => after(await run('royalty', 'match', ids, { done: '곡을 다시 맞췄어요.' }))}>곡 맞추기</button>
          </div>
          {shown && (
            <dl className="adm-ops-kv">
              <div><dt>상태</dt><dd>{shown.status}</dd></div>
              <div><dt>리포트 금액</dt><dd>{shown.source_amount} {shown.currency}</dd></div>
              <div><dt>받은 금액</dt><dd>{shown.received_amount} {shown.currency}</dd></div>
              <div><dt>못 맞춘 줄</dt><dd>{shown.unresolved}</dd></div>
            </dl>
          )}
          <Field label="입금 거래 번호" htmlFor="ops-r-bank"><input id="ops-r-bank" className={inputClass('md', 'is-mono')} value={r.bank} onChange={e => setR({ ...r, bank: e.target.value })} /></Field>
          <Field label="입금액" htmlFor="ops-r-amount"><input id="ops-r-amount" className={inputClass()} inputMode="decimal" value={r.amount} onChange={e => setR({ ...r, amount: e.target.value.replace(/[^\d.]/g, '') })} /></Field>
          <Field label="입금 통화" htmlFor="ops-r-cur2"><input id="ops-r-cur2" className={inputClass('md', 'is-mono')} maxLength={3} value={r.currency} onChange={e => setR({ ...r, currency: e.target.value })} /></Field>
          <Field label="입금 증빙" htmlFor="ops-r-evidence"><textarea id="ops-r-evidence" className="ui-textarea" rows={2} value={r.evidence} onChange={e => setR({ ...r, evidence: e.target.value })} /></Field>
          <button type="button" className="ui-btn is-soft" disabled={busy || !ids.org_id || !ids.report_id || !r.bank.trim() || !r.amount || !r.evidence.trim()} onClick={async () => after(await run('royalty', 'receipt', { ...ids, bank_reference: r.bank.trim(), amount: r.amount, currency: r.currency.trim().toUpperCase(), evidence: { statement: r.evidence.trim() } }, { done: '입금을 기록했어요.' }))}>입금 기록</button>
          <Field label="환율 (리포트 통화 1단위당 원)" htmlFor="ops-r-fx" help="원화 리포트는 1이에요."><input id="ops-r-fx" className={inputClass()} inputMode="decimal" value={r.fx} onChange={e => setR({ ...r, fx: e.target.value.replace(/[^\d.]/g, '') })} /></Field>
          <button type="button" className="ui-btn is-primary" disabled={busy || !ids.org_id || !ids.report_id || !r.fx} onClick={async () => after(await run('royalty', 'post', { ...ids, fx_rate: r.fx }, { confirm: { title: '장부에 반영할까요?', message: `환율 ${r.fx}로 수수료·아티스트·참여자 몫을 나눠 장부에 기록해요. 되돌리려면 반대 기록이 필요해요.`, confirmLabel: '반영' }, done: '장부에 반영했어요.' }))}>장부 반영</button>
        </div>
        {out != null && <pre className="adm-ops-out">{JSON.stringify(out, null, 2)}</pre>}
      </Section>
      {modal}
    </>
  );
}
