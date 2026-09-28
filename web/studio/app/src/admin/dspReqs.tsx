// DSP별 요구 조건 — 레지스트리 사양(무엇을 요구하는지)과 이 발매의 스테이징 검사(맞는지)를 한 표로.
// 문제가 있으면 바로 빨간색: 콘텐츠 차단(BLOCKER)은 빨강, 권고(WARNING)는 주황, 연동 대기는 회색.
import type { DspItem, StagingRow } from './api';
import { Chip } from './ui';
import { Glyph } from '../components/Glyph';
import { CheckIcon } from '../components/Check';

export type ReqState = 'ok' | 'bad' | 'warn' | 'wait' | 'pending';
interface Req { key: string; label: string; codes: string[]; spec: (d: DspItem) => string; partner?: boolean; krOnly?: boolean }

export const DSP_REQS: Req[] = [
  { key: 'lead', label: '발매일 여유', codes: ['DSP_LEAD_TIME_SHORT'], spec: d => `발매일 ${d.lead_days}일 전까지 전달` },
  {
    key: 'artwork', label: '커버아트', codes: ['DSP_ARTWORK_NOT_SQUARE', 'DSP_ARTWORK_TOO_SMALL', 'DSP_ARTWORK_TOO_LARGE', 'DSP_ARTWORK_UNMEASURED'],
    spec: d => `정사각형 ${d.artwork_min_px}px 이상${d.artwork_max_px ? ` · ${d.artwork_max_px}px 이하` : ''}`,
  },
  {
    key: 'audio', label: '음원 규격', codes: ['DSP_AUDIO_NOT_LOSSLESS', 'DSP_AUDIO_SAMPLE_RATE_LOW', 'DSP_AUDIO_BIT_DEPTH_LOW', 'DSP_AUDIO_UNMEASURED'],
    spec: d => [d.lossless_only !== false && '무손실', `${(d.audio_min_sample_rate ?? 44100) / 1000}kHz 이상`, `${d.audio_min_bits ?? 16}bit 이상`].filter(Boolean).join(' · '),
  },
  {
    key: 'credits', label: '크레딧', codes: ['DSP_CREDIT_COMPOSER_MISSING', 'DSP_CREDIT_LYRICIST_MISSING'],
    spec: d => [d.requires_composer !== false && '작곡가 필수', d.requires_lyricist && '작사가 필수(보컬곡)'].filter(Boolean).join(' · ') || '필수 없음',
  },
  { key: 'genre', label: '장르', codes: ['DSP_GENRE_MISSING'], spec: () => '장르 지정 필수' },
  { key: 'marking', label: '청소년 유해 표시', codes: ['DSP_KR_YOUTH_HARMFUL_MARKING'], spec: () => '19금 곡은 유해 표시 필수', krOnly: true },
  { key: 'loudness', label: '음량', codes: ['DSP_LOUDNESS_ADVISORY', 'DSP_CLIPPING_ADVISORY'], spec: d => `${d.loudness_target_lufs} LUFS 기준 (권고)` },
  { key: 'ids', label: 'UPC·ISRC', codes: ['DSP_IDENTIFIER_VIRTUAL'], spec: () => '정식 음반·음원 코드' },
  {
    key: 'policy', label: '플랫폼 정책', codes: ['DSP_CONTENT_ID_RISK', 'DSP_COVER_LICENSE_REQUIRED', 'DSP_KR_COVER_CONSENT', 'DSP_AI_POLICY'],
    spec: d => [d.content_id && 'Content ID(커버·샘플 주의)', d.cover_license_required && '커버곡 이용허락', d.ai_policy && 'AI 활용 정책'].filter(Boolean).join(' · ') || '추가 정책 없음',
  },
  {
    key: 'delivery', label: '전송 방식', codes: ['DSP_ERN_VERSION_UNSUPPORTED', 'DSP_AUDIO_SERVED_DOWNSAMPLED'],
    spec: d => [CHANNEL_LABEL[d.channel ?? ''] ?? '—', d.ern_version ? `DDEX ERN ${d.ern_version}` : '플랫폼 전용 형식', dealLabel(d)].filter(Boolean).join(' · '),
  },
  {
    key: 'package', label: '전송 파일', codes: ['DSP_ERN_BUILD', 'DSP_ERN_XSD', 'DSP_ERN_PREFLIGHT', 'DSP_ERN_BUSINESS_RULE', 'DSP_ERN_PRESET_INVALID'],
    spec: d => (d.format === 'Ddex' ? '국제 표준 형식 검사' : '플랫폼 전용 형식 검사'),
  },
  {
    key: 'route', label: '플랫폼 연동', partner: true,
    codes: ['DSP_ROUTE_NOT_LIVE', 'DSP_NOT_IN_APPROVED_SCOPE', 'DSP_SENDER_DPID_MISSING', 'DSP_RECIPIENT_DPID_MISSING', 'DSP_PARTNER_SPEC_PENDING'],
    spec: d => `${d.contract_route?.route === 'MERLIN' ? 'Merlin 계약' : '직계약'} · 계약·연동 완료`,
  },
];

