// 저작권 관리 API — 곡(Work) · 작가(Creator) · 저작권 관리 신청 · 계약 · 저작권료 · Open A&R.
// 화면은 `pubApi`만 쓴다. 실서버는 작업 공간 범위 경로(/api/orgs/:org/…)로 기획안 11.1의 API를 부르고,
// 체험 모드는 브라우저 저장소에 데모 데이터를 둔다 (api/client.ts와 같은 방식).
//
//   기획안 경로                            → 이 파일의 실서버 경로
//   GET/POST  /api/v1/works                → /api/orgs/:org/works
//   GET/PATCH /api/v1/works/:id            → /api/orgs/:org/works/:id
//   GET/PUT   /api/v1/works/:id/rights     → /api/orgs/:org/works/:id/rights
//   POST      /api/v1/works/:id/recordings → /api/orgs/:org/works/:id/recordings
//   GET       /api/v1/creators             → /api/orgs/:org/creators?q=
//   POST      /api/v1/publishing/applications, GET …/:id
//   GET       /api/v1/publishing/contracts
//   GET       /api/v1/works/:id/royalties  (전체는 /publishing/royalties)
//   POST      /api/v1/ar/submissions
//
// 수수료·계약 기간·연장 옵션은 화면에 고정하지 않고 /publishing/policy에서 받는다 (기획안 6.4).
import { ApiError } from './errors';
import { orgPath, req } from './http';
import { MOCK } from '../lib/mode';
import { createStore, uid } from '../lib/store';
import { stampNow } from '../lib/date';
import { clearAsyncCache } from '../lib/asyncCache';
import { statementsStore } from '../store/settlement';
import { api } from './client';
import { draftTrackOf } from '../lib/albumPrep';
import { getProfileSnapshot } from '../store/profile';
import { getAccountSnapshot } from '../store/account';

// ---------- 모양 ----------

export type RegistrationStatus = 'none' | 'preparing' | 'internal_review' | 'needs_fix' | 'ready' | 'submitted' | 'external_review' | 'registered' | 'rejected';
export type ApplicationStatus = 'draft' | 'submitted' | 'in_review' | 'needs_fix' | 'approved' | 'rejected' | 'withdrawn';
export type ContractStatus = 'pending_sign' | 'active' | 'expiring' | 'terminating' | 'ended';
export type ContributorRole = 'lyricist' | 'composer' | 'arranger' | 'other';
export type CreatorRole = 'artist' | 'composer' | 'lyricist' | 'arranger' | 'producer' | 'label';
export type RoyaltyKind = 'performance' | 'mechanical' | 'digital' | 'sync' | 'print' | 'other';
export type RoyaltyStatus = 'estimated' | 'confirmed' | 'paid';
export type ArStatus = 'submitted' | 'in_review' | 'needs_fix' | 'reviewed' | 'proposing' | 'closed';
/** 내가 이 곡에서 할 수 있는 일 — 서버가 정한다. 화면은 버튼을 숨길 뿐 보안은 서버 몫 (기획안 11.3) */
export type WorkRole = 'owner' | 'editor' | 'contributor' | 'viewer';

export interface Creator {
  id: string;
  name: string;
  /** 법적 이름 — 권한이 있는 화면(본인·소유자)에서만 서버가 내려 준다 */
  legalName?: string | null;
  roles: CreatorRole[];
  org?: string | null;
  society?: string | null;
  ipi?: string | null;
  country?: string | null;
  /** AUDENIQ 계정과 연결됐는지 — 외부 참여자는 false */
  linked: boolean;
  isMe?: boolean;
  workCount?: number;
}

export interface WorkContributor {
  id: string;
  creatorId: string | null;
  name: string;
  roles: ContributorRole[];
  society?: string | null;
  ipi?: string | null;
  /** 창작 지분(%) */
  share: number;
  /** 그중 퍼블리셔가 관리하는 지분(%) — 창작 지분과 따로 저장 */
  publisherShare: number;
  confirm: 'confirmed' | 'pending' | 'disputed';
  external: boolean;
  isMe?: boolean;
}

export interface WorkAttachment { id: string; kind: 'demo' | 'lyrics' | 'score' | 'proof'; name: string; size?: number; at: string }
export interface WorkRecording { id: string; releaseId: string; trackId: string; title: string; releaseTitle: string; version?: string | null; isrc?: string | null; status?: string }
export interface WorkEvent { at: string; text: string; who?: string; kind?: 'edit' | 'rights' | 'apply' | 'review' | 'register' | 'file' | 'link' }
export interface RegistrationStep { status: RegistrationStatus; at: string; note?: string; society?: string }

export interface WorkFix { reason: string; fields: string[]; requestedAt: string; documents?: string[] }

export interface Work {
  id: string;
  title: string;
  altTitles: string[];
  workType: string;
  language: string;
  genre: string;
  description: string;
  visibility: 'private' | 'shared';
  /** ISWC가 없으면 internalCode로 관리 */
  iswc: string | null;
  internalCode: string;
  hasPriorPublisher: boolean;
  priorPublisher?: string | null;
  contributors: WorkContributor[];
  attachments: WorkAttachment[];
  recordings: WorkRecording[];
  registration: RegistrationStatus;
  registrationLog: RegistrationStep[];
  application: { id: string; number: string; status: ApplicationStatus } | null;
  contractId: string | null;
  fix: WorkFix | null;
  myRole: WorkRole;
  activity: WorkEvent[];
  rowVersion: number;
  createdAt: string;
  updatedAt: string;
}

/** 발매 신청에서 곡 크레딧으로 곡을 만들 때 — enroll이면 저작권 관리 신청 준비까지 (기획안 4.2, 15) */
export interface PrepareResult { created: number; linked: number; skipped: number }

export type WorkInput = Pick<Work, 'title' | 'altTitles' | 'workType' | 'language' | 'genre' | 'description' | 'visibility' | 'iswc' | 'hasPriorPublisher' | 'priorPublisher' | 'contributors' | 'attachments'> & {
  /** 발매 곡에서 만들 때 바로 연결할 녹음물 */
  recording?: Omit<WorkRecording, 'id'> | null;
};

export interface PublishingPolicy {
  commissionRate: number;
  initialTermMonths: number;
  renewalOptions: number[];
  territories: { code: string; label: string }[];
  rights: { key: string; label: string; desc: string }[];
  version: string;
}

export interface PublishingApplication {
  id: string;
  number: string;
  status: ApplicationStatus;
  workIds: string[];
  workTitles: string[];
  territories: string[];
  rights: string[];
  submittedAt: string | null;
  contractId: string | null;
  fixReason?: string | null;
  history: { at: string; text: string }[];
}

