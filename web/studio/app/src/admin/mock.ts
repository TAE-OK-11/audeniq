// 체험(목) 빌드용 스태프 API — 서버 없이 관리자 화면을 확인할 수 있게 메모리 예시 데이터로 동작한다.
// 상태 전이 규칙은 crates/core/src/staff.rs와 같게 흉내 낸다 (민감 항목 승인 → 2차 승인, 거절 → WITHDRAWN 등).
import { ApiError, messageForCode } from '../api/errors';
import { applicationPending, needsSecond } from './labels';
import type {
  ApprovalItem, DecisionInput, DecisionResult, DeliveryDecisionInput, DeliveryItem, DspItem, InquiryMessage, InquiryItem,
  Overview, PayoutItem, QueueRelease, ReleaseSheet, ReleaseTimelineItem, StagingRow, StaffDocument, StaffMe, AgreementTerms, AgreementTermsInput, TakedownItem, CorrectionItem, InfringementClaim, PitchItem,
  IdentityAccount, IdentityVerifyInput, PubApplication, PubAppStatus, PubDecisionInput,
  PubChange, PubContractRequest, StaffArSubmission, ArDecisionAction, PubStatement, RoyaltyImportInput, RoyaltyImportResult
} from './api';
import type { Work, WorkContributor } from '../api/publishing';
import { applicationHash, type StudioDraft } from './application';
import { FIX_FIELDS, type FixField } from '../lib/publishing';

// 체험용 손글씨 서명 (작은 SVG)
const MOCK_SIGNATURE = 'data:image/svg+xml;utf8,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 220 60"><path d="M10 42c14-20 22-30 28-24s-10 26 2 22 18-30 26-28-6 26 6 24 16-18 24-20 4 16 14 14 20-14 30-16 12 10 22 8 20-6 38-10" fill="none" stroke="#1d2433" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/></svg>');

const ME = 'staff-me-0001';
const OTHER = 'staff-kim-0002';
/** 심사 담당 — 체험: '새벽의 온도'는 다른 담당자(김)가 맡고 있다 */
const EMAIL: Record<string, string> = { [ME]: 'staff@audeniq.com', [OTHER]: 'kim@audeniq.com' };
const claims: Record<string, { user_id: string; email: string; at: string }> = {};
const claimOf = (rid: string) => claims[rid] ? { ...claims[rid], mine: claims[rid].user_id === ME } : null;
const now = Date.now();
const iso = (hoursAgo: number) => new Date(now - hoursAgo * 3_600_000).toISOString();
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 180));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const uid = () => Math.random().toString(16).slice(2, 10) + '-mock';


interface MockRelease { q: QueueRelease; sheet: ReleaseSheet }

/** 스튜디오 위자드에서 입력한 것처럼 보이는 제출 내용 (체험용) */
function draftFor(q: QueueRelease, n: number, extra: Partial<StudioDraft> = {}): StudioDraft {
  const artist = q.artist ?? '아티스트';
  return {
    artist, type: q.release_type === 'EP' ? 'ep' : 'single', language: 'ko', genre: 'Indie Pop', label: q.org_name, upc: '',
    notes_lines: [`${artist}의 ${q.release_type === 'EP' ? '첫 EP' : '새 싱글'}이에요.`, '새벽 공기와 계절의 온도를 담았어요.'],
    release_date: q.release_date ?? '2026-10-20', territories: ['WORLD'], platforms: ['melon', 'genie', 'flo', 'spotify', 'apple', 'youtube'],
    ownership: q.org_name, phonogram: `2026 ${q.org_name}`, copyright: `2026 ${q.org_name}`,
    rightsChecks: { rightsMaster: true, rightsComposition: true, rightsArtwork: true, rightsConsent: true, rightsAi: true },
    artistProfile: { isNew: false, spotify: 'https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb', apple: '', melon: 'https://www.melon.com/artist/detail.htm?artistId=123456' },
    options: {
      express: true, expressAck: true, expressReason: '공연 일정에 맞춰야 해요',
      ai: true, aiUses: ['편곡·반주에 AI를 사용했어요', '커버아트를 AI로 만들었어요'], aiTools: ['Suno', 'Midjourney'],
      cover: false, sample: false, featured: n > 1, featuredConsentFile: n > 1 ? '피처링_동의서.pdf' : '', shared: false, rerelease: false, minor: false,
    },
    draftTracks: Array.from({ length: n }, (_, i) => ({
      id: `${q.id}-d${i + 1}`, serverId: `${q.id}-t${i + 1}`, title: i === 0 ? q.title : `${q.title} (Track ${i + 1})`, version: '',
      composers: `${artist}, 김도윤`, lyricists: i === n - 1 && n > 1 ? '' : artist, arrangers: '김도윤', performers: `${artist} (보컬)`, producer: '김도윤',
      featuring: i === 1 ? 'RAY' : '', explicit: false, instrumental: i === n - 1 && n > 1, duration: `03:${String(12 + i * 7).padStart(2, '0')}`,
      audioName: `${String(i + 1).padStart(2, '0')}_master.wav`, audioSpec: 'WAV · 24bit · 48kHz · 스테레오',
      lyrics: i === n - 1 && n > 1 ? '' : '창밖에 번지는 새벽의 온도\n아직 말하지 못한 이야기\n\n천천히, 조금 더 천천히\n우리의 계절이 지나가도록',
    })),
    application: {
      no: `AUD-20260926-${q.id.slice(0, 6).toUpperCase()}`, form: 'AUD-DIST-APP 1.0', submittedAt: '2026-09-26 14:05',
      signerName: artist, signerRole: '아티스트 본인', signature: '', hash: '', agreements: ['truth', 'terms', 'privacy', 'esign'],
    },
    ...extra,
  };
}

function sheetFor(q: QueueRelease, open: [string, string, string][], extra: Partial<ReleaseSheet> = {}): MockRelease {
  const tracks = q.release_type === 'SINGLE' ? 1 : 4;
  const checks = [
    ...open.map(([code, status, detail]) => ({ check_code: code, status, detail, at: q.submitted_at ?? iso(5) })),
    { check_code: 'S1_AUDIO_FORMAT', status: 'PASS', detail: 'FLAC 24bit / 48kHz', at: q.submitted_at ?? iso(5) },
    { check_code: 'S1_COVER_SIZE', status: 'PASS', detail: '3000x3000 JPG', at: q.submitted_at ?? iso(5) },
    { check_code: 'S2_META_CREDITS', status: open.some(o => o[0] === 'S2_META_CREDITS') ? 'REVIEW_REQUIRED' : 'PASS', detail: '', at: q.submitted_at ?? iso(5) },
  ].filter((c, i, a) => a.findIndex(x => x.check_code === c.check_code) === i);
  return {
    q,
    sheet: {
      track_audio: {},
      review_context: { decision_kind: null, allowed_actions: [], requires_second_approval: false, pending_second_approval_id: null, check_counts: {} },
      release: { id: q.id, org_id: q.org_id, org_name: q.org_name, title: q.title, release_type: q.release_type, status: q.status, upc: null, revision_id: q.revision_id, submitted_at: q.submitted_at },
      application: {
        artist: q.artist ?? '', language: 'KOR', genre: 'K-Pop', release_date: q.release_date ?? '', label: q.org_name,
        p_line: `℗ 2026 ${q.org_name}`, c_line: `© 2026 ${q.org_name}`, platforms: q.platforms,
        declarations: { rights_confirmed: true, adult_confirmed: true, is_cover: false, is_remix: false, contains_samples: open.some(o => o[0] === 'S2_UNDECLARED_CONTENT'), ai_involved: false, explicit_content: false },
        tracks: Array.from({ length: tracks }, (_, i) => ({
          id: `${q.id}-t${i + 1}`, title: i === 0 ? q.title : `${q.title} (Track ${i + 1})`, version: '', disc_number: 1, track_number: i + 1,
          isrc: null, asset_kind: 'FLAC', parental_advisory: false,
          credits: [{ party_id: 'p1', role: 'COMPOSER' }, { party_id: 'p2', role: 'LYRICIST' }],
        })),
      },
      draft: draftFor(q, tracks),
      signed_application: { application_no: `AUD-20260926-${q.id.slice(0, 6).toUpperCase()}`, content_hash: '', signer_name: q.artist ?? '', signer_role: '아티스트 본인', received_at: q.submitted_at ?? iso(5), form: 'AUD-DIST-APP 1.0', agreements: ['truth', 'terms', 'privacy', 'esign'], contact_email: `contact@${q.org_id.replace(/^org-/, '')}.example` },
      checks,
      open_checks: open.map(([code, status, detail]) => ({ check_code: code, status, detail, needs_second_approval: needsSecond({ check_code: code, status }) })),
      advisories: [],
      overrides: [],
      notes: [],
      second_approvals: [],
      documents: [],
      delivery_staging: [],
      timeline: [
        { action: 'stage2.decision', reason: `REVIEW:${open.map(o => o[0]).join(',')}`, actor_user_id: null, actor_service: 'worker', at: iso(3) },
        { action: 'stage1.decision', reason: 'PASS', actor_user_id: null, actor_service: 'worker', at: iso(4) },
        { action: 'release.submitted', reason: 'SUBMIT', actor_user_id: 'artist', actor_service: null, at: q.submitted_at ?? iso(5) },
      ],
      ...extra,
    },
  };
}