const CHANNEL_LABEL: Record<string, string> = { SFTP: 'SFTP 전송', TRANSPORTER: 'Apple Transporter', PARTNER_FEED: '플랫폼 전용 피드' };
const MODEL_LABEL: Record<string, string> = { SubscriptionModel: '구독', AdvertisementSupportedModel: '무료(광고)', PayAsYouGoModel: '다운로드' };
const dealLabel = (d: DspItem) => (d.deal?.commercial_models ?? []).map(m => MODEL_LABEL[m] ?? m).join('+');

export const reqsFor = (d: DspItem) => DSP_REQS.filter(r => !r.krOnly || d.region === 'Kr');

function reqState(r: Req, row: StagingRow | undefined): { state: ReqState; detail: string } {
  if (!row) return { state: 'pending', detail: '' };
  const hits = row.checks.filter(c => r.codes.includes(c.code));
  const worst = hits.find(c => c.severity === 'BLOCKER') ?? hits.find(c => c.severity === 'WARNING');
  if (!worst) return { state: 'ok', detail: '' };
  const detail = worst.detail ?? worst.message ?? '';
  if (worst.severity === 'BLOCKER') return { state: r.partner ? 'wait' : 'bad', detail };
  return { state: worst.severity === 'WARNING' ? 'warn' : 'ok', detail };
}

const STATE_LABEL: Record<ReqState, [string, 'green' | 'red' | 'amber' | 'gray' | 'blue']> = {
  ok: ['충족', 'green'], bad: ['문제', 'red'], warn: ['확인 권고', 'amber'], wait: ['DSP 연동 대기', 'gray'], pending: ['검사 전', 'gray'],
};