export interface ApplicationInput {
  workIds: string[];
  territories: string[];
  rights: string[];
  policyVersion: string;
  agreed: string[];
  signature: { name: string; method: 'typed' | 'drawn'; data?: string };
  /** 같은 신청을 두 번 보내도 한 번만 접수되게 (서버 멱등 키) */
  idempotencyKey: string;
}

export interface PublishingContract {
  id: string;
  number: string;
  status: ContractStatus;
  startDate: string;
  endDate: string;
  commissionRate: number;
  termMonths: number;
  works: { id: string; title: string }[];
  territories: string[];
  rights: string[];
  renewal: { months: number; requestedAt: string } | null;
  terminationRequestedAt: string | null;
  signedAt: string | null;
}

export interface Royalty {
  id: string;
  workId: string;
  workTitle: string;
  kind: RoyaltyKind;
  source: string;
  period: string;
  country: string;
  currency: string;
  collected: number;
  /** 관리 지분(%) */
  share: number;
  fee: number;
  net: number;
  status: RoyaltyStatus;
  /** 확정돼 정산 잔액에 적립된 날 (예상은 null) */
  postedAt?: string | null;
}

export interface PublishingTask { id: string; kind: 'fix' | 'split' | 'sign' | 'renew' | 'ar'; title: string; sub: string; to: string }

export interface PublishingSummary {
  enrolled: boolean;
  works: { total: number; managed: number; inProgress: number; unregistered: number };
  tasks: PublishingTask[];
  revenue: {
    distribution: { confirmed: number; estimated: number };
    publishing: { confirmed: number; estimated: number };
    payable: number;
  };
  /** 아직 곡이 없는 발매 곡 — '이 곡의 저작권도 관리할까요?' */
  suggestion: { releaseId: string; trackId: string; title: string; releaseTitle: string } | null;
}

export interface ArSubmission {
  id: string;
  workId: string;
  workTitle: string;
  genre: string;
  mood: string[];
  description: string;
  references: string;
  scope: 'internal' | 'partners';
  status: ArStatus;
  submittedAt: string;
  fixReason?: string | null;
  history: { at: string; text: string }[];
}
export type ArInput = Pick<ArSubmission, 'workId' | 'genre' | 'mood' | 'description' | 'references' | 'scope'> & { demoName?: string; lyrics?: string; consent: boolean; idempotencyKey: string };

// ---------- 체험 모드 데이터 ----------

const ME = 'cr_me';
const POLICY: PublishingPolicy = {
  commissionRate: 10,
  initialTermMonths: 12,
  renewalOptions: [6, 12, 36],
  territories: [
    { code: 'KR', label: '대한민국' }, { code: 'JP', label: '일본' }, { code: 'US', label: '미국' }, { code: 'WORLD', label: '전 세계(그 밖의 국가)' },
  ],
  rights: [
    { key: 'performance', label: '공연권', desc: '방송·공연·매장 음악에서 생기는 사용료' },
    { key: 'mechanical', label: '복제권', desc: '음반·다운로드 등 복제에서 생기는 사용료' },
    { key: 'digital', label: '전송권', desc: '스트리밍 서비스에서 생기는 사용료' },
    { key: 'sync', label: '싱크 라이선스', desc: '영상·광고·게임에 쓰일 때의 이용 허락' },
  ],
  version: '2026-10',
};

const me = (share: number, roles: ContributorRole[], publisherShare = 0, confirm: WorkContributor['confirm'] = 'confirmed'): WorkContributor =>
  ({ id: uid(), creatorId: ME, name: '서린', roles, society: 'KOMCA', ipi: '00712345678', share, publisherShare, confirm, external: false, isMe: true });