const q = (p: Partial<QueueRelease> & Pick<QueueRelease, 'id' | 'title' | 'artist' | 'org_name'>): QueueRelease => ({
  org_id: `org-${p.id}`, release_type: 'SINGLE', status: 'STAGE2_REVIEW', revision_id: `rev-${p.id}`,
  release_date: '2026-10-17', submitted_at: iso(6), platforms: ['D-1', 'D-2', 'D-5', 'D-6'], agreement: null, ...p,
});

const releases: MockRelease[] = [
  sheetFor(q({ id: 'r1a2b3c4', title: '새벽의 온도', artist: '한결', org_name: '한결 뮤직', submitted_at: iso(26) }),
    [['S2_META_CREDITS', 'REVIEW_REQUIRED', '2번 트랙 작사가 표기가 아티스트명과 달라요.']]),
  sheetFor(q({ id: 'r2b3c4d5', title: 'Blue Hour', artist: 'NOVA', org_name: 'Nova Sound', release_type: 'EP', submitted_at: iso(9), platforms: ['D-1', 'D-3', 'D-5', 'D-6', 'D-7'] }),
    [['S2_INTEGRITY_DUP', 'REVIEW_REQUIRED', '기존 발매(ORG Moonlit)의 마스터와 지문 일치 92%'], ['S2_RIGHTS_SCOPE', 'REVIEW_REQUIRED', '권리 범위: 전 세계 배급 요청, 계약서는 대한민국 한정']]),
  sheetFor(q({ id: 'r3c4d5e6', title: '여름 끝에서', artist: '소월', org_name: '소월 프로젝트', submitted_at: iso(2) }),
    [['S2_UNDECLARED_CONTENT', 'REVIEW_REQUIRED', '샘플 사용 신호가 감지됐지만 신고 항목에 없어요.']]),
  sheetFor(q({ id: 'r4d5e6f7', title: 'Paper Moon', artist: 'Lumi', org_name: 'Lumi Records', status: 'READY_FOR_DELIVERY', agreement: 'REVIEW', submitted_at: iso(70) }), []),
  sheetFor(q({ id: 'r5e6f7a8', title: '고요한 밤의 노래', artist: '이안', org_name: '이안', status: 'STAGE2_CORRECTION', submitted_at: iso(50) }), []),
];

const takedowns: TakedownItem[] = [{
  id: 'td-1', org_id: 'org-2', org_name: '한결 뮤직', release_id: 'rel-live-1', title: '여름의 끝', package_id: 'pkg-1',
  reason: '이전 레이블과의 계약이 끝나 서비스를 내려야 해요.', status: 'PENDING', decision_note: '',
  created_at: new Date(Date.now() - 3 * 3600_000).toISOString(), decided_at: null,
  platforms: [
    { partner_id: 'D-1', slug: 'melon', name: '멜론', live_status: 'LIVE' },
    { partner_id: 'D-5', slug: 'spotify', name: 'Spotify', live_status: 'LIVE' },
  ],
}];

const corrections: CorrectionItem[] = [{
  id: 'mc-1', org_id: 'org-2', org_name: '한결 뮤직', release_id: 'rel-live-1', title: '여름의 끝', status: 'PENDING',
  changes: { genre: '인디 팝', tracks: [{ track_id: 'trk-1', credits: [{ name: '김한결', role: 'Composer' }, { name: '이편곡', role: 'Arranger' }] }] },
  reason: '작곡가 이름 철자가 틀렸어요.', decision_note: '', created_at: new Date(Date.now() - 2 * 3600_000).toISOString(), decided_at: null,
  tracks: [{ track_id: 'trk-1', title: '여름의 끝' }],
}];

const pitches: PitchItem[] = [{
  id: 'pp-1', org_id: 'org-2', org_name: '한결 뮤직', release_id: 'rel-live-1', title: '여름의 끝', artist: '한결',
  release_date: new Date(Date.now() + 14 * 86400_000).toISOString().slice(0, 10), track_id: 'trk-1', track_title: '여름의 끝',
  description: '여름이 끝나갈 무렵의 아쉬움을 담은 어쿠스틱 팝이에요. 기타와 현악이 천천히 쌓여요.',
  genres: '어쿠스틱 팝', moods: '아련함, 저녁', instruments: '어쿠스틱 기타, 현악', similar_artists: '', marketing: '뮤직비디오 공개 예정',
  status: 'SUBMITTED', platforms: [], platforms_requested: ['melon', 'spotify'], result: '', staff_note: '',
  created_at: new Date(Date.now() - 3 * 3600_000).toISOString(), updated_at: new Date(Date.now() - 3 * 3600_000).toISOString(),
}];

const infringementClaims: InfringementClaim[] = [{
  id: 'ic-1', receipt: 'IC-1A2B3C4D', status: 'RECEIVED', right_type: 'COPYRIGHT',
  work: '저희 회사가 2024년에 발표한 ‘봄비’ (작곡 김하늘)', target: '‘여름의 끝’ 2번 트랙이 같은 멜로디를 씁니다. 멜론 앨범 페이지',
  evidence_urls: ['https://example.com/evidence'], org_id: null, org_name: null, release_id: null, release_title: null,
  staff_note: '', decided_at: null, counter_deadline: null, counter_notice: null, counter_at: null,
  created_at: new Date(Date.now() - 5 * 3600_000).toISOString(),
  claimant: { role: 'RIGHTS_HOLDER', name: '하늘음악 주식회사', email: 'legal@haneul.example', phone: '02-000-0000' },
}];

const approvals: (ApprovalItem & { status: string })[] = [{
  id: 'ap-7e21', org_id: 'org-r6', release_id: 'r6f7a8b9', revision_id: 'rev-r6', title: 'Midnight Drive',
  check_codes: ['AUDIO_SIMILAR_TO_EXISTING'], reason: '유사도 높은 기존 곡은 동일 아티스트의 라이브 버전으로 확인했어요.',
  requested_by: OTHER, expires_at: new Date(now + 40 * 3_600_000).toISOString(), at: iso(4), status: 'PENDING',
}];

