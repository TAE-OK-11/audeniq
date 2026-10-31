// 앨범 퍼블리싱 탭이 함께 쓰는 계산 — 신청 전 준비 점검 · 참여자 확인 현황 · 지분 변경 전후 · 참여자 재사용 · 저작권료 기간·조건.
// 화면(AlbumPublishingTab · WorkSheet · Publishing)은 이 결과만 그린다 (테스트: publishingPrep.test.ts)
import type { Creator, Royalty, Work, WorkContributor } from '../api/publishing';
import { ROLE_LABEL, isManaged, pct, societyName, splitIssues, sumShares } from './publishing';
import { uid } from './store';

// ---------- 신청 전 준비 점검 ----------

/** 곡 시트에서 고칠 자리 — 누르면 그 칸으로 내려가 연다 */
export type PrepField = 'split' | 'confirm' | 'ipi' | 'fix' | 'evidence';
export interface WorkIssue { field: PrepField; text: string; blocking: boolean; contributorId?: string }

export type WorkStage = 'ready' | 'blocked' | 'fix' | 'applied' | 'signing' | 'managed' | 'readonly';
export const STAGE_LABEL: Record<WorkStage, string> = { ready: '신청 가능', blocked: '확인 필요', fix: '보완 요청', applied: '심사 중', signing: '계약서 서명', managed: '관리 중', readonly: '공동 참여' };
/** 상태 색 — 신청 가능은 할 수 있는 일(파랑), 심사 중 보라, 관리 중은 끝난 일(초록) */
export const STAGE_TONE = { ready: 'blue', blocked: 'amber', fix: 'amber', applied: 'violet', signing: 'blue', managed: 'green', readonly: 'gray' } as const satisfies Record<WorkStage, string>;

const canEdit = (w: Work) => w.myRole === 'owner' || w.myRole === 'editor';
const applying = (w: Work) => !!w.application && ['submitted', 'in_review', 'needs_fix'].includes(w.application.status);
/** 승인됐고 계약서 서명 전 */
export const signing = (w: Work) => w.application?.status === 'approved' && !w.contractId && w.registration !== 'registered';

/** 한 곡에서 신청 전에 남은 일 — blocking은 신청을 막고, 아닌 것(IPI·확인 대기)은 미리 해 두면 심사가 빨라진다 */
export function workIssues(w: Work): WorkIssue[] {
  const out: WorkIssue[] = [];
  if (w.fix) out.push({ field: 'fix', text: w.fix.reason, blocking: true });
  for (const i of splitIssues(w.contributors)) {
    const confirm = /이의|확인을 기다/.test(i.text);
    out.push({ field: confirm ? 'confirm' : 'split', text: i.text, blocking: i.level === 'error', contributorId: i.contributorId });
  }
  for (const c of w.contributors) {
    if (!c.isMe && !c.ipi) out.push({ field: 'ipi', text: `${c.name || '이름 없는 참여자'}의 IPI 번호가 비어 있어요.`, blocking: false, contributorId: c.id });
    // 관리 단체에 가입했으면 IPI가 있다 — 내 번호가 비면 단체 등록 때 다른 사람으로 잡힐 수 있다
    if (c.isMe && c.society && !c.ipi) out.push({ field: 'ipi', text: '내 IPI 번호가 비어 있어요. 관리 단체 회원이면 꼭 적어 주세요.', blocking: false, contributorId: c.id });
  }
  // 내가 만들었다는 근거 (0176) — 서버도 곡마다 자료 1개 이상을 요구한다(WORK_EVIDENCE_REQUIRED)
  if (!w.attachments.length) out.push({ field: 'evidence', text: '창작 증빙 자료가 없어요. 데모·작업 파일·악보·가사·합의서 중 하나를 올려 주세요.', blocking: true });
  else {
    if (w.workType !== 'instrumental' && !w.attachments.some(a => a.kind === 'lyrics')) out.push({ field: 'evidence', text: '가사가 없어요. 노래가 있는 곡이면 가사를 붙여 주세요.', blocking: false });
    if (!w.createdOn) out.push({ field: 'evidence', text: '창작을 마친 날을 적어 두면 분쟁 때 근거가 돼요.', blocking: false });
  }
  // 공동 작가가 아직 확인하지 않았고 합의서도 없다 — 분쟁 때 가장 강한 근거는 모두가 서명한 합의서
  if (w.contributors.some(c => !c.isMe && c.confirm !== 'confirmed') && !w.attachments.some(a => a.kind === 'split_sheet')) {
    out.push({ field: 'evidence', text: '공동 작가가 아직 확인하지 않았어요. 서명한 공동 작곡 합의서가 있으면 올려 두세요.', blocking: false });
  }
  return out;
}