function seedWorks(): Work[] {
  const base = { altTitles: [], language: 'ko', genre: 'Indie Pop', description: '', visibility: 'private' as const, hasPriorPublisher: false, priorPublisher: null, attachments: [], fix: null, rowVersion: 1 };
  return [
    {
      ...base, id: 'w1', title: '첫 번째 싱글', workType: 'original', iswc: 'T-345.246.800-1', internalCode: 'AQW-000101',
      description: '하루의 끝에서 시작되는 첫 번째 이야기.',
      contributors: [me(100, ['lyricist', 'composer'], 100)],
      attachments: [{ id: 'f1', kind: 'lyrics', name: '첫번째싱글_가사.txt', size: 1820, at: '2026-03-02 10:00' }],
      recordings: [
        { id: 'rc1', releaseId: 'r1', trackId: 't1', title: '첫 번째 싱글', releaseTitle: '첫 번째 싱글', isrc: 'KRA262600001', status: 'live' },
        { id: 'rc2', releaseId: 'r1', trackId: 't2', title: '첫 번째 싱글', version: 'Inst.', releaseTitle: '첫 번째 싱글', isrc: 'KRA262600002', status: 'live' },
      ],
      registration: 'registered',
      registrationLog: [
        { status: 'internal_review', at: '2026-03-02 10:12' }, { status: 'ready', at: '2026-03-04 15:40', note: '권리 정보 확인 완료' },
        { status: 'submitted', at: '2026-03-05 09:00', society: 'KOMCA' }, { status: 'registered', at: '2026-03-19 11:20', society: 'KOMCA', note: '곡 코드 발급' },
      ],
      application: { id: 'a1', number: 'PUB-2026-0001', status: 'approved' }, contractId: 'c1', myRole: 'owner',
      activity: [
        { at: '2026-03-19 11:20', text: 'KOMCA 등록이 끝났어요.', kind: 'register' },
        { at: '2026-03-03 09:00', text: '저작권 관리 계약에 서명했어요.', kind: 'apply', who: '서린' },
        { at: '2026-03-02 10:00', text: '곡을 만들었어요.', kind: 'edit', who: '서린' },
      ],
      createdAt: '2026-03-02 10:00', updatedAt: '2026-03-19 11:20',
    },
    {
      ...base, id: 'w2', title: '파도', workType: 'original', iswc: null, internalCode: 'AQW-000102',
      contributors: [
        me(60, ['lyricist', 'composer'], 60),
        { id: uid(), creatorId: null, name: '김하늘', roles: ['composer'], society: 'KOMCA', ipi: null, share: 40, publisherShare: 0, confirm: 'pending', external: true },
      ],
      recordings: [{ id: 'rc3', releaseId: 'r2', trackId: 't3', title: '파도', releaseTitle: '여름 EP', isrc: 'KRA262600003', status: 'review' }],
      registration: 'needs_fix',
      registrationLog: [{ status: 'internal_review', at: '2026-10-20 13:00' }, { status: 'needs_fix', at: '2026-10-24 17:10', note: '공동 작곡가 확인 필요' }],
      application: { id: 'a2', number: 'PUB-2026-0014', status: 'needs_fix' }, contractId: null, myRole: 'owner',
      fix: {
        reason: '공동 작곡가 김하늘 님의 IPI 번호가 비어 있고, 지분 확인을 아직 받지 못했어요. IPI 번호를 입력하고 공동 작곡 합의서를 올려 주세요.',
        fields: ['contributors.ipi', 'attachments.proof'], requestedAt: '2026-10-24 17:10', documents: ['공동 작곡 합의서'],
      },
      activity: [
        { at: '2026-10-24 17:10', text: '담당자가 보완을 요청했어요.', kind: 'review', who: 'AUDENIQ' },
        { at: '2026-10-20 13:00', text: '저작권 관리를 신청했어요.', kind: 'apply', who: '서린' },
        { at: '2026-10-19 21:40', text: '지분을 60 : 40으로 나누었어요.', kind: 'rights', who: '서린' },
      ],
      createdAt: '2026-10-19 21:30', updatedAt: '2026-10-24 17:10',
    },
    {
      ...base, id: 'w3', title: '노을', workType: 'original', iswc: null, internalCode: 'AQW-000103',
      contributors: [me(100, ['lyricist', 'composer'], 100)],
      recordings: [{ id: 'rc4', releaseId: 'r2', trackId: 't5', title: '노을', releaseTitle: '여름 EP', isrc: null, status: 'review' }],
      registration: 'internal_review',
      registrationLog: [{ status: 'internal_review', at: '2026-10-27 10:00' }],
      application: { id: 'a3', number: 'PUB-2026-0019', status: 'in_review' }, contractId: null, myRole: 'owner',
      activity: [{ at: '2026-10-27 10:00', text: '저작권 관리를 신청했어요.', kind: 'apply', who: '서린' }],
      createdAt: '2026-10-26 22:00', updatedAt: '2026-10-27 10:00',
    },
    {
      ...base, id: 'w4', title: '새벽 네 시', workType: 'original', iswc: null, internalCode: 'AQW-000104', genre: 'R&B',
      description: '아직 녹음 전인 데모. 피아노 한 대로 시작해서 후렴에서 드럼이 들어와요.',
      contributors: [
        me(50, ['composer']),
        { id: uid(), creatorId: 'cr_2', name: '박도윤', roles: ['lyricist'], society: 'KOMCA', ipi: '00798765432', share: 40, publisherShare: 0, confirm: 'confirmed', external: false },
      ],
      attachments: [{ id: 'f2', kind: 'demo', name: 'saebyeok_demo_v2.mp3', size: 4_120_000, at: '2026-10-28 02:14' }],
      recordings: [], registration: 'none', registrationLog: [], application: null, contractId: null, myRole: 'owner',
      activity: [{ at: '2026-10-28 02:14', text: '데모 음원을 올렸어요.', kind: 'file', who: '서린' }],
      createdAt: '2026-10-28 02:10', updatedAt: '2026-10-28 02:14',
    },
    {
      ...base, id: 'w5', title: '겨울 편지', workType: 'original', iswc: 'T-345.246.811-4', internalCode: 'AQW-000088', genre: 'Ballad',
      contributors: [
        { id: uid(), creatorId: 'cr_3', name: '이온', roles: ['composer'], society: 'KOMCA', ipi: '00755511122', share: 70, publisherShare: 70, confirm: 'confirmed', external: false },
        me(30, ['lyricist'], 30),
      ],
      recordings: [], registration: 'registered',
      registrationLog: [{ status: 'registered', at: '2025-12-10 10:00', society: 'KOMCA' }],
      application: { id: 'a0', number: 'PUB-2025-0042', status: 'approved' }, contractId: 'c0', myRole: 'contributor',
      activity: [{ at: '2025-12-10 10:00', text: 'KOMCA 등록이 끝났어요.', kind: 'register' }],
      createdAt: '2025-11-30 12:00', updatedAt: '2025-12-10 10:00',
    },
  ];
}

const CREATORS: Creator[] = [
  { id: ME, name: '서린', legalName: '김서린', roles: ['artist', 'composer', 'lyricist'], society: 'KOMCA', ipi: '00712345678', country: 'KR', linked: true, isMe: true },
  { id: 'cr_2', name: '박도윤', roles: ['lyricist'], society: 'KOMCA', ipi: '00798765432', country: 'KR', linked: true, workCount: 12 },
  { id: 'cr_3', name: '이온', roles: ['composer', 'producer'], society: 'KOMCA', ipi: '00755511122', country: 'KR', linked: true, workCount: 31 },
  { id: 'cr_4', name: 'Mina Cho', roles: ['composer', 'arranger'], society: 'ASCAP', ipi: '00633322211', country: 'US', linked: true, workCount: 7 },
  { id: 'cr_5', name: '하람', roles: ['lyricist', 'artist'], society: null, ipi: null, country: 'KR', linked: true, workCount: 2 },
];

function seedContracts(): PublishingContract[] {
  return [{
    id: 'c1', number: 'AQP-2026-0003', status: 'expiring', startDate: '2026-03-03', endDate: '2026-11-28', commissionRate: 10, termMonths: 12,
    works: [{ id: 'w1', title: '첫 번째 싱글' }], territories: ['KR', 'JP', 'US', 'WORLD'], rights: ['performance', 'mechanical', 'digital'],
    renewal: null, terminationRequestedAt: null, signedAt: '2026-03-03 09:00',
  }];
}

function seedApplications(): PublishingApplication[] {
  return [
    { id: 'a1', number: 'PUB-2026-0001', status: 'approved', workIds: ['w1'], workTitles: ['첫 번째 싱글'], territories: ['KR', 'JP', 'US', 'WORLD'], rights: ['performance', 'mechanical', 'digital'], submittedAt: '2026-03-02 10:12', contractId: 'c1', history: [{ at: '2026-03-02 10:12', text: '제출 완료' }, { at: '2026-03-03 08:40', text: '승인' }] },
    { id: 'a2', number: 'PUB-2026-0014', status: 'needs_fix', workIds: ['w2'], workTitles: ['파도'], territories: ['KR'], rights: ['performance', 'mechanical', 'digital'], submittedAt: '2026-10-20 13:00', contractId: null, fixReason: '공동 작곡가 확인 필요', history: [{ at: '2026-10-20 13:00', text: '제출 완료' }, { at: '2026-10-21 10:00', text: '심사 시작' }, { at: '2026-10-24 17:10', text: '보완 요청' }] },
    { id: 'a3', number: 'PUB-2026-0019', status: 'in_review', workIds: ['w3'], workTitles: ['노을'], territories: ['KR', 'WORLD'], rights: ['performance', 'mechanical', 'digital'], submittedAt: '2026-10-27 10:00', contractId: null, history: [{ at: '2026-10-27 10:00', text: '제출 완료' }, { at: '2026-10-28 09:30', text: '심사 시작' }] },
  ];
}