const documents: StaffDocument[] = [
  { id: 'doc-1', org_id: 'org-r4d5e6f7', org_name: 'Lumi Records', release_id: 'r4d5e6f7', release_title: 'Paper Moon', kind: 'AGREEMENT', title: 'Paper Moon · AUDENIQ 디지털 음원 배급 신청·계약서', status: 'REVIEW', review_note: null, file_name: null, asset_id: null, signed_at: iso(25), row_version: 3, updated_at: iso(25) },
  { id: 'doc-2', org_id: 'org-r2b3c4d5', org_name: 'Nova Sound', release_id: 'r2b3c4d5', release_title: 'Blue Hour', kind: 'RIGHTS_PROOF', title: '샘플 사용 허락서', status: 'REVIEW', review_note: null, file_name: 'sample-license.pdf', asset_id: 'as-1', row_version: 2, updated_at: iso(8) },
  { id: 'doc-3', org_id: 'org-r3', org_name: '소월 프로젝트', release_id: 'r3c4d5e6', release_title: '여름 끝에서', kind: 'RIGHTS_PROOF', title: '공동 작곡 권리 확인서', status: 'NEEDS', review_note: '서명 페이지가 빠져 있어요.', file_name: 'co-writer.pdf', asset_id: 'as-2', row_version: 4, updated_at: iso(30) },
];

const inquiries: (InquiryItem & { messages: InquiryMessage[] })[] = [
  {
    id: 'iq-1', org_id: 'org-r1a2b3c4', org_name: '한결 뮤직', category: '발매·심사', release_id: 'r1a2b3c4', subject: '심사가 얼마나 걸리나요?', status: 'OPEN', created_at: iso(20), updated_at: iso(20),
    messages: [{ id: 'm1', author_kind: 'MEMBER', author_user: 'u1', body: '10월 17일 발매 예정인데 아직 심사 대기 상태라서요. 일정 안에 가능할까요?', created_at: iso(20) }],
  },
  {
    id: 'iq-2', org_id: 'org-r2b3c4d5', org_name: 'Nova Sound', category: '정산·지급', release_id: null, subject: '지급 계좌 변경 문의', status: 'OPEN', created_at: iso(7), updated_at: iso(7),
    messages: [{ id: 'm2', author_kind: 'MEMBER', author_user: 'u2', body: '법인 계좌로 바꾸고 싶은데 이번 달 정산부터 적용되나요?', created_at: iso(7) }],
  },
  {
    id: 'iq-3', org_id: 'org-x', org_name: '이안', category: '수정·테이크다운', release_id: null, subject: '커버 이미지 교체', status: 'ANSWERED', created_at: iso(60), updated_at: iso(40),
    messages: [
      { id: 'm3', author_kind: 'MEMBER', author_user: 'u3', body: '발매 후 커버를 바꿀 수 있나요?', created_at: iso(60) },
      { id: 'm4', author_kind: 'STAFF', author_user: OTHER, body: '발매·곡 관리에서 수정 신청을 올려 주시면 재심사 후 반영돼요.', created_at: iso(40) },
    ],
  },
];

const deliveries: DeliveryItem[] = [
  { package_id: 'pk-4d5e', dsp: 'D-1', org_id: 'org-r4', org_name: 'Lumi Records', release_id: 'r4d5e6f7', title: 'Paper Moon', readiness: 'READY', approval: 'PENDING', route_status: 'LIVE', route_reason: null, ern_is_preview: false, blockers: [], warnings: ['DSP_LOUDNESS_ADVISORY'], staged_at: iso(12) },
  { package_id: 'pk-4d5e', dsp: 'D-5', org_id: 'org-r4', org_name: 'Lumi Records', release_id: 'r4d5e6f7', title: 'Paper Moon', readiness: 'AWAITING_PARTNER', approval: 'PENDING', route_status: 'PENDING_ONBOARDING', route_reason: 'recipient DPID not registered', ern_is_preview: true, blockers: [], warnings: [], staged_at: iso(12) },
  { package_id: 'pk-9a0b', dsp: 'D-6', org_id: 'org-y', org_name: 'Nova Sound', release_id: 'r9', title: 'Afterglow', readiness: 'CONTENT_BLOCKED', approval: 'PENDING', route_status: null, route_reason: null, ern_is_preview: false, blockers: ['DDEX-PREFLIGHT-RELEASE-TYPE'], warnings: [], staged_at: iso(30) },
];

const DSP_NAMES = ['Melon', 'Genie', 'FLO', 'Bugs', 'Spotify', 'Apple Music / iTunes', 'YouTube Music', 'Amazon Music', 'TIDAL', 'Deezer', 'Qobuz'];
const dsps: DspItem[] = DSP_NAMES.map((name, i) => ({
  code: `D-${i + 1}`, dsp: `D-${i + 1}`, slug: name.toLowerCase(), name, region: i < 4 ? 'Kr' : 'Global', format: i < 4 ? 'PartnerSpec' : 'Ddex',
  lead_days: i < 4 ? 14 : 7, artwork_min_px: 3000, loudness_target_lufs: -14,
  route: i === 0 || i === 4 ? {
    transport: i === 0 ? 'SFTP' : 'DDEX_SFTP', activation_kind: i === 0 ? 'CONTRACTED' : 'SANDBOX', route_kind: 'DIRECT', delivery_enabled: i === 0,
    recipient_dpid_registered: i === 0, adapter_can_send: i === 0, onboarding_stage: i === 0 ? 'LIVE' : 'TESTING', onboarding_gaps: i === 0 ? [] : ['RECIPIENT_DPID'],
  } : null,
}));

const payouts: PayoutItem[] = [
  { id: 'po-1', org_id: 'org-r4', org_name: 'Lumi Records', amount: '184500', currency: 'KRW', status: 'REQUESTED', payout_order_id: null, created_at: iso(15) },
  { id: 'po-2', org_id: 'org-x', org_name: '이안', amount: '52300', currency: 'KRW', status: 'REQUESTED', payout_order_id: null, created_at: iso(40) },
];

releases.find(r => r.q.id === 'r4d5e6f7')!.sheet.documents.push(documents[0]);

claims.r1a2b3c4 = { user_id: OTHER, email: EMAIL[OTHER], at: iso(3) };
// 체험: Blue Hour는 YouTube Content ID도 신청했는데 원본 녹음 확인을 체크하지 않았다
{
  const blue = releases.find(r => r.q.id === 'r2b3c4d5')!.sheet;
  blue.application.platforms.push('D-27');
  blue.draft!.platforms = [...(blue.draft!.platforms ?? []), 'youtube-cid'];
  Object.assign(blue.draft!.options!, { contentIdExclusiveRightsAck: true, contentIdOriginalRecordingAck: false });
  // 권리 확인 몇 개를 체크하지 않았다 (요약에서 한 줄로 묶이는지 확인)
  blue.draft!.rightsChecks = { rightsMaster: true, rightsComposition: false, rightsArtwork: false, rightsConsent: true };
}
// 체험: Paper Moon은 배급 준비까지 끝나 플랫폼별 상태가 있다
{
  const paper = releases.find(r => r.q.id === 'r4d5e6f7')!.sheet;
  const row = (dsp: string, readiness: string, checks: StagingRow['checks'] = [], extra: Partial<StagingRow> = {}): StagingRow => ({
    package_id: 'pk-r4', dsp, readiness, approval: readiness === 'READY' ? 'APPROVED' : 'PENDING', checks,
    route_status: null, route_reason: null, ern_message_id: null, ern_sha256: null, ern_is_preview: false,
    approval_by: null, approval_note: null, approval_at: null, staged_at: iso(20), ...extra,
  });
  paper.delivery_staging = [
    row('D-5', 'CONTENT_BLOCKED', [
      { code: 'S2_DSP_TITLE_FEATURED_ARTIST', severity: 'BLOCKER', class: 'CONTENT', message: '곡명에 feat. 표기가 있어요. Spotify는 참여 아티스트 칸으로 받아요.' },
      { code: 'S2_DSP_SPOTIFY_ARTWORK_ENCODING', severity: 'WARNING', class: 'CONTENT', message: '커버에 ICC 프로필이 들어 있어요. sRGB로 다시 내보내면 좋아요.' },
    ]),
    row('D-6', 'AWAITING_PARTNER', [{ code: 'PARTNER_FEED', severity: 'INFO', message: 'Apple Music 전송 계정 연결을 기다리고 있어요.' }], { route_reason: '파트너 연동 전 — 연결되면 자동 전송' }),
    row('D-1', 'READY', [{ code: 'DSP_LOUDNESS_ADVISORY', severity: 'INFO', message: '음량 -9.8 LUFS — 플랫폼에서 자동 조정돼요.' }]),
    row('D-2', 'READY'),
    row('D-3', 'READY', [], { ern_is_preview: true }),
  ];
}
/** 발매 심사 대기: 2차 검사에서 멈췄거나, 자동 검사를 통과했고 신청서가 검토 전 */
const awaiting = (r: MockRelease) => r.q.status === 'STAGE2_REVIEW' || (r.q.status === 'READY_FOR_DELIVERY' && applicationPending(r.q.agreement));