/** 지분을 바꿔도 검토가 끝날 때까지 지금 값으로 관리되는 곡 — 등록이 끝났거나 계약 중 (심사·보완 중인 곡은 바로 고친다) */
export const changeNeedsReview = (w: Pick<Work, 'registration' | 'contractId'>) => w.registration === 'registered' || !!w.contractId;

export function workStage(w: Work): WorkStage {
  if (!canEdit(w)) return 'readonly';
  if (w.registration === 'registered' || w.contractId) return 'managed';
  // 담당자가 승인했지만 계약서에 아직 서명하지 않았다 — 배급처럼 계약서 서명이 남은 일
  if (signing(w)) return 'signing';
  if (isManaged(w) && !applying(w)) return 'managed';
  if (applying(w) && !w.fix) return 'applied';
  // 담당자 보완 요청 — 곡 목록 배지가 '확인 필요'로 보여 앨범 머리의 '보완 요청'과 다른 말을 했다
  if (w.fix) return 'fix';
  return workIssues(w).some(i => i.blocking) ? 'blocked' : 'ready';
}

export type PrepKey = 'tracks' | PrepField;
export interface PrepGroup {
  key: PrepKey;
  label: string;
  sub: string;
  state: 'done' | 'todo' | 'wait';
  /** 이 항목에 걸린 곡 — 누르면 첫 곡의 시트를 그 칸으로 연다 */
  works: Work[];
}

/** 앨범 단위 준비 점검 — '신청 가능 3곡 · 지분 확인 필요 1곡' 한 줄과 항목별 할 일 */
export function albumReadiness(works: readonly Work[], missingTracks: number) {
  const mine = works.filter(canEdit);
  const open = mine.filter(w => workStage(w) === 'ready' || workStage(w) === 'blocked' || !!w.fix);
  const ready = mine.filter(w => workStage(w) === 'ready' && !applying(w));
  const by = (f: PrepField, blockingOnly = false) => open.filter(w => workIssues(w).some(i => i.field === f && (!blockingOnly || i.blocking)));
  const split = by('split', true);
  const disputed = open.filter(w => w.contributors.some(c => c.confirm === 'disputed'));
  const pending = open.filter(w => w.contributors.some(c => c.confirm === 'pending') && !disputed.includes(w));
  const ipi = by('ipi');
  const evidence = by('evidence', true);
  const fix = mine.filter(w => !!w.fix);
  const groups: PrepGroup[] = [
    {
      key: 'tracks', label: '곡 정보 정리', works: [],
      state: missingTracks ? 'todo' : 'done',
      sub: missingTracks ? `${missingTracks}곡의 작사·작곡 정보를 아직 정리하지 않았어요` : '모든 곡을 정리했어요',
    },
    {
      key: 'fix', label: '보완 요청', works: fix, state: fix.length ? 'todo' : 'done',
      sub: fix.length ? `${fix.map(w => `‘${w.title}’`).join(', ')}에 담당자 보완 요청이 있어요` : '보완 요청이 없어요',
    },
    {
      key: 'split', label: '지분', works: split, state: split.length ? 'todo' : 'done',
      sub: split.length ? `${split.length}곡의 지분 합계·역할을 맞춰 주세요` : '모든 곡의 지분이 100%예요',
    },
    {
      key: 'confirm', label: '참여자 확인', works: [...disputed, ...pending],
      state: disputed.length ? 'todo' : pending.length ? 'wait' : 'done',
      sub: disputed.length ? `${disputed.length}곡에 이의 제기가 있어요. 지분을 다시 나눠 주세요`
        : pending.length ? `${pending.length}곡이 참여자 확인을 기다리고 있어요` : '모든 참여자가 확인했어요',
    },
    {
      key: 'evidence', label: '창작 증빙', works: evidence, state: evidence.length ? 'todo' : 'done',
      sub: evidence.length ? `${evidence.length}곡에 창작 증빙 자료가 없어요. 데모·작업 파일·악보·가사 중 하나를 올려 주세요` : '모든 곡에 창작 증빙 자료가 있어요',
    },
    {
      key: 'ipi', label: 'IPI 번호', works: ipi, state: ipi.length ? 'todo' : 'done',
      sub: ipi.length ? `${ipi.length}곡에 IPI가 빈 참여자가 있어요 (없어도 신청할 수 있어요)` : '참여자 IPI를 모두 적었어요',
    },
  ];
  const blocked = open.filter(w => workStage(w) === 'blocked');
  const parts = [ready.length && `신청 가능 ${ready.length}곡`, split.length && `지분 확인 필요 ${split.length}곡`, evidence.length && `증빙 필요 ${evidence.length}곡`,
    disputed.length && `이의 제기 ${disputed.length}곡`, missingTracks && `정리 전 ${missingTracks}곡`].filter(Boolean) as string[];
  return { ready, blocked, groups, summary: parts.join(' · ') || (open.length ? '확인할 곡이 없어요' : '신청할 곡이 없어요') };
}