function seedRoyalties(): Royalty[] {
  const row = (id: string, workId: string, workTitle: string, kind: RoyaltyKind, source: string, period: string, country: string, collected: number, share: number, status: RoyaltyStatus): Royalty => {
    const managed = Math.round(collected * share / 100);
    const fee = Math.round(managed * 0.1);
    // 사용된 달로부터 석 달쯤 뒤 관리 단체가 확정해 적립된다
    const [y, m] = period.split('-').map(Number);
    const postedAt = status === 'estimated' ? null : `${m + 3 > 12 ? y + 1 : y}-${String(((m + 2) % 12) + 1).padStart(2, '0')}-20`;
    return { id, workId, workTitle, kind, source, period, country, currency: 'KRW', collected, share, fee, net: managed - fee, status, postedAt };
  };
  return [
    row('y1', 'w1', '첫 번째 싱글', 'digital', 'KOMCA', '2026-06', 'KR', 182_400, 100, 'paid'),
    row('y2', 'w1', '첫 번째 싱글', 'performance', 'KOMCA', '2026-06', 'KR', 64_200, 100, 'paid'),
    row('y3', 'w1', '첫 번째 싱글', 'digital', 'KOMCA', '2026-07', 'KR', 210_900, 100, 'confirmed'),
    row('y4', 'w1', '첫 번째 싱글', 'mechanical', 'JASRAC', '2026-07', 'JP', 38_500, 100, 'confirmed'),
    row('y5', 'w1', '첫 번째 싱글', 'digital', 'KOMCA', '2026-08', 'KR', 198_000, 100, 'estimated'),
    row('y6', 'w5', '겨울 편지', 'performance', 'KOMCA', '2026-07', 'KR', 420_000, 30, 'confirmed'),
    row('y7', 'w5', '겨울 편지', 'digital', 'KOMCA', '2026-08', 'KR', 388_000, 30, 'estimated'),
  ];
}

function seedAr(): ArSubmission[] {
  return [{
    id: 'ar1', workId: 'w4', workTitle: '새벽 네 시', genre: 'R&B', mood: ['신비로운', '잔잔한'], description: '여성 보컬 발라드 R&B로 피칭하고 싶어요.', references: 'https://example.com/ref',
    scope: 'partners', status: 'in_review', submittedAt: '2026-10-28 03:00',
    history: [{ at: '2026-10-28 03:00', text: '제출 완료' }, { at: '2026-10-29 11:00', text: '심사 시작' }],
  }];
}

interface PubDb { works: Work[]; contracts: PublishingContract[]; applications: PublishingApplication[]; royalties: Royalty[]; ar: ArSubmission[]; me: Creator; sentKeys: string[] }
const seed = (): PubDb => ({ works: seedWorks(), contracts: seedContracts(), applications: seedApplications(), royalties: seedRoyalties(), ar: seedAr(), me: CREATORS[0], sentKeys: [] });
const db = /* @__PURE__ */ createStore<PubDb>(seed, {
  persist: MOCK ? 'publishing.v2' : undefined,
  revive: (raw, fb) => (raw && typeof raw === 'object' && Array.isArray((raw as PubDb).works) ? { ...fb, ...(raw as PubDb) } : fb),
});

/** 체험 모드: 확정된 저작권료를 정산 원장(적립 내역)에 '저작권 관리' 적립으로 한 번씩 올린다 — 실서버는 서버 원장이 같은 일을 한다.
 *  같은 저작권료는 royaltyId로 한 번만 올라가 배급과 저작권 관리에 겹쳐 세지 않는다 (기획안 8.4) */
function postRoyalties() {
  const posted = new Set(statementsStore.get().map(s => s.royaltyId).filter(Boolean));
  const add = db.get().royalties.filter(r => r.status !== 'estimated' && r.postedAt && !posted.has(r.id)).map(r => ({
    id: `roy-${r.id}`, period: r.period, platform: r.source, amount: r.net, note: `${r.workTitle} 저작권료`, created: r.postedAt!,
    revenueType: 'publishing' as const, royaltyId: r.id,
  }));
  if (add.length) statementsStore.set(ss => [...ss, ...add]);
}
if (MOCK && typeof window !== 'undefined') postRoyalties();

/** 작가의 활동명·국가·법적 이름은 계정 정보(아티스트 활동명·활동 국가·본인 확인 실명)와 같은 값 */
function withAccount(me: Creator): Creator {
  const p = getProfileSnapshot();
  const a = getAccountSnapshot();
  return { ...structuredClone(me), name: p.name.trim() || me.name, country: p.country || me.country, legalName: a.realName || me.legalName };
}

const wait = (ms = 120) => new Promise(r => setTimeout(r, ms));
const notFound = () => new ApiError('곡을 찾을 수 없어요. 삭제됐거나 볼 수 있는 권한이 없어요.', 404, 'NOT_FOUND');

function mutate<T>(fn: (d: PubDb) => T): T {
  const next = structuredClone(db.get());
  const out = fn(next);
  db.set(next);
  clearAsyncCache();
  return out;
}

function nextCode(d: PubDb) {
  const n = d.works.reduce((m, w) => Math.max(m, Number(w.internalCode.replace(/\D/g, '')) || 0), 100) + 1;
  return `AQW-${String(n).padStart(6, '0')}`;
}