/** 요청한 플랫폼마다: 요구 조건 × 이 발매의 충족 여부. 콘텐츠 문제가 하나라도 있으면 카드 전체가 빨간색 */
export function DspRequirements({ codes, dsps, staging }: { codes: string[]; dsps: DspItem[]; staging: StagingRow[] }) {
  // 최신 패키지 기준 (staged_at 내림차순으로 온다)
  const latest = staging.length ? staging[0].package_id : null;
  const rows = staging.filter(s => s.package_id === latest);
  const list = dsps.filter(d => codes.includes(d.code ?? d.dsp));
  if (!list.length) return <p className="small muted">요청한 플랫폼이 없어요.</p>;
  return (
    <div className="adm-dspreq-grid">
      {list.map(d => {
        const row = rows.find(s => s.dsp === (d.code ?? d.dsp));
        const results = reqsFor(d).map(r => ({ r, ...reqState(r, row) }));
        const bad = results.some(x => x.state === 'bad');
        const warn = !bad && results.some(x => x.state === 'warn');
        return (
          <div key={d.code ?? d.dsp} className={`adm-dspreq${bad ? ' is-bad' : warn ? ' is-warn' : ''}`}>
            <div className="adm-dspreq-top">
              <b>{d.name}</b>
              <Chip tone={bad ? 'red' : warn ? 'amber' : row ? 'green' : 'gray'}>
                {bad ? `문제 ${results.filter(x => x.state === 'bad').length}건` : warn ? '확인 권고' : row ? '요구 조건 충족' : '배급 준비 후 검사'}
              </Chip>
            </div>
            <ul>
              {results.map(({ r, state, detail }) => (
                <li key={r.key} className={`is-${state}`} title={detail || undefined}>
                  <span className="adm-dspreq-dot" aria-hidden="true">{state === 'ok' ? <CheckIcon size={11} /> : state === 'bad' ? <Glyph name="alert" size={12} /> : null}</span>
                  <span className="adm-min">
                    <b>{r.label}</b>
                    <small>{r.spec(d)}</small>
                  </span>
                  <Chip tone={STATE_LABEL[state][1]}>{STATE_LABEL[state][0]}</Chip>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </div>
  );
}

// ---------- 배급 승인 화면용: 코드 대신 한 줄 요약 ----------
export const DSP_NAME: Record<string, string> = {
  'D-1': '멜론', 'D-2': '지니', 'D-3': 'FLO', 'D-4': '벅스', 'D-5': 'Spotify', 'D-6': 'Apple Music',
  'D-7': 'YouTube Music', 'D-8': 'Amazon Music', 'D-9': 'TIDAL', 'D-10': 'Deezer', 'D-11': 'Qobuz',
};

/** 발매 내용을 고쳐야 하는 문제 (아티스트 보완) */
const CONTENT_TEXT: Record<string, string> = {
  DSP_ARTWORK_NOT_SQUARE: '커버아트가 정사각형이 아니에요',
  DSP_ARTWORK_TOO_SMALL: '커버아트 해상도가 작아요',
  DSP_ARTWORK_TOO_LARGE: '커버아트가 너무 커요',
  DSP_ARTWORK_UNMEASURED: '커버아트 크기를 확인하지 못했어요',
  DSP_AUDIO_NOT_LOSSLESS: '무손실 음원이 아니에요',
  DSP_AUDIO_SAMPLE_RATE_LOW: '음원 샘플레이트가 낮아요',
  DSP_AUDIO_BIT_DEPTH_LOW: '음원 비트 심도가 낮아요',
  DSP_AUDIO_UNMEASURED: '음원 규격을 확인하지 못했어요',
  DSP_CREDIT_COMPOSER_MISSING: '작곡가 크레딧이 없어요',
  DSP_CREDIT_LYRICIST_MISSING: '작사가 크레딧이 없어요',
  DSP_GENRE_MISSING: '장르가 없어요',
  DSP_LEAD_TIME_SHORT: '발매일까지 여유가 부족해요',
  DSP_KR_YOUTH_HARMFUL_MARKING: '청소년 유해 표시가 필요해요',
};
const ERN_CODES = ['DSP_ERN_BUILD', 'DSP_ERN_XSD', 'DSP_ERN_PREFLIGHT', 'DSP_ERN_BUSINESS_RULE', 'DSP_ERN_PRESET_INVALID'];
const LINK_CODES = ['DSP_ROUTE_NOT_LIVE', 'DSP_NOT_IN_APPROVED_SCOPE', 'DSP_SENDER_DPID_MISSING', 'DSP_RECIPIENT_DPID_MISSING', 'DSP_PARTNER_SPEC_PENDING'];
const ADVISORY_TEXT: Record<string, string> = {
  DSP_LOUDNESS_ADVISORY: '음량 권고', DSP_CLIPPING_ADVISORY: '클리핑 권고',
  DSP_CONTENT_ID_RISK: 'Content ID 주의 (커버·샘플·리믹스)', DSP_COVER_LICENSE_REQUIRED: '커버곡 이용허락 확인',
  DSP_KR_COVER_CONSENT: '원작자 커버 동의서 확인', DSP_AI_POLICY: 'AI 활용 정책 확인',
};

export interface Verdict { tone: 'red' | 'gray' | 'green'; headline: string; problems: string[]; notes: string[]; approvable: boolean }

/** 한 DSP 패키지의 상태를 담당자가 읽을 말로: 문제(빨강) > DSP 점검 > DSP 연동 대기 > 전송 가능 */
export function deliveryVerdict(d: { readiness: string; blockers: string[]; warnings: string[] }): Verdict {
  const content = d.blockers.filter(b => b in CONTENT_TEXT).map(b => CONTENT_TEXT[b]);
  const ern = d.blockers.some(b => ERN_CODES.includes(b));
  const link = d.blockers.some(b => LINK_CODES.includes(b));
  const virtual = d.blockers.includes('DSP_IDENTIFIER_VIRTUAL');
  const notes = [
    ...d.warnings.map(w => ADVISORY_TEXT[w] ?? CONTENT_TEXT[w]).filter(Boolean),
    ...(virtual ? ['테스트용 UPC·ISRC — 정식 코드 등록 후 재발급 필요'] : []),
  ];
  if (content.length) return { tone: 'red', headline: '발매 내용 문제', problems: content, notes, approvable: false };
  if (ern) return { tone: 'red', headline: 'DSP 점검 필요', problems: ['플랫폼으로 보낼 파일을 만들지 못했어요 — ‘다시 검사’를 눌러 보고, 계속되면 개발팀에 알려 주세요'], notes, approvable: false };
  if (link) return { tone: 'gray', headline: 'DSP 연동 대기', problems: [], notes, approvable: d.readiness !== 'CONTENT_BLOCKED' };
  return { tone: 'green', headline: '전송 가능', problems: [], notes, approvable: d.readiness !== 'CONTENT_BLOCKED' };
}