const find = (rid: string) => releases.find(r => r.q.id === rid) ?? fail('NOT_FOUND', 404);
const setStatus = (r: MockRelease, status: string) => { r.q.status = status; r.sheet.release.status = status; };
const audit = (r: MockRelease, action: string, reason: string) =>
  r.sheet.timeline.unshift({ action, reason, actor_user_id: ME, actor_service: null, at: new Date().toISOString() });

// ---------- 저작권 관리 신청 (퍼블리싱) ----------
const person = (name: string, roles: WorkContributor['roles'], share: number, managed: number, ipi: string | null, confirm: WorkContributor['confirm'] = 'confirmed', me = false): WorkContributor =>
  ({ id: uid(), creatorId: me ? 'party-applicant' : null, name, roles, society: 'KOMCA', ipi, share, publisherShare: managed, confirm, external: !me, isMe: false });
const pubWork = (id: string, title: string, people: WorkContributor[], release: string, registration: Work['registration']): Work => ({
  id, title, altTitles: [], workType: 'original', language: 'ko', genre: 'Indie Pop', description: '', visibility: 'private', iswc: null,
  internalCode: `AQW-${id.replace(/\D/g, '').padStart(6, '0')}`, hasPriorPublisher: false, priorPublisher: null, contributors: people, attachments: [],
  recordings: [{ id: uid(), releaseId: 'rel-pub', trackId: uid(), title, releaseTitle: release, isrc: null, status: 'live' }],
  registration, registrationLog: [], application: null, contractId: null, fix: null, myRole: 'viewer', activity: [], rowVersion: 1, createdAt: iso(80), updatedAt: iso(30),
});
const pubApps: PubApplication[] = [
  {
    id: 'pa-1', orgId: 'org-2', orgName: '한결 뮤직', number: 'AUD-20261028-K7Q2MX', status: 'submitted',
    workIds: ['pw-1', 'pw-2'], workTitles: ['파도', '노을'], territories: ['KR', 'WORLD'], rights: ['performance', 'mechanical', 'digital'],
    submittedAt: iso(26), fixReason: null, replyNote: null, documentCode: 'b1f0c4a7e2d94c1aa0d35c6e8f2b7a91c4d0e3f5a6b7c8d9e0f1a2b3c4d5e6f7',
    history: [{ at: iso(26), text: '제출 완료' }],
    works: [
      pubWork('pw-1', '파도', [person('김한결', ['lyricist', 'composer'], 60, 60, '00712345678', 'confirmed', true), person('김하늘', ['composer'], 40, 0, null, 'pending')], '여름 EP', 'internal_review'),
      pubWork('pw-2', '노을', [person('김한결', ['lyricist', 'composer'], 100, 100, '00712345678', 'confirmed', true)], '여름 EP', 'internal_review'),
    ],
  },
  {
    id: 'pa-2', orgId: 'org-3', orgName: '새벽 레코즈', number: 'AUD-20261027-P3ND8W', status: 'needs_fix',
    workIds: ['pw-3'], workTitles: ['새벽 네 시'], territories: ['KR'], rights: ['performance', 'digital'],
    submittedAt: iso(70), fixReason: '공동 작사가 박도윤 님의 IPI 번호가 비어 있어요.', replyNote: null, documentCode: null,
    history: [{ at: iso(70), text: '제출 완료' }, { at: iso(50), text: '심사 시작' }, { at: iso(48), text: '보완 요청' }],
    works: [pubWork('pw-3', '새벽 네 시', [person('서린', ['composer'], 60, 60, '00799911122', 'confirmed', true), person('박도윤', ['lyricist'], 40, 0, null)], '새벽', 'needs_fix')],
  },
  {
    id: 'pa-3', orgId: 'org-2', orgName: '한결 뮤직', number: 'AUD-20261012-ZR4TQ9', status: 'approved',
    workIds: ['pw-4'], workTitles: ['첫 번째 싱글'], territories: ['KR', 'JP', 'US', 'WORLD'], rights: ['performance', 'mechanical', 'digital'],
    submittedAt: iso(400), fixReason: null, replyNote: null, documentCode: null,
    history: [{ at: iso(400), text: '제출 완료' }, { at: iso(380), text: '심사 시작' }, { at: iso(370), text: '승인' }],
    works: [pubWork('pw-4', '첫 번째 싱글', [person('김한결', ['lyricist', 'composer'], 100, 100, '00712345678', 'confirmed', true)], '첫 번째 싱글', 'ready')],
  },
];
const pubFind = (aid: string) => pubApps.find(a => a.id === aid) ?? fail('NOT_FOUND', 404);
const pubList = (a: PubApplication) => { const { works: _w, ...rest } = a; return rest; };
const pubRegistration = (a: PubApplication, reg: Work['registration'], ids = a.workIds) => { for (const w of a.works ?? []) if (ids.includes(w.id)) w.registration = reg; };

// 퍼블리싱 요청 (0173) — 지분 변경 · 계약 요청 · Open A&R · 저작권료 명세서 예시
const changeWork = (() => {
  const w = pubWork('pw-4', '첫 번째 싱글', [person('김한결', ['lyricist', 'composer'], 100, 100, '00712345678', 'confirmed', true)], '첫 번째 싱글', 'registered');
  // 같은 사람은 같은 id — 서버처럼 '바뀐 지분'으로 보이게 (id가 다르면 빠짐·추가로 보였다)
  w.pendingChange = { contributors: [{ ...w.contributors[0], share: 70, publisherShare: 70 }, person('이온', ['composer'], 30, 0, '00755511122', 'pending')], requestedAt: iso(20) };
  return w;
})();
const pubChanges: PubChange[] = [{ orgId: 'org-2', orgName: '한결 뮤직', work: changeWork }];
const pubContractReqs: PubContractRequest[] = [
  { id: 'pc-1', orgId: 'org-2', orgName: '한결 뮤직', number: 'AQP-20261013-QW8E2R', status: 'expiring', endDate: '2026-11-28', commissionRate: 10, works: ['첫 번째 싱글'], renewal: { months: 12, requestedAt: iso(30) }, terminationRequestedAt: null },
  { id: 'pc-2', orgId: 'org-3', orgName: '새벽 레코즈', number: 'AQP-20260502-T7Y3UI', status: 'terminating', endDate: '2027-05-01', commissionRate: 10, works: ['새벽 네 시'], renewal: null, terminationRequestedAt: iso(8) },
];
const pubAr: StaffArSubmission[] = [
  { id: 'ar-1', workId: 'pw-9', workTitle: '여름밤 산책', orgId: 'org-3', orgName: '새벽 레코즈', genre: 'City Pop', mood: ['청량한', '설레는'], description: '여름 시즌 싱글로 어울려요.', references: '',
    scope: 'partners', sync: true, status: 'submitted', fixReason: null, fixReply: null, submittedAt: iso(30), history: [{ at: iso(30), text: '제출 완료' }], demoName: 'summer_walk_demo.mp3', lyrics: '여름밤 걸어가\n바람이 불어와' },
  { id: 'ar-2', workId: 'pw-10', workTitle: '새벽 네 시', orgId: 'org-3', orgName: '새벽 레코즈', genre: 'R&B', mood: ['잔잔한'], description: '', references: '',
    scope: 'internal', sync: false, status: 'in_review', fixReason: null, fixReply: null, submittedAt: iso(90), history: [{ at: iso(90), text: '제출 완료' }, { at: iso(60), text: '심사 시작' }], demoName: 'saebyeok_demo_v2.mp3', lyrics: null },
];
const pubStatements: PubStatement[] = [
  { id: 'ps-1', source: 'KOMCA', period: '2026-08', status: 'confirmed', fileName: 'komca_2026_08.csv', rowCount: 3, totalCollected: 412000, importedAt: iso(240) },
];
/** 콘솔의 '정산 잔액에 반영'(opsMock) — 확정한 명세서를 지급 완료로 */
export function markStatementPosted(sid: string) {
  const st = pubStatements.find(x => x.id === sid) ?? fail('NOT_FOUND', 404);
  if (st.status !== 'confirmed') fail('ROYALTY_NOT_CONFIRMED');
  st.status = 'paid';
}
const AR_NEXT: Record<ArDecisionAction, [StaffArSubmission['status'][], StaffArSubmission['status'], string]> = {
  CLAIM: [['submitted'], 'in_review', '심사 시작'], FIX: [['in_review'], 'needs_fix', '보완 요청'], REVIEWED: [['in_review'], 'reviewed', '심사 완료'],
  PROPOSING: [['reviewed'], 'proposing', '제안 진행'], CLOSE: [['submitted', 'in_review', 'needs_fix', 'reviewed', 'proposing'], 'closed', '종료'],
};
const KNOWN_CODES: Record<string, string> = { 'AQW-000004': '첫 번째 싱글', 'AQW-000001': '파도', 'AQW-000002': '노을' };