function mockSummary(d: PubDb): PublishingSummary {
  const managed = d.works.filter(w => ['ready', 'submitted', 'external_review', 'registered'].includes(w.registration)).length;
  const inProgress = d.works.filter(w => ['preparing', 'internal_review', 'needs_fix'].includes(w.registration)).length;
  const tasks: PublishingTask[] = [];
  const preparing = d.works.filter(w => w.registration === 'preparing' && !w.application);
  if (preparing.length) {
    const rel = preparing[0].recordings[0];
    tasks.push({
      id: 'prepare', kind: 'split', title: `발매 곡 ${preparing.length}곡 지분 확인하고 저작권 관리 신청`,
      sub: '발매 신청에서 고른 곡이에요. 크레딧으로 나눈 지분이 맞는지 확인하면 바로 신청돼요.',
      to: rel ? `/publishing/apply?release=${rel.releaseId}` : '/publishing/apply',
    });
  }
  for (const w of d.works) {
    if (w.fix) tasks.push({ id: `fix-${w.id}`, kind: 'fix', title: `‘${w.title}’ 권리 정보 보완 요청`, sub: w.fix.reason, to: `/works/${w.id}?tab=register` });
    const pending = w.contributors.filter(c => c.confirm === 'pending' && !c.isMe);
    const mine = w.contributors.find(c => c.isMe && c.confirm === 'pending');
    if (mine) tasks.push({ id: `split-me-${w.id}`, kind: 'split', title: `‘${w.title}’ 지분 확인 요청`, sub: `내 지분 ${mine.share}%를 확인해 주세요.`, to: `/works/${w.id}?tab=rights` });
    else if (pending.length && !w.fix) tasks.push({ id: `split-${w.id}`, kind: 'split', title: `‘${w.title}’ 지분 확인을 기다리는 중`, sub: `${pending.map(p => p.name).join(', ')} 님의 확인이 필요해요.`, to: `/works/${w.id}?tab=rights` });
  }
  for (const c of d.contracts) {
    if (c.status === 'pending_sign') tasks.push({ id: `sign-${c.id}`, kind: 'sign', title: '저작권 관리 계약 서명 요청', sub: c.works.map(w => w.title).join(', '), to: `/contracts?pub=${c.id}` });
    // 연장은 계약서(서류) 화면이 아니라 곡 화면의 계약 관리에서
    if (c.status === 'expiring' && !c.renewal && c.works[0]) tasks.push({ id: `renew-${c.id}`, kind: 'renew', title: '저작권 관리 계약 갱신 요청', sub: `${c.endDate}에 끝나요. 연장하면 관리가 끊기지 않아요.`, to: `/works/${c.works[0].id}?tab=register` });
  }
  for (const a of d.ar) if (a.status === 'needs_fix') tasks.push({ id: `ar-${a.id}`, kind: 'ar', title: `A&R ‘${a.workTitle}’ 보완 요청`, sub: a.fixReason ?? '', to: '/publishing?tab=ar' });
  const pub = d.royalties.reduce((acc, r) => {
    if (r.status === 'estimated') acc.estimated += r.net; else acc.confirmed += r.net;
    return acc;
  }, { confirmed: 0, estimated: 0 });
  const dist = statementsStore.get().filter(s => s.revenueType !== 'publishing').reduce((n, s) => n + s.amount, 0);
  const linked = new Set(d.works.flatMap(w => w.recordings.map(r => r.trackId)));
  // 데모 발매 곡 중 아직 곡이 없는 곡 (api/mock.ts SEED와 같은 id)
  const candidates = [{ releaseId: 'r2', trackId: 't4', title: '한낮', releaseTitle: '여름 EP' }, { releaseId: 'r2', trackId: 't6', title: '밤바다', releaseTitle: '여름 EP' }];
  return {
    enrolled: d.contracts.length > 0 || d.applications.length > 0,
    works: { total: d.works.length, managed, inProgress, unregistered: d.works.filter(w => w.registration === 'none').length },
    tasks,
    revenue: {
      distribution: { confirmed: dist, estimated: 0 },
      publishing: pub,
      payable: d.royalties.filter(r => r.status === 'confirmed').reduce((n, r) => n + r.net, 0),
    },
    suggestion: candidates.find(c => !linked.has(c.trackId)) ?? null,
  };
}