// ---------- 발매 크레딧 ↔ 곡 참여자 ----------

const nameKey = (n: string) => n.toLowerCase().replace(/\s+/g, '');
/** 발매 크레딧(작사·작곡·편곡)과 곡 참여자가 다른 곳 — missing은 크레딧엔 있는데 참여자에 없는 이름, extra는 참여자(작사·작곡·편곡)엔 있는데
 *  크레딧에 없는 이름. 관리 단체 등록은 발매 크레딧과 맞아야 해서 신청 전에 맞춰 두면 반려를 줄인다 */
export function creditGaps(w: Pick<Work, 'contributors'>, credits: { composers?: string | null; lyricists?: string | null; arrangers?: string | null; instrumental?: boolean }): { missing: string[]; extra: string[] } {
  const split = (s?: string | null) => String(s ?? '').split(/[,/·&]|\s+(?:and|feat\.?)\s+/i).map(x => x.trim()).filter(Boolean);
  const credited = [...split(credits.composers), ...(credits.instrumental ? [] : split(credits.lyricists)), ...split(credits.arrangers)];
  if (!credited.length) return { missing: [], extra: [] };
  const people = w.contributors.filter(c => c.roles.some(r => r !== 'other'));
  const have = new Set(people.map(c => nameKey(c.name)));
  const credKeys = new Set(credited.map(nameKey));
  return {
    missing: Array.from(new Set(credited.filter(n => !have.has(nameKey(n))))),
    extra: people.filter(c => !credKeys.has(nameKey(c.name))).map(c => c.name),
  };
}

// ---------- 참여자 확인 현황 ----------

export type ConfirmState = WorkContributor['confirm'];
export const CONFIRM_LABEL: Record<ConfirmState, string> = { confirmed: '확인 완료', pending: '확인 대기', disputed: '이의 제기' };
export const CONFIRM_TONE = { confirmed: 'green', pending: 'amber', disputed: 'red' } as const;

export interface PersonEntry { work: Work; contributor: WorkContributor }
export interface PersonGroup { key: string; name: string; isMe: boolean; state: ConfirmState; entries: PersonEntry[] }

/** 참여자별로 모은 확인 현황 — 한 사람이 여러 곡에 있으면 가장 급한 상태(이의 → 대기 → 완료)로 묶는다 */
export function confirmationBoard(works: readonly Work[]) {
  const m = new Map<string, PersonGroup>();
  const rank: Record<ConfirmState, number> = { disputed: 0, pending: 1, confirmed: 2 };
  for (const w of works) for (const c of w.contributors) {
    const key = c.creatorId ?? `name:${c.name.trim()}`;
    const g = m.get(key) ?? { key, name: c.name || '이름 없는 참여자', isMe: !!c.isMe, state: 'confirmed' as ConfirmState, entries: [] };
    g.entries.push({ work: w, contributor: c });
    if (rank[c.confirm] < rank[g.state]) g.state = c.confirm;
    m.set(key, g);
  }
  const people = [...m.values()].sort((a, b) => rank[a.state] - rank[b.state] || a.name.localeCompare(b.name, 'ko'));
  const count = (s: ConfirmState) => people.filter(p => p.state === s).length;
  return { people, counts: { confirmed: count('confirmed'), pending: count('pending'), disputed: count('disputed') } };
}

