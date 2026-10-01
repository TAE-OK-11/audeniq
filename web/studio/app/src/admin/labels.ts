// 관리자 화면 표기 — 서버 상태·검사 코드를 한국어 라벨과 칩 색(tone)으로.
import { correctionTarget, isKnownCorrection } from '../lib/corrections';
import type { Duty, StaffRole } from './api';
export { dspLabel } from '../lib/catalog';

export type Tone = 'blue' | 'violet' | 'amber' | 'green' | 'red' | 'gray';

export const ROLE_LABEL: Record<StaffRole, string> = {
  ADMIN: '관리자', REVIEWER: '심사 담당', OPERATOR: '배급 운영', SUPPORT: '고객 지원',
};

export const DUTY_LABEL: Record<Duty, string> = {
  REVIEW: '발매 심사', DOCUMENTS: '서류 검토', INQUIRIES: '문의 답변', DELIVERY: '배급 승인',
};

export const RELEASE_STATUS: Record<string, [string, Tone]> = {
  PENDING: ['심사 대기', 'violet'],
  SUBMITTED: ['접수됨', 'blue'],
  STAGE1_RUNNING: ['1차 검사 중', 'blue'],
  STAGE1_CORRECTION: ['1차 보완 요청', 'amber'],
  STAGE1_PASSED: ['1차 통과', 'blue'],
  STAGE2_RUNNING: ['2차 검사 중', 'blue'],
  STAGE2_REVIEW: ['검사 판단 필요', 'violet'],
  STAGE2_CORRECTION: ['2차 보완 요청', 'amber'],
  STAGE2_PASSED: ['2차 통과', 'blue'],
  STAGE3_PREPARING: ['배포 준비 중', 'blue'],
  STAGE3_CORRECTION: ['배포 보완 요청', 'amber'],
  READY_FOR_DELIVERY: ['배급 준비 완료', 'green'],
  ON_HOLD_RIGHTS: ['권리 보류', 'red'],
  WITHDRAWN: ['거절·철회', 'gray'],
};

/** 심사 목록 필터 (백엔드 RELEASE_STATUSES 중 담당자가 자주 보는 순서).
 *  PENDING = 담당자 결정 대기: 새 발매 신청(자동 검사 통과, 신청서 검토 전) + 2차 검사에서 멈춘 발매 */
export const QUEUE_FILTERS = [
  'PENDING', 'STAGE2_REVIEW', 'STAGE1_CORRECTION', 'STAGE2_CORRECTION', 'STAGE3_CORRECTION',
  'ON_HOLD_RIGHTS', 'READY_FOR_DELIVERY', 'SUBMITTED', 'STAGE3_PREPARING', 'WITHDRAWN',
];

export const CHECK_STATUS: Record<string, [string, Tone]> = {
  PASS: ['통과', 'green'],
  NOT_APPLICABLE: ['해당 없음', 'gray'],
  REVIEW_REQUIRED: ['검토 필요', 'violet'],
  CORRECTION_REQUIRED: ['보완 필요', 'amber'],
  BLOCKED: ['차단', 'red'],
  FAIL: ['실패', 'red'],
};

export const DOC_KIND: Record<string, string> = { AGREEMENT: '배급 계약서', RIGHTS_PROOF: '권리 증빙' };
export const DOC_STATUS: Record<string, [string, Tone]> = {
  AWAITING_DOCUMENTS: ['제출 대기', 'gray'],
  REVIEW: ['검토 대기', 'violet'],
  PREPARED: ['서명 준비', 'blue'],
  NEEDS: ['보완 요청', 'amber'],
  APPROVED: ['승인', 'green'],
  SIGNED: ['서명 완료', 'green'],
  REJECTED: ['반려', 'red'],
};

/** 발매 신청서(배급 계약서)가 담당자 결정을 기다리는 상태 */
export const applicationPending = (agreement: string | null | undefined) => agreement === 'REVIEW' || agreement === 'PREPARED';

export const INQUIRY_STATUS: Record<string, [string, Tone]> = {
  OPEN: ['답변 대기', 'violet'], ANSWERED: ['답변 완료', 'green'], CLOSED: ['종료', 'gray'],
};

export const APPROVAL_STATUS: Record<string, [string, Tone]> = {
  PENDING: ['승인 대기', 'violet'], APPROVED: ['승인', 'green'], DECLINED: ['반려', 'gray'], HELD: ['보류', 'amber'],
};