const mockApi = {
  async policy() { await wait(60); return POLICY; },
  async summary() { await wait(); return mockSummary(db.get()); },
  async listWorks() { await wait(); return structuredClone(db.get().works).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)); },
  async getWork(id: string) {
    await wait(80);
    const w = db.get().works.find(x => x.id === id);
    if (!w) throw notFound();
    return structuredClone(w);
  },
  async workByTrack(trackId: string) { await wait(60); return structuredClone(db.get().works.find(w => w.recordings.some(r => r.trackId === trackId)) ?? null); },
  async createWork(input: WorkInput) {
    await wait(260);
    return mutate(d => {
      const now = stampNow();
      const { recording, ...rest } = input;
      const w: Work = {
        ...rest, id: `w_${uid()}`, internalCode: nextCode(d), recordings: recording ? [{ ...recording, id: uid() }] : [],
        registration: 'none', registrationLog: [], application: null, contractId: null, fix: null, myRole: 'owner', rowVersion: 1,
        activity: [{ at: now, text: recording ? `‘${recording.releaseTitle}’ 발매 곡에서 곡을 만들었어요.` : '곡을 만들었어요.', kind: 'edit', who: d.me.name }],
        createdAt: now, updatedAt: now,
      };
      d.works.unshift(w);
      return structuredClone(w);
    });
  },
  async updateWork(id: string, patch: Partial<WorkInput>, rowVersion: number) {
    await wait(220);
    return mutate(d => {
      const w = d.works.find(x => x.id === id);
      if (!w) throw notFound();
      if (w.myRole !== 'owner' && w.myRole !== 'editor') throw new ApiError('이 곡을 고칠 권한이 없어요.', 403, 'FORBIDDEN');
      if (w.rowVersion !== rowVersion) throw new ApiError('다른 곳에서 먼저 고쳤어요. 새로 불러온 뒤 다시 저장해 주세요.', 409, 'CONFLICT');
      const before = JSON.stringify(w.contributors.map(c => [c.name, c.share, c.publisherShare]));
      const { recording: _r, ...rest } = patch;
      Object.assign(w, rest);
      const now = stampNow();
      const rightsChanged = patch.contributors && before !== JSON.stringify(w.contributors.map(c => [c.name, c.share, c.publisherShare]));
      w.activity.unshift({ at: now, text: rightsChanged ? `지분을 ${w.contributors.map(c => `${c.name} ${c.share}%`).join(' · ')}로 바꿨어요.` : '곡 정보를 고쳤어요.', kind: rightsChanged ? 'rights' : 'edit', who: d.me.name });
      w.rowVersion += 1;
      w.updatedAt = now;
      return structuredClone(w);
    });
  },
  async deleteWork(id: string) {
    await wait(200);
    mutate(d => {
      const w = d.works.find(x => x.id === id);
      if (!w) throw notFound();
      if (w.application && w.application.status !== 'withdrawn') throw new ApiError('저작권 관리를 신청한 곡은 지울 수 없어요. 문의로 요청해 주세요.', 409, 'WORK_IN_USE');
      d.works = d.works.filter(x => x.id !== id);
    });
  },
  async linkRecording(workId: string, rec: Omit<WorkRecording, 'id'>) {
    await wait(180);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      if (!w) throw notFound();
      if (!w.recordings.some(r => r.trackId === rec.trackId)) {
        w.recordings.push({ ...rec, id: uid() });
        w.activity.unshift({ at: stampNow(), text: `‘${rec.releaseTitle}’의 ‘${rec.title}’ 녹음을 연결했어요.`, kind: 'link', who: d.me.name });
        w.rowVersion += 1;
        w.updatedAt = stampNow();
      }
      return structuredClone(w);
    });
  },
  async unlinkRecording(workId: string, recordingId: string) {
    await wait(160);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      if (!w) throw notFound();
      const r = w.recordings.find(x => x.id === recordingId);
      w.recordings = w.recordings.filter(x => x.id !== recordingId);
      if (r) w.activity.unshift({ at: stampNow(), text: `‘${r.title}’ 녹음 연결을 풀었어요.`, kind: 'link', who: d.me.name });
      w.rowVersion += 1;
      return structuredClone(w);
    });
  },
  async requestSplitConfirm(workId: string, contributorId: string) {
    await wait(200);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      const c = w?.contributors.find(x => x.id === contributorId);
      if (!w || !c) throw notFound();
      c.confirm = 'pending';
      w.activity.unshift({ at: stampNow(), text: `${c.name} 님에게 지분 확인을 요청했어요.`, kind: 'rights', who: d.me.name });
      return structuredClone(w);
    });
  },
  /** 체험 모드: 상대가 확인한 것처럼 */
  async simulateConfirm(workId: string, contributorId: string) {
    await wait(120);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      const c = w?.contributors.find(x => x.id === contributorId);
      if (!w || !c) throw notFound();
      c.confirm = 'confirmed';
      w.activity.unshift({ at: stampNow(), text: `${c.name} 님이 지분 ${c.share}%를 확인했어요.`, kind: 'rights', who: c.name });
      return structuredClone(w);
    });
  },
  /** 내 지분 확인 — 맞으면 확인, 다르면 변경 요청(이의) */
  async respondSplit(workId: string, agree: boolean) {
    await wait(220);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      const c = w?.contributors.find(x => x.isMe);
      if (!w || !c) throw notFound();
      c.confirm = agree ? 'confirmed' : 'disputed';
      w.activity.unshift({ at: stampNow(), text: agree ? `내 지분 ${c.share}%를 확인했어요.` : '지분 변경을 요청했어요.', kind: 'rights', who: d.me.name });
      return structuredClone(w);
    });
  },
  async searchCreators(q: string) {
    await wait(140);
    const k = q.trim().toLowerCase();
    if (!k) return [];
    const me = withAccount(db.get().me);
    return [me, ...CREATORS.filter(c => !c.isMe)].filter(c => c.name.toLowerCase().includes(k) || (c.ipi ?? '').includes(k)).map(c => ({ ...c, legalName: c.isMe ? c.legalName : null }));
  },
  async myCreator() { await wait(80); return withAccount(db.get().me); },
  /** 작가 정보 중 역할·관리 단체·IPI·소속만 저장한다 — 활동명·국가·실명은 계정 정보의 값을 쓴다 */
  async saveMyCreator(c: Creator) {
    await wait(220);
    return mutate(d => { d.me = { ...d.me, roles: c.roles, society: c.society, ipi: c.ipi, org: c.org }; return withAccount(d.me); });
  },

  /** 발매의 곡마다 곡을 만들어 녹음을 연결한다 — 이미 연결된 곡·커버곡은 건너뛴다.
   *  크레딧의 작사·작곡·편곡을 참여자로 넣고 지분은 균등하게 미리 나눠 두되, 신청 전에 사용자가 확인한다 */
  async prepareWorksFromRelease(releaseId: string, enroll: boolean, skipTitles: string[] = []): Promise<PrepareResult> {
    const rel = await api.getRelease(releaseId);
    const me = withAccount(db.get().me);
    return mutate(d => {
      const out: PrepareResult = { created: 0, linked: 0, skipped: 0 };
      const now = stampNow();
      for (const t of rel.tracks) {
        const has = d.works.find(w => w.recordings.some(r => r.trackId === t.id));
        if (has) {
          if (enroll && has.registration === 'none' && !has.application && has.myRole !== 'contributor') { has.registration = 'preparing'; out.linked++; }
          continue;
        }
        if (!t.title.trim() || skipTitles.includes(t.title)) { out.skipped++; continue; }
        const dt = draftTrackOf(t, rel.draft);
        const people = new Map<string, WorkContributor>();
        const add = (names: string[], role: ContributorRole) => {
          for (const n of names) {
            const cur = people.get(n);
            if (cur) { if (!cur.roles.includes(role)) cur.roles.push(role); continue; }
            const isMe = n === me.name || n === rel.artist;
            people.set(n, { id: uid(), creatorId: isMe ? ME : null, name: isMe ? me.name : n, roles: [role], society: isMe ? me.society : null, ipi: isMe ? me.ipi : null, share: 0, publisherShare: 0, confirm: isMe ? 'confirmed' : 'pending', external: !isMe, isMe });
          }
        };
        const names = (v: string | null | undefined) => String(v ?? '').split(/[,/·&]/).map(x => x.trim()).filter(Boolean);
        add(names(t.composers ?? dt?.composers), 'composer');
        if (!dt?.instrumental) add(names(t.lyricists ?? dt?.lyricists), 'lyricist');
        add(names(dt?.arrangers), 'arranger');
        if (!people.size) people.set(me.name, { id: uid(), creatorId: ME, name: me.name, roles: ['composer'], society: me.society, ipi: me.ipi, share: 0, publisherShare: 0, confirm: 'confirmed', external: false, isMe: true });
        const list = [...people.values()];
        const each = Math.floor((100 / list.length) * 100) / 100;
        list.forEach((c, i) => { c.share = i === list.length - 1 ? Math.round((100 - each * (list.length - 1)) * 100) / 100 : each; c.publisherShare = enroll && c.isMe ? c.share : 0; });
        d.works.unshift({
          id: `w_${uid()}`, title: t.title, altTitles: [], workType: dt?.instrumental ? 'instrumental' : 'original', language: rel.draft?.language ?? 'ko',
          genre: rel.draft?.genre ?? '', description: '', visibility: 'private', iswc: null, internalCode: nextCode(d), hasPriorPublisher: false, priorPublisher: null,
          contributors: list, attachments: [], recordings: [{ id: uid(), releaseId: rel.id, trackId: t.id, title: t.title, releaseTitle: rel.title, version: t.version, isrc: t.isrc, status: rel.status }],
          registration: enroll ? 'preparing' : 'none', registrationLog: enroll ? [{ status: 'preparing', at: now, note: '발매 신청에서 함께 신청' }] : [],
          application: null, contractId: null, fix: null, myRole: 'owner', rowVersion: 1,
          activity: [{ at: now, text: `‘${rel.title}’ 발매 신청에서 크레딧으로 만들었어요. 지분이 맞는지 확인해 주세요.`, kind: 'link', who: d.me.name }],
          createdAt: now, updatedAt: now,
        });
        out.created++;
      }
      return out;
    });
  },
  async listApplications() { await wait(); return structuredClone(db.get().applications); },
  async submitApplication(input: ApplicationInput) {
    await wait(700);
    return mutate(d => {
      // 같은 멱등 키는 처음 만든 신청을 그대로 돌려준다
      const dup = d.applications.find(a => a.id === `a_${input.idempotencyKey}`);
      if (dup) return structuredClone(dup);
      const works = input.workIds.map(id => d.works.find(w => w.id === id)).filter(Boolean) as Work[];
      if (!works.length) throw new ApiError('신청할 곡을 골라 주세요.', 400, 'VALIDATION');
      const busy = works.find(w => w.application && ['submitted', 'in_review', 'needs_fix', 'approved'].includes(w.application.status));
      if (busy) throw new ApiError(`‘${busy.title}’은(는) 이미 신청했어요. 진행 상황은 곡 화면에서 볼 수 있어요.`, 409, 'DUPLICATE_APPLICATION');
      if (input.policyVersion !== POLICY.version) throw new ApiError('계약 조건이 바뀌었어요. 새 조건을 확인하고 다시 신청해 주세요.', 409, 'POLICY_CHANGED');
      const now = stampNow();
      const n = d.applications.length + 20;
      const app: PublishingApplication = {
        id: `a_${input.idempotencyKey}`, number: `PUB-${now.slice(0, 4)}-${String(n).padStart(4, '0')}`, status: 'submitted',
        workIds: works.map(w => w.id), workTitles: works.map(w => w.title), territories: input.territories, rights: input.rights,
        submittedAt: now, contractId: null, history: [{ at: now, text: '전자계약 서명 · 제출 완료' }],
      };
      d.applications.unshift(app);
      for (const w of works) {
        w.application = { id: app.id, number: app.number, status: 'submitted' };
        w.registration = 'internal_review';
        w.registrationLog.push({ status: 'internal_review', at: now });
        w.contributors = w.contributors.map(c => (c.isMe ? { ...c, publisherShare: c.publisherShare || c.share } : c));
        w.activity.unshift({ at: now, text: `저작권 관리를 신청했어요 (${app.number}).`, kind: 'apply', who: d.me.name });
        w.updatedAt = now;
      }
      return structuredClone(app);
    });
  },
  async withdrawApplication(id: string) {
    await wait(300);
    return mutate(d => {
      const a = d.applications.find(x => x.id === id);
      if (!a) throw notFound();
      if (!['submitted', 'in_review', 'needs_fix'].includes(a.status)) throw new ApiError('이미 처리된 신청은 철회할 수 없어요.', 409, 'INVALID_STATE');
      a.status = 'withdrawn';
      a.history.push({ at: stampNow(), text: '철회' });
      for (const w of d.works.filter(x => a.workIds.includes(x.id))) {
        w.application = { id: a.id, number: a.number, status: 'withdrawn' };
        w.registration = 'none';
        w.fix = null;
        w.activity.unshift({ at: stampNow(), text: '저작권 관리 신청을 철회했어요.', kind: 'apply', who: d.me.name });
      }
      return structuredClone(a);
    });
  },
  /** 보완 요청 처리 — 원본은 두고 고친 내용과 서류로 다시 낸다 (기획안 7.3) */
  async resubmitFix(workId: string, note: string, files: string[]) {
    await wait(500);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      if (!w || !w.fix) throw new ApiError('처리할 보완 요청이 없어요.', 409, 'INVALID_STATE');
      const now = stampNow();
      for (const f of files) w.attachments.push({ id: uid(), kind: 'proof', name: f, at: now });
      w.fix = null;
      w.registration = 'internal_review';
      w.registrationLog.push({ status: 'internal_review', at: now, note: '보완 제출' });
      if (w.application) w.application.status = 'in_review';
      const a = d.applications.find(x => x.id === w.application?.id);
      if (a) { a.status = 'in_review'; a.history.push({ at: now, text: '보완 제출' }); }
      w.activity.unshift({ at: now, text: `보완 자료를 냈어요${note ? ` — ${note}` : ''}.`, kind: 'review', who: d.me.name });
      return structuredClone(w);
    });
  },
  async addAttachment(workId: string, kind: WorkAttachment['kind'], name: string, size?: number) {
    await wait(260);
    return mutate(d => {
      const w = d.works.find(x => x.id === workId);
      if (!w) throw notFound();
      const at = stampNow();
      w.attachments.push({ id: uid(), kind, name, size, at });
      w.activity.unshift({ at, text: `‘${name}’ 파일을 올렸어요.`, kind: 'file', who: d.me.name });
      return structuredClone(w);
    });
  },

  async listContracts() { await wait(); return structuredClone(db.get().contracts); },
  async renewContract(id: string, months: number) {
    await wait(500);
    return mutate(d => {
      const c = d.contracts.find(x => x.id === id);
      if (!c) throw notFound();
      if (!POLICY.renewalOptions.includes(months)) throw new ApiError('고를 수 없는 연장 기간이에요.', 400, 'VALIDATION');
      c.renewal = { months, requestedAt: stampNow() };
      return structuredClone(c);
    });
  },
  async requestTermination(id: string) {
    await wait(400);
    return mutate(d => {
      const c = d.contracts.find(x => x.id === id);
      if (!c) throw notFound();
      c.status = 'terminating';
      c.terminationRequestedAt = stampNow();
      return structuredClone(c);
    });
  },

  async listRoyalties(workId?: string) {
    await wait();
    const all = db.get().royalties;
    return structuredClone(workId ? all.filter(r => r.workId === workId) : all).sort((a, b) => b.period.localeCompare(a.period));
  },

  async listAr() { await wait(); return structuredClone(db.get().ar); },
  async submitAr(input: ArInput) {
    await wait(600);
    return mutate(d => {
      const dup = d.ar.find(a => a.id === `ar_${input.idempotencyKey}`);
      if (dup) return structuredClone(dup);
      const w = d.works.find(x => x.id === input.workId);
      if (!w) throw notFound();
      if (!input.consent) throw new ApiError('피칭 동의가 필요해요.', 400, 'VALIDATION');
      if (d.ar.some(a => a.workId === w.id && a.status !== 'closed')) throw new ApiError('이 곡은 이미 심사 중이에요.', 409, 'DUPLICATE_SUBMISSION');
      const now = stampNow();
      const s: ArSubmission = {
        id: `ar_${input.idempotencyKey}`, workId: w.id, workTitle: w.title, genre: input.genre, mood: input.mood, description: input.description,
        references: input.references, scope: input.scope, status: 'submitted', submittedAt: now, history: [{ at: now, text: '제출 완료' }],
      };
      d.ar.unshift(s);
      w.activity.unshift({ at: now, text: 'Open A&R에 제출했어요.', kind: 'apply', who: d.me.name });
      return structuredClone(s);
    });
  },
  async withdrawAr(id: string) {
    await wait(300);
    return mutate(d => {
      const a = d.ar.find(x => x.id === id);
      if (!a) throw notFound();
      a.status = 'closed';
      a.history.push({ at: stampNow(), text: '제출 취소' });
      return structuredClone(a);
    });
  },
  reset() { db.reset(); clearAsyncCache(); },
};