// ---------- 변경 전후 ----------

export interface SplitChange { key: string; name: string; kind: 'changed' | 'added' | 'removed'; lines: string[] }

const roleList = (c: WorkContributor) => c.roles.map(r => ROLE_LABEL[r]).join('·') || '역할 없음';

/** 지분·권리 정보가 어떻게 바뀌는지 — '서린 창작 지분 60% → 50%' */
export function splitDiff(before: readonly WorkContributor[], after: readonly WorkContributor[]): SplitChange[] {
  const out: SplitChange[] = [];
  const prev = new Map(before.map(c => [c.id, c]));
  for (const c of after) {
    const p = prev.get(c.id);
    const name = c.name || '이름 없는 참여자';
    if (!p) { out.push({ key: c.id, name, kind: 'added', lines: [`새 참여자 · ${roleList(c)} · 창작 지분 ${pct(c.share)}`] }); continue; }
    prev.delete(c.id);
    const lines: string[] = [];
    if (p.share !== c.share) lines.push(`창작 지분 ${pct(p.share)} → ${pct(c.share)}`);
    if (p.publisherShare !== c.publisherShare) lines.push(`관리 지분 ${pct(p.publisherShare)} → ${pct(c.publisherShare)}`);
    if (roleList(p) !== roleList(c)) lines.push(`역할 ${roleList(p)} → ${roleList(c)}`);
    if ((p.society ?? '') !== (c.society ?? '')) lines.push(`관리 단체 ${societyName(p.society) || '없음'} → ${societyName(c.society) || '없음'}`);
    if ((p.ipi ?? '') !== (c.ipi ?? '')) lines.push(`IPI ${p.ipi || '없음'} → ${c.ipi || '없음'}`);
    if (p.name !== c.name) lines.push(`이름 ${p.name} → ${c.name}`);
    if (lines.length) out.push({ key: c.id, name, kind: 'changed', lines });
  }
  for (const p of prev.values()) out.push({ key: p.id, name: p.name || '이름 없는 참여자', kind: 'removed', lines: [`빠짐 · 창작 지분 ${pct(p.share)}`] });
  return out;
}

// ---------- 참여자 정보 재사용 ----------

/** 이전 곡의 참여자 구성을 가져온다 — 역할·단체·IPI는 그대로, 지분은 곡마다 다르니 비워 두고 직접 확인해서 넣는다 */
export function reuseContributors(from: readonly WorkContributor[], keep: readonly WorkContributor[] = []): WorkContributor[] {
  const have = new Set(keep.map(c => c.creatorId ?? `name:${c.name}`));
  const add = from.filter(c => !have.has(c.creatorId ?? `name:${c.name}`)).map(c => ({
    ...c, id: uid(), share: 0, publisherShare: 0, confirm: (c.isMe ? 'confirmed' : 'pending') as ConfirmState,
  }));
  return [...keep, ...add];
}

/** 가져올 수 있는 이전 곡 — 참여자가 있고, 지금 곡이 아닌 것 (최근에 고친 곡부터) */
export const reusableWorks = (works: readonly Work[], exceptId?: string) =>
  works.filter(w => w.id !== exceptId && w.contributors.length > 0 && sumShares(w.contributors) > 0);

// ---------- 저작권료 기간·조건 ----------

export type RoyaltyPeriod = '6m' | '12m' | 'all' | 'custom';
export const ROYALTY_PERIODS: { value: RoyaltyPeriod; label: string }[] = [
  { value: '6m', label: '최근 6개월' }, { value: '12m', label: '최근 1년' }, { value: 'all', label: '전체 기간' }, { value: 'custom', label: '직접 선택' },
];
/** 음악 리포트처럼 처음에는 최근 1년 */
export const DEFAULT_PERIOD: RoyaltyPeriod = '12m';
export const isDefaultFilter = (f: RoyaltyFilter) => f.period === DEFAULT_PERIOD && !f.work && !f.source && !f.country;
export interface RoyaltyFilter { period: RoyaltyPeriod; from?: string; to?: string; work?: string; source?: string; country?: string }

const monthKey = (d: Date, back = 0) => {
  const m = d.getFullYear() * 12 + d.getMonth() - back;
  return `${Math.floor(m / 12)}-${String((m % 12) + 1).padStart(2, '0')}`;
};