export const READINESS: Record<string, [string, Tone]> = {
  READY: ['전송 가능', 'green'], AWAITING_PARTNER: ['파트너 연동 대기', 'blue'], CONTENT_BLOCKED: ['콘텐츠 차단', 'red'],
};

export const DECISION_LABEL: Record<string, string> = {
  APPROVE: '승인', REQUEST_CORRECTION: '보완 요청', REJECT: '거절',
};

export const PAYOUT_STATUS: Record<string, [string, Tone]> = {
  REQUESTED: ['요청됨', 'violet'], ORDERED: ['지급 지시', 'green'], REJECTED: ['반려', 'red'], CANCELLED: ['취소', 'gray'],
};

export const RELEASE_TYPE: Record<string, string> = { SINGLE: '싱글', EP: 'EP', ALBUM: '정규' };

/** 검사 코드 → 사람이 읽는 이름 (아티스트 화면의 보완 항목 매핑을 재사용) */
export function checkLabel(code: string): string {
  if (isKnownCorrection(code)) return correctionTarget(code).label;
  const extra: Record<string, string> = {
    IMAGE_COLOR_PROFILE: '커버 색상·프로필', IMAGE_TEXT_SCAN: '커버 문자 검사', IMAGE_QR_SCAN: '커버 QR 검사',
    S2_INTEGRITY_DUP: '중복 음원', AUDIO_SIMILAR_TO_EXISTING: '기존 음원과 유사', S2_PROTECTED_NAME: '보호 아티스트명',
    DSP_LOUDNESS_ADVISORY: '음량(라우드니스) 권고', DSP_CLIPPING_ADVISORY: '클리핑 권고',
    S2_EXPRESS_REQUEST: '신속 발매 요청', S2_ADDITIONAL_RIGHTS: '추가 권리 확인',
  };
  return extra[code] ?? code;
}

export const pick = (map: Record<string, [string, Tone]>, key: string | null | undefined): [string, Tone] =>
  (key && map[key]) || [key || '—', 'gray'];