export const mockStaff = {
  pubChanges: () => wait({ items: pubChanges }),
  pubChangeDecide: (wid: string, b: { approve: boolean; reason?: string }) => {
    const i = pubChanges.findIndex(c => c.work.id === wid);
    if (i < 0) fail('NOT_FOUND', 404);
    if (!b.approve && !b.reason?.trim()) fail('STAFF_REASON_REQUIRED', 400);
    const [c] = pubChanges.splice(i, 1);
    return wait({ ...c.work, contributors: b.approve ? c.work.pendingChange!.contributors : c.work.contributors, pendingChange: null });
  },
  pubContractRequests: () => wait({ items: pubContractReqs }),
  pubContractDecide: (cid: string, b: { kind: 'renewal' | 'termination' | 'scope'; approve: boolean; reason?: string }) => {
    const i = pubContractReqs.findIndex(c => c.id === cid);
    if (i < 0) fail('NOT_FOUND', 404);
    if (!b.approve && !b.reason?.trim()) fail('STAFF_REASON_REQUIRED', 400);
    pubContractReqs.splice(i, 1);
    return wait({ id: cid, outcome: b.kind === 'renewal' ? (b.approve ? 'renewed' : 'renewal_rejected') : b.kind === 'scope' ? (b.approve ? 'scope_changed' : 'scope_rejected') : (b.approve ? 'ended' : 'termination_rejected') });
  },
  pubArList: (status?: string) => wait({ items: pubAr.filter(a => !status || a.status === status) }),
  pubArDecide: (aid: string, b: { action: ArDecisionAction; reason?: string }) => {
    const a = pubAr.find(x => x.id === aid) ?? fail('NOT_FOUND', 404);
    const [from, to, text] = AR_NEXT[b.action];
    if (!from.includes(a.status)) fail('AR_STATE');
    if (b.action === 'FIX' && !b.reason?.trim()) fail('STAFF_REASON_REQUIRED', 400);
    a.status = to;
    if (to === 'needs_fix') { a.fixReason = b.reason!.trim(); a.fixReply = null; }
    a.history.push({ at: iso(0), text });
    return wait(a);
  },
  pubStatements: () => wait({ items: pubStatements }),
  pubRoyaltyImport: (i: RoyaltyImportInput) => {
    // 서버처럼 원화가 아니면 환율을 곱해 원으로 (내림)
    const fx = i.currency && i.currency !== 'KRW' ? i.fxRate ?? 0 : 1;
    if (!(fx > 0)) fail('ROYALTY_FX_REQUIRED', 400);
    const rows: RoyaltyImportResult['rows'] = i.rows.map((r, k) => {
      const title = KNOWN_CODES[r.work.trim().toUpperCase()];
      if (!title) return { line: k + 1, work: r.work, error: '곡 코드(AQW-… 또는 ISWC)를 찾지 못했어요' };
      const won = Math.floor(r.collected * fx);
      const fee = Math.floor(won / 10);
      return { line: k + 1, workTitle: title, orgName: '한결 뮤직', collected: won, fee, net: won - fee };
    });
    const errors = rows.filter(r => r.error).length;
    if (!i.preview && errors) fail('ROYALTY_ROWS_INVALID');
    if (i.preview) return wait({ rows, errors });
    const st: PubStatement = { id: uid(), source: i.source, period: i.period, status: i.status, fileName: i.fileName ?? null, rowCount: rows.length, totalCollected: rows.reduce((n, r) => n + (r.collected ?? 0), 0), importedAt: iso(0), currency: i.currency ?? 'KRW', fxRate: i.currency && i.currency !== 'KRW' ? fx : null };
    pubStatements.unshift(st);
    return wait({ id: st.id, rows, errors: 0 });
  },
  pubStatementStatus: (sid: string, status: 'confirmed' | 'paid') => {
    const st = pubStatements.find(x => x.id === sid) ?? fail('NOT_FOUND', 404);
    if (!((st.status === 'estimated' && status === 'confirmed') || (st.status === 'confirmed' && status === 'paid'))) fail('ROYALTY_STATE');
    st.status = status;
    return wait({ id: sid, status });
  },
  me: () => wait<StaffMe>({ user_id: ME, role: 'OWNER', level: 4, duties: ['REVIEW', 'DOCUMENTS', 'INQUIRIES', 'DELIVERY', 'IDENTITY', 'FINANCE'], dual_approval: false }),
  overview: () => wait<Overview>({
    review: releases.filter(awaiting).length,
    correction: releases.filter(r => r.q.status.endsWith('_CORRECTION')).length,
    in_pipeline: 3,
    second_approvals: approvals.filter(a => a.status === 'PENDING').length,
    documents: documents.filter(d => d.kind === 'RIGHTS_PROOF' && d.status === 'REVIEW').length,
    inquiries: inquiries.filter(i => i.status === 'OPEN').length,
    deliveries_to_approve: deliveries.filter(d => d.approval === 'PENDING' && d.readiness !== 'CONTENT_BLOCKED').length,
    deliveries_blocked: deliveries.filter(d => d.readiness === 'CONTENT_BLOCKED').length,
    audio_advisories: deliveries.filter(d => d.approval === 'PENDING' && d.warnings.length).length,
    payout_requests: payouts.filter(p => p.status === 'REQUESTED').length,
  }),
  releases: (status: string) => wait({ items: releases.filter(r => (status === 'PENDING' ? awaiting(r) : r.q.status === status)).map(r => ({ ...r.q, claim: claimOf(r.q.id) })) }),
  release: async (rid: string) => {
    const r = find(rid);
    const sheet = r.sheet;
    // 체험용 신청서에 서명 이미지와 문서 확인 코드를 붙인다 (실서버는 스튜디오가 서명할 때 계산)
    const d = sheet.draft;
    if (d?.application && !d.application.hash) {
      d.application.signature = MOCK_SIGNATURE;
      d.application.hash = await applicationHash(d, sheet.release.title, d.application);
      if (sheet.signed_application) {
        sheet.signed_application.content_hash = d.application.hash;
        sheet.signed_application.signature = MOCK_SIGNATURE;
      }
    }
    const pending = approvals.find(a => a.release_id === rid && a.status === 'PENDING' && Date.parse(a.expires_at) > Date.now());
    const kind = r.q.status === 'STAGE2_REVIEW' ? 'CHECKS' : awaiting(r) ? 'APPLICATION' : null;
    sheet.second_approvals.forEach(a => { a.active = a.status === 'PENDING' && Date.parse(a.expires_at) > Date.now(); });
    const claim = claimOf(rid);
    sheet.review_context = {
      decision_kind: kind,
      claim, can_claim: !!kind && !claim, can_take_over: !!kind && !!claim && !claim.mine,
      allowed_actions: kind && claim?.mine ? [...(pending ? [] : ['APPROVE' as const]), ...(kind === 'APPLICATION' || sheet.open_checks.length ? ['REQUEST_CORRECTION' as const] : []), 'REJECT'] : [],
      requires_second_approval: sheet.open_checks.some(needsSecond),
      pending_second_approval_id: pending?.id ?? null,
      check_counts: sheet.checks.reduce<Record<string, number>>((m, c) => { m[c.status] = (m[c.status] ?? 0) + 1; return m; }, {}),
    };
    return wait(sheet);
  },
  timeline: (rid: string, limit = 100) => {
    const r = find(rid);
    const items: ReleaseTimelineItem[] = r.sheet.timeline.map(t => ({
      at: t.at, source: 'audit' as const, kind: t.action,
      detail: { reason: t.reason, actor_user_id: t.actor_user_id, actor_service: t.actor_service },
    }));
    // 체험: 처리 작업·검사·플랫폼 기록도 섞어 보여 준다
    const sub = r.q.submitted_at ?? iso(5);
    const after = (min: number) => new Date(Date.parse(sub) + min * 60_000).toISOString();
    items.push(
      { at: after(1), source: 'job', kind: 'asset.analyze', detail: { status: 'SUCCEEDED', attempts: 1 } },
      { at: after(2), source: 'job', kind: 'stage1', detail: { status: 'SUCCEEDED', attempts: 1 } },
      { at: after(6), source: 'job', kind: 'stage2', detail: { status: 'SUCCEEDED', attempts: 2, last_error: 'fingerprint service timeout (retried)' } },
      ...r.sheet.open_checks.map(c => ({ at: after(7), source: 'check' as const, kind: c.check_code, detail: { status: c.status, detail: c.detail, stage: 2 } })),
    );
    for (const d of r.sheet.delivery_staging) {
      items.push({ at: d.staged_at, source: 'staff_decision', kind: d.dsp, detail: { approval: d.approval, note: d.readiness === 'CONTENT_BLOCKED' ? '콘텐츠 문제로 전송 보류' : null } });
      if (d.readiness === 'READY') items.push({ at: after(30), source: 'dsp_request', kind: d.dsp, detail: { outcome: 'SENT' } });
    }
    items.sort((a, b) => a.at.localeCompare(b.at));
    return wait({ release_id: rid, truncated: items.length > limit, items: items.slice(-limit) });
  },
  claim: (rid: string, takeOver = false) => {
    const r = find(rid);
    if (!awaiting(r)) fail('RELEASE_NOT_IN_REVIEW');
    const cur = claims[rid];
    if (cur && cur.user_id !== ME && !takeOver) fail('REVIEW_CLAIMED_BY_OTHER');
    if (!cur || cur.user_id !== ME) {
      claims[rid] = { user_id: ME, email: EMAIL[ME], at: new Date().toISOString() };
      audit(r, cur ? 'staff.review_taken_over' : 'staff.review_claimed', cur?.user_id ?? '');
    }
    return wait({ release_id: rid, claimed_by: ME });
  },
  unclaim: (rid: string) => {
    const r = find(rid);
    if (claims[rid]) { delete claims[rid]; audit(r, 'staff.review_released', ME); }
    return wait({ release_id: rid, claimed_by: null });
  },
  agreementTerms: async (rid: string, i: AgreementTermsInput) => {
    const r = find(rid);
    const d = r.sheet.documents.find(x => x.kind === 'AGREEMENT' && ['REVIEW', 'PREPARED'].includes(x.status));
    if (!d) fail('AGREEMENT_NOT_IN_REVIEW');
    const sc = i.scope;
    const scope = sc && { ...sc, worldwide: !sc.territories.length || sc.territories.includes('WORLD'), all_dsps: !sc.dsps.length };
    const terms: AgreementTerms = { ...i, scope, form: 'AUD-DIST 2.0', terms_version: 'AUD-TERMS 2026.10', user_bps: 10000 - i.fee_bps, currency: 'KRW',
      confirmations: [], required: [], set_at: new Date().toISOString() };
    const body = `AUDENIQ 음원 배급 계약서 (체험)\n\n배급 형태: ${i.exclusivity === 'EXCLUSIVE' ? '독점' : '비독점'}\n배급수수료: 회사 ${i.fee_bps / 100}% / 이용자 ${(10000 - i.fee_bps) / 100}%${i.rate_note ? `\n특별 요율: ${i.rate_note}` : ''}\n개별 특약: ${i.special_terms || '없음'}`;
    Object.assign(d!, { agreement_terms: terms, body, row_version: d!.row_version + 1 });
    return wait({ id: d!.id, row_version: d!.row_version, terms, body });
  },
  decide: async (rid: string, i: DecisionInput): Promise<DecisionResult> => {
    const r = find(rid);
    if (!awaiting(r)) fail('RELEASE_NOT_IN_REVIEW');
    if (!claims[rid]) fail('REVIEW_CLAIM_REQUIRED');
    if (claims[rid].user_id !== ME) fail('REVIEW_CLAIMED_BY_OTHER');
    // 백엔드와 같게: 승인 메모는 선택(비우면 '담당자 승인'), 보완 요청·거절은 사유 필수
    if (!i.reason.trim() && i.action !== 'APPROVE') fail('DECISION_REASON_REQUIRED');
    if (!i.reason.trim()) i = { ...i, reason: '담당자 승인' };
    if (r.q.status === 'READY_FOR_DELIVERY') {
      // 자동 검사를 통과한 새 발매 신청: 신청서(계약서)와 발매를 함께 결정
      const next = i.action === 'APPROVE' ? null : i.action === 'REJECT' ? 'WITHDRAWN' : 'STAGE3_CORRECTION';
      const agreement = i.action === 'APPROVE' ? 'APPROVED' : i.action === 'REJECT' ? 'REJECTED' : 'NEEDS';
      if (i.action === 'APPROVE' && r.sheet.documents.some(d => d.kind === 'AGREEMENT' && ['REVIEW', 'PREPARED'].includes(d.status) && !d.agreement_terms)) fail('AGREEMENT_TERMS_REQUIRED');
      r.q.agreement = agreement;
      r.sheet.documents.filter(d => d.kind === 'AGREEMENT').forEach(d => Object.assign(d, { status: agreement, review_note: next ? i.reason : null }));
      r.sheet.notes.push({ id: uid(), revision_id: i.revision_id, check_code: null, decision: i.action, note: i.reason, author_user_id: ME, at: new Date().toISOString() });
      if (next) setStatus(r, next);
      audit(r, i.action === 'APPROVE' ? 'staff.approved' : i.action === 'REJECT' ? 'staff.rejected' : 'staff.correction_requested', next ? `READY_FOR_DELIVERY->${next}` : 'APPLICATION:APPROVED');
      return wait(next === 'WITHDRAWN' ? { result: 'REJECTED', status: 'WITHDRAWN' } : { result: 'APPLIED', reevaluation_queued: false });
    }
    const open = r.sheet.open_checks;
    const note = (decision: string, text: string, code: string | null = null) =>
      r.sheet.notes.push({ id: uid(), revision_id: i.revision_id, check_code: code, decision, note: text, author_user_id: ME, at: new Date().toISOString() });
    if (i.action === 'APPROVE') {
      if (open.some(needsSecond)) {
        if (approvals.some(a => a.release_id === rid && a.status === 'PENDING')) fail('SECOND_APPROVAL_ALREADY_PENDING');
        const a = { id: `ap-${uid()}`, org_id: r.q.org_id, release_id: rid, revision_id: i.revision_id, title: r.q.title, check_codes: open.map(c => c.check_code), reason: i.reason, requested_by: ME, expires_at: new Date(Date.now() + 72 * 3_600_000).toISOString(), at: new Date().toISOString(), status: 'PENDING' };
        approvals.push(a);
        r.sheet.second_approvals.unshift({ id: a.id, check_codes: a.check_codes, reason: a.reason, requested_by: ME, status: 'PENDING', decided_by: null, expires_at: a.expires_at, at: a.at, active: true });
        audit(r, 'staff.approval_requested', a.check_codes.join(','));
        return wait({ result: 'PENDING_SECOND_APPROVAL', approval_id: a.id, check_codes: a.check_codes });
      }
      note('APPROVE', i.reason);
      r.sheet.open_checks = [];
      setStatus(r, 'STAGE3_PREPARING');
      audit(r, 'staff.approved', `PASS:${open.length}`);
      return wait({ result: 'APPLIED', reevaluation_queued: true, passed: open.map(c => c.check_code) });
    }
    if (i.action === 'REQUEST_CORRECTION') {
      if (!open.length) fail('NOTHING_TO_CORRECT');
      note('REQUEST_CORRECTION', i.reason);
      i.notes?.forEach(n => note('REQUEST_CORRECTION', n.note, n.check_code));
      setStatus(r, 'STAGE2_CORRECTION');
      audit(r, 'staff.correction_requested', open.map(c => c.check_code).join(','));
      return wait({ result: 'APPLIED', reevaluation_queued: true });
    }
    note('REJECT', i.reason);
    i.notes?.forEach(n => note('REJECT', n.note, n.check_code));
    setStatus(r, 'WITHDRAWN');
    audit(r, 'staff.rejected', 'STAGE2_REVIEW->WITHDRAWN');
    return wait({ result: 'REJECTED', status: 'WITHDRAWN' });
  },
  withdraw: async (rid: string) => {
    const r = find(rid);
    setStatus(r, 'WITHDRAWN');
    audit(r, 'release.withdrawn', 'STAFF');
    return wait({ status: 'WITHDRAWN' });
  },
  reissue: async (rid: string) => {
    const r = find(rid);
    if (r.q.status !== 'READY_FOR_DELIVERY') fail('RELEASE_NOT_READY_FOR_DELIVERY');
    return fail('NO_VIRTUAL_IDENTIFIERS');
  },
  approvals: () => wait({ items: approvals.filter(a => a.status === 'PENDING') }),
  takedowns: () => wait({ items: takedowns }),
  pitches: () => wait({ items: pitches }),
  updatePitch: async (pid: string, body: { status: string; platforms?: string[]; result?: string; note?: string }) => {
    const p = pitches.find(x => x.id === pid) ?? fail('NOT_FOUND');
    if (body.status === 'PITCHED' && !body.platforms?.length) fail('PITCH_PLATFORMS_REQUIRED');
    if (body.status === 'SELECTED' && !body.result?.trim()) fail('PITCH_RESULT_REQUIRED');
    Object.assign(p, {
      status: body.status, platforms: body.platforms?.length ? body.platforms : p.platforms,
      result: body.result?.trim() || p.result, staff_note: body.note?.trim() || p.staff_note, updated_at: new Date().toISOString(),
    });
    return wait({ status: body.status });
  },
  corrections: () => wait({ items: corrections }),
  approveCorrection: async (cid: string, note: string) => {
    const c = corrections.find(x => x.id === cid && x.status === 'PENDING') ?? fail('CONFLICT');
    Object.assign(c, { status: 'APPROVED', decision_note: note, decided_at: new Date().toISOString() });
    return wait({ status: 'APPROVED', jobs: ['job-mock'] });
  },
  declineCorrection: async (cid: string, note: string) => {
    if (!note.trim()) fail('CORRECTION_NOTE_REQUIRED');
    const c = corrections.find(x => x.id === cid && x.status === 'PENDING') ?? fail('CONFLICT');
    Object.assign(c, { status: 'DECLINED', decision_note: note, decided_at: new Date().toISOString() });
    return wait({ status: 'DECLINED' });
  },
  claims: (all: boolean) => wait({ items: infringementClaims.filter(c => all || ['RECEIVED', 'SUSPENDED', 'COUNTER_NOTICED'].includes(c.status)) }),
  rejectClaim: async (cid: string, note: string) => {
    if (!note.trim()) fail('CLAIM_NOTE_REQUIRED');
    const c = infringementClaims.find(x => x.id === cid && x.status === 'RECEIVED') ?? fail('CONFLICT');
    Object.assign(c, { status: 'REJECTED', staff_note: note, decided_at: new Date().toISOString() });
    return wait({ status: 'REJECTED' });
  },
  suspendClaim: async (cid: string, releaseId: string, note: string) => {
    const c = infringementClaims.find(x => x.id === cid && x.status === 'RECEIVED') ?? fail('CONFLICT');
    Object.assign(c, {
      status: 'SUSPENDED', release_id: releaseId, release_title: '여름의 끝', org_id: 'org-2', org_name: '한결 뮤직', staff_note: note,
      decided_at: new Date().toISOString(), counter_deadline: new Date(Date.now() + 30 * 86_400_000).toISOString(),
      platforms: [{ partner_id: 'D-1', slug: 'melon', name: '멜론', live_status: 'TAKEDOWN_REQUESTED' }],
    });
    return wait({ status: 'SUSPENDED', jobs: ['job-mock'] });
  },
  restoreClaim: async (cid: string, note: string) => {
    const c = infringementClaims.find(x => x.id === cid && ['SUSPENDED', 'COUNTER_NOTICED'].includes(x.status)) ?? fail('CONFLICT');
    Object.assign(c, { status: 'RESTORED', staff_note: note || c.staff_note });
    return wait({ status: 'RESTORED' });
  },
  upholdClaim: async (cid: string, note: string) => {
    if (!note.trim()) fail('CLAIM_NOTE_REQUIRED');
    const c = infringementClaims.find(x => x.id === cid && ['SUSPENDED', 'COUNTER_NOTICED'].includes(x.status)) ?? fail('CONFLICT');
    Object.assign(c, { status: 'UPHELD', staff_note: note });
    return wait({ status: 'UPHELD' });
  },
  approveTakedown: async (tid: string, note: string) => {
    const t = takedowns.find(x => x.id === tid && x.status === 'PENDING') ?? fail('CONFLICT');
    Object.assign(t, { status: 'APPROVED', decision_note: note, decided_at: new Date().toISOString() });
    t.platforms.forEach(p => { p.live_status = 'TAKEDOWN_REQUESTED'; });
    return wait({ status: 'APPROVED' });
  },
  declineTakedown: async (tid: string, note: string) => {
    if (!note.trim()) fail('REVIEW_NOTE_REQUIRED');
    const t = takedowns.find(x => x.id === tid && x.status === 'PENDING') ?? fail('CONFLICT');
    Object.assign(t, { status: 'DECLINED', decision_note: note, decided_at: new Date().toISOString() });
    return wait({ status: 'DECLINED' });
  },
  markTakenDown: async (pkg: string, partner: string) => {
    takedowns.filter(t => t.package_id === pkg).forEach(t => t.platforms.forEach(p => { if (p.partner_id === partner) p.live_status = 'TAKEN_DOWN'; }));
    return wait({ job_id: 'job-mock' });
  },
  approve: async (aid: string) => {
    const a = approvals.find(x => x.id === aid && x.status === 'PENDING') ?? fail('APPROVAL_NOT_PENDING');
    if (a.requested_by === ME) fail('SECOND_APPROVER_MUST_DIFFER');
    a.status = 'APPROVED';
    return wait({ result: 'APPLIED' });
  },
  decline: async (aid: string) => {
    const a = approvals.find(x => x.id === aid && x.status === 'PENDING') ?? fail('APPROVAL_NOT_PENDING');
    a.status = 'DECLINED';
    return wait({ result: 'DECLINED' });
  },
  documents: (status: string) => wait({ items: documents.filter(d => d.kind === 'RIGHTS_PROOF' && d.status === status) }),
  reviewDocument: async (did: string, i: { status: 'APPROVED' | 'NEEDS'; note: string; row_version: number }) => {
    const d = documents.find(x => x.id === did);
    if (!d || d.row_version !== i.row_version) return fail('CONFLICT', 409);
    if (i.status === 'NEEDS' && !i.note.trim()) fail('REVIEW_NOTE_REQUIRED');
    Object.assign(d, { status: i.status, review_note: i.note.trim() || null, row_version: d.row_version + 1, updated_at: new Date().toISOString() });
    return wait({ status: d.status, row_version: d.row_version });
  },
  requestProof: async (org: string, i: { release_id: string; title: string; body: string }) => {
    const r = find(i.release_id);
    const d: StaffDocument = { id: `doc-${uid()}`, org_id: org, org_name: r.q.org_name, release_id: r.q.id, release_title: r.q.title, kind: 'RIGHTS_PROOF', title: i.title, status: 'AWAITING_DOCUMENTS', review_note: null, file_name: null, asset_id: null, row_version: 1, updated_at: new Date().toISOString() };
    documents.push(d);
    r.sheet.documents.push(d);
    return wait({ id: d.id });
  },
  inquiries: (status: string) => wait({ items: inquiries.filter(i => i.status === status).map(({ messages: _m, ...rest }) => rest) }),
  inquiry: async (iid: string) => {
    const i = inquiries.find(x => x.id === iid) ?? fail('NOT_FOUND', 404);
    const { messages, ...head } = i;
    return wait({ inquiry: head, messages });
  },
  reply: async (iid: string, body: string) => {
    const i = inquiries.find(x => x.id === iid) ?? fail('NOT_FOUND', 404);
    if (i.status === 'CLOSED') fail('INQUIRY_CLOSED');
    const m = { id: uid(), author_kind: 'STAFF', author_user: ME, body: body.trim(), created_at: new Date().toISOString() };
    i.messages.push(m);
    i.status = 'ANSWERED';
    i.updated_at = m.created_at;
    return wait({ id: m.id, status: 'ANSWERED' });
  },
  deliveries: (f: { approval: string; readiness?: string; dsp?: string }) =>
    wait({ items: deliveries.filter(d => d.approval === f.approval && (!f.readiness || d.readiness === f.readiness) && (!f.dsp || d.dsp === f.dsp)) }),
  ern: (pkg: string, dsp: string) => wait(`<?xml version="1.0" encoding="UTF-8"?>
<ern:NewReleaseMessage xmlns:ern="http://ddex.net/xml/ern/382" MessageSchemaVersionId="ern/382">
  <MessageHeader>
    <MessageId>${pkg}-${dsp}</MessageId>
    <MessageSender><PartyId>PADPIDA0000000000A</PartyId><PartyName><FullName>AUDENIQ</FullName></PartyName></MessageSender>
    <MessageRecipient><PartyId>PADPIDA${dsp.replace('-', '')}PREVIEW</PartyId></MessageRecipient>
  </MessageHeader>
  <!-- 체험 모드 예시 ERN -->
</ern:NewReleaseMessage>`),
  decideDelivery: async (pkg: string, dsp: string, i: DeliveryDecisionInput) => {
    const d = deliveries.find(x => x.package_id === pkg && x.dsp === dsp) ?? fail('NOT_FOUND', 404);
    if (i.action === 'HOLD' && !i.note?.trim()) fail('REVIEW_NOTE_REQUIRED');
    if (i.action === 'APPROVE') {
      if (d.readiness === 'CONTENT_BLOCKED') fail('DELIVERY_CONTENT_BLOCKED');
      if (d.warnings.length && !i.acknowledge_warnings) fail('WARNINGS_NOT_ACKNOWLEDGED');
    }
    d.approval = i.action === 'APPROVE' ? 'APPROVED' : 'HELD';
    return wait({ approval: d.approval });
  },
  restage: (_pkg: string) => wait({ job_id: uid() }),
  dsps: () => wait({ items: dsps }),
  payouts: (status: string) => wait({ items: payouts.filter(p => p.status === status) }),
  pubApplications: (status: PubAppStatus) => wait({ items: pubApps.filter(a => a.status === status).map(pubList) }),
  pubApplication: (aid: string) => wait(pubFind(aid)),
  pubClaim: (aid: string) => {
    const a = pubFind(aid);
    if (a.status !== 'submitted') fail('APPLICATION_STATE');
    a.status = 'in_review';
    a.history.push({ at: iso(0), text: '심사 시작' });
    return wait(pubList(a));
  },
  pubDecide: (aid: string, i: PubDecisionInput) => {
    const a = pubFind(aid);
    const open = a.status === 'submitted' || a.status === 'in_review';
    const reason = (i.reason ?? '').trim();
    if (i.action === 'REJECT' && !reason) fail('APPLICATION_REASON_REQUIRED', 400);
    if (i.action === 'REQUEST_CORRECTION' && !reason && !i.fields?.length && !i.documents?.length) fail('APPLICATION_REASON_REQUIRED', 400);
    if (i.action === 'APPROVE' && open) { a.status = 'approved'; pubRegistration(a, 'ready'); a.history.push({ at: iso(0), text: '승인' }); }
    else if (i.action === 'REQUEST_CORRECTION' && open) {
      // 서버와 같이 reason은 한 줄 요약(고칠 항목·서류), note는 담당자가 따로 쓴 의견
      const parts = [...(i.fields ?? []).map(f => FIX_FIELDS[f as FixField] ?? f), ...(i.documents?.length ? [`서류 ${i.documents.join(', ')}`] : [])];
      const summary = parts.length ? `${parts.join(' · ')} 보완` : reason;
      a.status = 'needs_fix'; a.fixReason = summary;
      const ids = i.work_ids?.length ? i.work_ids : a.workIds;
      pubRegistration(a, 'needs_fix', ids);
      for (const w of a.works ?? []) if (ids.includes(w.id)) w.fix = { reason: summary, note: reason || null, fields: i.fields ?? [], documents: i.documents ?? [], requestedAt: iso(0) };
      a.history.push({ at: iso(0), text: '보완 요청' });
    } else if (i.action === 'REJECT' && (open || a.status === 'needs_fix')) { a.status = 'rejected'; a.fixReason = reason; pubRegistration(a, 'rejected'); a.history.push({ at: iso(0), text: '반려' }); }
    else if (i.action === 'REGISTER' && a.status === 'approved') { pubRegistration(a, 'registered'); a.history.push({ at: iso(0), text: '관리 단체 등록 완료' }); }
    else fail('APPLICATION_STATE');
    return wait(pubList(a));
  },
  identityLookup: (email: string) => (email.toLowerCase().endsWith('@example.com') ? wait(identityAccount) : fail('NOT_FOUND', 404)),
  identityVerify: (_uid: string, b: IdentityVerifyInput) => {
    identityAccount.identity = {
      provider: 'OFFLINE', method: b.channel, offline: true, verified_at: iso(0),
      name_masked: `${b.real_name.slice(0, 1)}**`, adult: ageOn(b.birth_date) >= 19,
    };
    identityAccount.checks.unshift({
      id: `chk-${identityAccount.checks.length + 1}`, staff_user_id: ME, channel: b.channel,
      document_type: b.document_type, guardian_relation: b.guardian_relation ?? null, correction: b.correction, note: b.note, created_at: iso(0),
    });
    return wait(identityAccount);
  },
};

// 실명인증 체험: 아무개@example.com 으로 찾을 수 있는 계정 하나
const ageOn = (birth: string) => {
  const t = new Date(), [y, m, d] = birth.split('-').map(Number);
  return t.getFullYear() - y - (t.getMonth() + 1 < m || (t.getMonth() + 1 === m && t.getDate() < d) ? 1 : 0);
};
const identityAccount: IdentityAccount = {
  user_id: '7f1c2a90-5b3e-4d21-9a6f-0c8e1d2b3a45', email_masked: 'h***@e****.com', status: 'ACTIVE', joined_at: iso(4000),
  identity: null, checks: [],
};