/** 고른 기간의 시작·끝 달 (끝을 모르면 null) */
export function royaltyRange(f: RoyaltyFilter, now = new Date()): { from: string | null; to: string | null } {
  if (f.period === '6m') return { from: monthKey(now, 5), to: monthKey(now) };
  if (f.period === '12m') return { from: monthKey(now, 11), to: monthKey(now) };
  if (f.period === 'custom') return { from: f.from || null, to: f.to || null };
  return { from: null, to: null };
}

export function filterRoyalties(rows: readonly Royalty[], f: RoyaltyFilter, now = new Date()): Royalty[] {
  const { from, to } = royaltyRange(f, now);
  return rows.filter(r => (!from || r.period >= from) && (!to || r.period <= to)
    && (!f.work || r.workId === f.work) && (!f.source || r.source === f.source) && (!f.country || r.country === f.country));
}

/** 고른 조건을 한 줄로 — 화면 머리와 CSV 파일 이름에 같은 말 */
export function royaltyFilterLabel(f: RoyaltyFilter, names: { work?: string } = {}, now = new Date()): string {
  const { from, to } = royaltyRange(f, now);
  const period = f.period === 'custom' ? (from || to ? `${from ?? '처음'} ~ ${to ?? '지금'}` : '전체 기간')
    : ROYALTY_PERIODS.find(p => p.value === f.period)?.label ?? '';
  return [period, names.work, f.source, f.country].filter(Boolean).join(' · ');
}

/** 주소(?period=&from=&to=&work=&src=&country=)와 필터를 오간다 — 탭을 옮기거나 새로고침해도 같은 조건 */
export function filterFromParams(p: URLSearchParams): RoyaltyFilter {
  const period = (ROYALTY_PERIODS.some(x => x.value === p.get('period')) ? p.get('period') : DEFAULT_PERIOD) as RoyaltyPeriod;
  return { period, from: p.get('from') ?? undefined, to: p.get('to') ?? undefined, work: p.get('work') ?? undefined, source: p.get('src') ?? undefined, country: p.get('country') ?? undefined };
}
export function filterToParams(f: RoyaltyFilter): Record<string, string | null> {
  return {
    period: f.period === DEFAULT_PERIOD ? null : f.period, from: f.period === 'custom' ? f.from || null : null, to: f.period === 'custom' ? f.to || null : null,
    work: f.work || null, src: f.source || null, country: f.country || null,
  };
}

// ---------- 필명 ----------

const penKey = (v: string) => v.replace(/\s+/g, '').toLowerCase();
/** 내 필명인데 다른 사람으로 들어간 참여자 — 남처럼 확인을 기다리고 내 몫이 남의 몫으로 등록된다 (0176) */
export function penAsOthers(list: readonly WorkContributor[], me: Pick<Creator, 'penNames'> | null | undefined): WorkContributor[] {
  return list.filter(c => !c.isMe && !!me?.penNames?.some(p => penKey(p.name) === penKey(c.name)));
}
/** 그 참여자를 나로 — 내 줄이 이미 있으면 역할·지분을 합치고(내 이름 그대로), 없으면 그 줄을 내 줄로(필명 이름 그대로, 필명 IPI) */
export function mergeIntoMe(list: readonly WorkContributor[], contributorId: string, me: Pick<Creator, 'id' | 'name' | 'society' | 'ipi' | 'penNames'>): WorkContributor[] {
  const c = list.find(x => x.id === contributorId);
  if (!c) return [...list];
  const mine = list.find(x => x.isMe);
  const r2 = (n: number) => Math.round(n * 100) / 100;
  if (mine) {
    return list.filter(x => x.id !== c.id).map(x => (x.id === mine.id ? {
      ...x, roles: Array.from(new Set([...x.roles, ...c.roles])), share: r2(x.share + c.share), publisherShare: r2(x.publisherShare + c.publisherShare),
    } : x));
  }
  const pen = me.penNames?.find(p => penKey(p.name) === penKey(c.name));
  return list.map(x => (x.id === c.id ? {
    ...x, isMe: true, creatorId: me.id, external: false, confirm: 'confirmed' as const, penName: penKey(c.name) !== penKey(me.name),
    society: c.society || me.society || null, ipi: c.ipi || pen?.ipi || me.ipi || null,
  } : x));
}