const stampFmt = new Intl.DateTimeFormat('ko-KR', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
const dayFmt = new Intl.DateTimeFormat('ko-KR', { year: 'numeric', month: 'short', day: 'numeric' });

export function when(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? String(iso) : stampFmt.format(d);
}

export function day(iso: string | null | undefined): string {
  if (!iso) return '—';
  const d = new Date(iso.length === 10 ? `${iso}T00:00:00` : iso);
  return Number.isNaN(d.getTime()) ? String(iso) : dayFmt.format(d);
}

/** '3시간 전' 같은 상대 시각 — 대기열이 얼마나 밀렸는지 한눈에 */
export function ago(iso: string | null | undefined): string {
  if (!iso) return '';
  const ms = Date.now() - new Date(iso).getTime();
  if (!Number.isFinite(ms)) return '';
  const m = Math.round(ms / 60000);
  if (m < 1) return '방금';
  if (m < 60) return `${m}분 전`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}시간 전`;
  return `${Math.round(h / 24)}일 전`;
}

export const shortId = (v: string | null | undefined) => (v ? v.slice(0, 8) : '—');

/** 한 명이 통과시킬 수 있는 저위험 검사 (백엔드 review::LOW_RISK_SELF_APPROVABLE와 같게 유지) */
export const LOW_RISK = ['S2_RELEASE_DATE_FAR_FUTURE', 'S2_RELEASE_DATE_FAR_PAST', 'S2_META_CREDITS'];

/** 승인 시 두 번째 심사자가 필요한 항목 — staff::sensitive와 같은 규칙 */
export const needsSecond = (c: { check_code: string; status: string }) => c.status === 'BLOCKED' || !LOW_RISK.includes(c.check_code);

/** review::MAX_OVERRIDE_REASON_CHARS */
export const MAX_REASON = 2000;

// ---------- 담당자가 읽기 쉬운 검사 설명 ----------
const num = (detail: string | null | undefined, re: RegExp) => {
  const m = re.exec(detail ?? '');
  return m ? Number(m[1]) : null;
};

/** 검사 결과 → 담당자가 바로 이해할 한 줄 설명 (원문 detail은 ‘상세 보기’에서) */
export function checkSummary(c: { check_code: string; detail: string | null }): string {
  const d = c.detail ?? '';
  if (c.check_code.startsWith('S2_DSP_') && isKnownCorrection(c.check_code)) return correctionTarget(c.check_code).hint;
  switch (c.check_code) {
    case 'AUDIO_AI_PROVENANCE':
    case 'IMAGE_AI_PROVENANCE':
    case 'S2_AI_LYRICS_PROVENANCE':
      if (d.includes('AI_METADATA_SIGNAL') || d.includes('AI_DISCLOSURE_SIGNAL')) {
        return 'AI 생성 도구 메타데이터 또는 명시적 작성 문구가 있어요. 생성 경위·AI 신고·이용 권리를 확인해 주세요. 조작 가능한 근거여서 AI 작성을 확정하지 않아요. SynthID는 검사하지 않았어요.';
      }
      if (d.includes('FAILED') || d.includes('metadata reader')) {
        return '출처 검사를 완료하지 못했어요. 시스템 재시도가 필요하며 AI 여부를 판정할 수 없어요.';
      }
      return '지원하는 AI 출처 근거가 없어서 판정할 수 없어요. 사람이 만들었다는 증거가 아니며 SynthID는 검사하지 않았어요.';
    case 'AUDIO_SIMILAR_TO_EXISTING': {
      const n = num(d, /similar to (\d+) asset/);
      return `이미 등록된 음원${n ? ` ${n}개` : ''}와 거의 같은 소리예요. 같은 곡을 다시 낸 것인지, 권리가 있는지 확인해 주세요.`;
    }
    case 'AUDIO_LOUDNESS_OUT_OF_RANGE':
    case 'DSP_LOUDNESS_ADVISORY': {
      const lufs = num(d, /integrated_lufs=(-?[\d.]+)/);
      const peak = num(d, /true_peak_dbtp=(-?[\d.]+)/);
      const dir = lufs === null ? '기준과 달라요' : lufs > -14 ? '기준보다 커요' : '기준보다 작아요';
      const measured = [lufs !== null && `측정 ${lufs} LUFS`, peak !== null && `피크 ${peak} dBTP`].filter(Boolean).join(', ');
      return `음량이 스트리밍 ${dir}(권장 -14 LUFS, 피크 -1 dBTP 이하)${measured ? ` · ${measured}` : ''}. 플랫폼이 볼륨을 자동으로 맞추므로 발매는 가능해요.`;
    }
    case 'AUDIO_CLIPPING':
    case 'DSP_CLIPPING_ADVISORY':
      return '소리가 깨지는 구간(클리핑)이 있어요. 심하면 아티스트에게 마스터 파일 교체를 요청하세요.';
    case 'AUDIO_CONTENT_SUSPECT': return '음원 내용이 의심스러워요(무음·잡음·테스트 음원 등). 직접 들어 보고 판단해 주세요.';
    case 'S2_INTEGRITY_DUP': return '다른 발매와 같은 마스터 음원이 쓰였어요. 중복 발매인지 확인해 주세요.';
    case 'S2_INTEGRITY_DISPUTES': return '권리 분쟁이 걸린 음원·아티스트와 관련 있어요.';
    case 'S2_CATALOG_FINGERPRINT': return '카탈로그의 다른 곡과 음원 지문이 겹쳐요. 같은 곡인지 확인해 주세요.';
    case 'S2_CATALOG_IDENTIFIERS': return 'UPC·ISRC가 다른 발매와 겹치거나 형식이 맞지 않아요.';
    case 'S2_SPECIAL_FLAGS': return '19금·커버곡·샘플·AI 활용 같은 특수 항목이 있어요. 신고 내용과 증빙이 맞는지 확인해 주세요.';
    case 'S2_UNDECLARED_CONTENT': return '신고하지 않은 커버곡·샘플·AI 활용 신호가 감지됐어요.';
    case 'S2_CONTENT_SIGNALS': return '콘텐츠 신고 항목을 다시 확인해야 해요.';
    case 'S2_RIGHTS_SCOPE': return '권리자 정보와 배급 범위가 맞는지 확인해 주세요.';
    case 'S2_DOCS_ORIGIN': return '권리 증빙 서류의 출처를 확인할 수 없어요.';
    case 'S2_META_CREDITS': return '작사·작곡 등 크레딧이 비어 있거나 형식이 맞지 않아요.';
    case 'S2_DSP_ELIGIBILITY': return '선택한 플랫폼 중 배급할 수 없는 곳이 있어요.';
    case 'S2_RELEASE_DATE_FAR_PAST': return '발매일이 너무 과거예요.';
    case 'S2_RELEASE_DATE_FAR_FUTURE': return '발매일이 너무 먼 미래예요.';
    case 'S2_PROTECTED_NAME':
    case 'ARTIST_NAME_PROTECTED': return '보호된 유명 아티스트명과 겹쳐요. 본인 활동명인지 확인해 주세요.';
    default:
      return isKnownCorrection(c.check_code) ? correctionTarget(c.check_code).hint : checkLabel(c.check_code);
  }
}

// ---------- 거절 사유 (누르면 그대로 기록, ‘기타’만 직접 입력) ----------
export const REJECT_REASONS: { id: string; label: string; text: string }[] = [
  { id: 'rights', label: '권리 확인 불가', text: '음원·작사·작곡 등 배급에 필요한 권리를 확인할 수 없어요.' },
  { id: 'duplicate', label: '중복 음원', text: '이미 유통 중이거나 등록된 음원과 같거나 매우 비슷해요.' },
  { id: 'plagiarism', label: '표절 의심', text: '다른 저작물을 표절한 것으로 판단돼요.' },
  { id: 'unauthorized', label: '타인 콘텐츠 무단 사용', text: '다른 사람의 음원·샘플·이미지를 허락 없이 사용한 것으로 보여요.' },
  { id: 'sexual', label: '선정적 콘텐츠', text: '선정적인 내용이 포함돼 배급할 수 없어요.' },
  { id: 'harmful', label: '폭력·혐오·불법 콘텐츠', text: '폭력·혐오·차별·불법 행위를 담고 있어 배급할 수 없어요.' },
  { id: 'cover', label: '앨범 커버 문제', text: '앨범 커버가 배급 기준에 맞지 않아요(선정성·무단 이미지·로고·텍스트 규정 등).' },
  { id: 'quality', label: '음원 품질 미달', text: '음원 품질이 배급 기준에 미치지 못해요(잡음·손상·무음·저음질).' },
  { id: 'metadata', label: '허위·오기재 정보', text: '아티스트명·곡명 등 발매 정보가 사실과 다르거나 오해를 줄 수 있어요.' },
  { id: 'impersonation', label: '아티스트 사칭', text: '다른 아티스트를 사칭하거나 혼동을 줄 수 있는 표기예요.' },
  { id: 'spam', label: '반복·스팸성 발매', text: '같은 내용을 반복하거나 검색 노출만을 노린 발매로 판단돼요.' },
  { id: 'ai', label: 'AI 생성물 정책 위반', text: 'AI 생성물 관련 배급 정책에 맞지 않아요.' },
  { id: 'policy', label: '내부 규정 위반', text: 'AUDENIQ 배급 운영 규정에 맞지 않아요.' },
];

// ---------- 시스템 검사 진행 (담당자가 볼 요약) ----------
export type StageState = 'done' | 'running' | 'staff' | 'fix' | 'wait' | 'stopped';
export interface Stage { key: string; label: string; hint: string; state: StageState }
const STAGE_STATE: Record<StageState, string> = {
  done: '통과', running: '진행 중', staff: '담당자 확인 필요', fix: '아티스트 보완 중', wait: '대기', stopped: '중단',
};
export const stageStateLabel = (s: StageState) => STAGE_STATE[s];

/** 발매 상태 → 1차(파일·음원) / 2차(권리·정보) / 3차(배급 준비) 진행 */
export function systemStages(status: string): Stage[] {
  const at: Record<string, StageState[]> = {
    SUBMITTED: ['running', 'wait', 'wait'],
    STAGE1_RUNNING: ['running', 'wait', 'wait'],
    STAGE1_CORRECTION: ['fix', 'wait', 'wait'],
    STAGE1_PASSED: ['done', 'running', 'wait'],
    STAGE2_RUNNING: ['done', 'running', 'wait'],
    STAGE2_REVIEW: ['done', 'staff', 'wait'],
    STAGE2_CORRECTION: ['done', 'fix', 'wait'],
    STAGE2_PASSED: ['done', 'done', 'running'],
    STAGE3_PREPARING: ['done', 'done', 'running'],
    STAGE3_CORRECTION: ['done', 'done', 'fix'],
    READY_FOR_DELIVERY: ['done', 'done', 'done'],
    ON_HOLD_RIGHTS: ['done', 'staff', 'wait'],
    WITHDRAWN: ['stopped', 'stopped', 'stopped'],
  };
  const s = at[status] ?? ['wait', 'wait', 'wait'];
  return [
    { key: 's1', label: '1차 검사', hint: '파일 형식·음질·음량·중복 음원', state: s[0] },
    { key: 's2', label: '2차 검사', hint: '권리·발매 정보·콘텐츠 신고', state: s[1] },
    { key: 's3', label: '3차 배급 준비', hint: '음반·음원 코드 발급·플랫폼별 전송 파일', state: s[2] },
  ];
}