// ---------- 실서버 ----------

const enc = encodeURIComponent;
/** 저작권 관리 API가 아직 열리지 않은 서버(404)면 빈 목록 — 배급 화면이 이 때문에 깨지지 않게 (기획안 원칙 6) */
async function orEmpty<T>(p: Promise<T>, empty: T): Promise<T> {
  try { return await p; } catch (e) { if (e instanceof ApiError && (e.status === 404 || e.code === 'NOT_FOUND')) return empty; throw e; }
}
const EMPTY_SUMMARY: PublishingSummary = {
  enrolled: false, works: { total: 0, managed: 0, inProgress: 0, unregistered: 0 }, tasks: [],
  revenue: { distribution: { confirmed: 0, estimated: 0 }, publishing: { confirmed: 0, estimated: 0 }, payable: 0 }, suggestion: null,
};

const remoteApi = {
  policy: () => req<PublishingPolicy>(orgPath('/publishing/policy')),
  summary: () => orEmpty(req<PublishingSummary>(orgPath('/publishing/summary'), { quietServer: true }), EMPTY_SUMMARY),
  listWorks: () => orEmpty(req<{ items: Work[] }>(orgPath('/works')).then(r => r.items), [] as Work[]),
  getWork: (id: string) => req<Work>(orgPath(`/works/${enc(id)}`)),
  workByTrack: (trackId: string) => orEmpty(req<{ items: Work[] }>(orgPath(`/works?track_id=${enc(trackId)}`), { quietServer: true }).then(r => r.items[0] ?? null), null),
  createWork: (input: WorkInput) => req<Work>(orgPath('/works'), { method: 'POST', body: input }),
  prepareWorksFromRelease: (releaseId: string, enroll: boolean, skipTitles: string[] = []) =>
    req<PrepareResult>(orgPath(`/releases/${enc(releaseId)}/works`), { method: 'POST', body: { enroll, skip_titles: skipTitles } }),
  updateWork: async (id: string, patch: Partial<WorkInput>, rowVersion: number) => {
    // 지분은 권리 전용 경로로 — 서버가 변경 이력과 확인 요청을 따로 남긴다
    const { contributors, ...rest } = patch;
    let w: Work | null = null;
    if (Object.keys(rest).length) w = await req<Work>(orgPath(`/works/${enc(id)}`), { method: 'PATCH', body: { ...rest, row_version: rowVersion } });
    if (contributors) w = await req<Work>(orgPath(`/works/${enc(id)}/rights`), { method: 'PUT', body: { contributors, row_version: w?.rowVersion ?? rowVersion } });
    return w ?? req<Work>(orgPath(`/works/${enc(id)}`));
  },
  deleteWork: (id: string) => req<void>(orgPath(`/works/${enc(id)}`), { method: 'DELETE' }),
  linkRecording: (workId: string, rec: Omit<WorkRecording, 'id'>) => req<Work>(orgPath(`/works/${enc(workId)}/recordings`), { method: 'POST', body: { release_id: rec.releaseId, track_id: rec.trackId } }),
  unlinkRecording: (workId: string, recordingId: string) => req<Work>(orgPath(`/works/${enc(workId)}/recordings/${enc(recordingId)}`), { method: 'DELETE' }),
  requestSplitConfirm: (workId: string, contributorId: string) => req<Work>(orgPath(`/works/${enc(workId)}/rights/${enc(contributorId)}/confirmation`), { method: 'POST' }),
  respondSplit: (workId: string, agree: boolean) => req<Work>(orgPath(`/works/${enc(workId)}/rights/confirmation`), { method: 'POST', body: { agree } }),
  simulateConfirm: (workId: string) => req<Work>(orgPath(`/works/${enc(workId)}`)),
  searchCreators: (q: string) => req<{ items: Creator[] }>(orgPath(`/creators?q=${enc(q)}`)).then(r => r.items),
  myCreator: () => req<Creator>('/api/me/creator'),
  saveMyCreator: (c: Creator) => req<Creator>('/api/me/creator', { method: 'PUT', body: c }),
  listApplications: () => orEmpty(req<{ items: PublishingApplication[] }>(orgPath('/publishing/applications')).then(r => r.items), [] as PublishingApplication[]),
  submitApplication: (input: ApplicationInput) => req<PublishingApplication>(orgPath('/publishing/applications'), { method: 'POST', body: input, timeoutMs: 30_000 }),
  withdrawApplication: (id: string) => req<PublishingApplication>(orgPath(`/publishing/applications/${enc(id)}/withdraw`), { method: 'POST' }),
  resubmitFix: (workId: string, note: string, files: string[]) => req<Work>(orgPath(`/works/${enc(workId)}/registration/resubmit`), { method: 'POST', body: { note, files } }),
  addAttachment: (workId: string, kind: WorkAttachment['kind'], name: string, size?: number) => req<Work>(orgPath(`/works/${enc(workId)}/attachments`), { method: 'POST', body: { kind, name, size } }),
  listContracts: () => orEmpty(req<{ items: PublishingContract[] }>(orgPath('/publishing/contracts')).then(r => r.items), [] as PublishingContract[]),
  renewContract: (id: string, months: number) => req<PublishingContract>(orgPath(`/publishing/contracts/${enc(id)}/renewal`), { method: 'POST', body: { months } }),
  requestTermination: (id: string) => req<PublishingContract>(orgPath(`/publishing/contracts/${enc(id)}/termination`), { method: 'POST' }),
  listRoyalties: (workId?: string) => orEmpty(req<{ items: Royalty[] }>(orgPath(workId ? `/works/${enc(workId)}/royalties` : '/publishing/royalties')).then(r => r.items), [] as Royalty[]),
  listAr: () => orEmpty(req<{ items: ArSubmission[] }>(orgPath('/ar/submissions')).then(r => r.items), [] as ArSubmission[]),
  submitAr: (input: ArInput) => req<ArSubmission>(orgPath('/ar/submissions'), { method: 'POST', body: input }),
  withdrawAr: (id: string) => req<ArSubmission>(orgPath(`/ar/submissions/${enc(id)}/withdraw`), { method: 'POST' }),
  reset() { /* 실서버에는 없음 */ },
};

export const pubApi: typeof mockApi = MOCK ? mockApi : (remoteApi as unknown as typeof mockApi);
