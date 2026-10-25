// 문의 주제 — 유형마다 자주 오는 문의를 주제로 나누고, 주제마다 담당자가 바로 답할 수 있게 필요한 것만 묻는다.
// 계정·정산처럼 발매와 상관없는 주제는 '관련 발매'를 묻지 않고, 발매 수정·취소는 발매를 꼭 고르게 한다.
// 서버는 그대로 (유형·발매·제목·내용) — 고른 주제와 칸은 제목 꼬리표와 내용 맨 위 줄로 들어간다 (crates/core/src/portal.rs create_inquiry).

/** 발매 상태로 고를 수 있는 발매 거르기 — 신청한 것(작성 중 제외) · 아직 발매 전 · 발매된 것 */
export type ReleaseNeed = 'none' | 'optional' | 'required';
export type ReleaseScope = 'submitted' | 'before_live' | 'live';

export interface TopicField {
  key: string; label: string;
  kind: 'text' | 'date' | 'month' | 'select' | 'url';
  options?: readonly string[];
  placeholder?: string;
  required?: boolean;
}

export interface Topic {
  key: string; label: string;
  release: ReleaseNeed;
  /** 고를 수 있는 발매 (release가 none이 아니면) */
  scope?: ReleaseScope;
  fields: readonly TopicField[];
  /** 내용 칸 이름·예시 — 빈칸이면 '내용' */
  body?: string;
  bodyPlaceholder?: string;
  /** 칸을 채웠어도 설명은 꼭 받는다 */
  bodyRequired?: boolean;
  /** 문의하지 않고 바로 할 수 있는 곳 — 담당자를 기다리지 않게 먼저 알려 준다 */
  shortcut?: { text: string; to: string; label: string };
}

export const IDENTITY_CATEGORY = '본인인증·실명인증';

const PLATFORMS = ['멜론', '지니', '벅스', 'FLO', '바이브', 'Spotify', 'Apple Music', 'YouTube Music', '기타'] as const;

export const TOPICS: Record<string, readonly Topic[]> = {
  '발매·심사': [
    { key: 'status', label: '심사 진행 상황', release: 'required', scope: 'submitted', fields: [],
      bodyPlaceholder: '궁금한 점을 적어 주세요. 예: 발매일까지 심사가 끝날 수 있을까요?',
      shortcut: { text: '진행 단계와 담당자 의견은 발매 상세에서 바로 볼 수 있어요.', to: '/releases', label: '발매 보기' } },
    { key: 'fix', label: '보완 요청 문의', release: 'required', scope: 'submitted',
      fields: [{ key: 'item', label: '이해가 안 되는 보완 항목', kind: 'text', placeholder: '예: 커버아트 문구', required: true }],
      bodyPlaceholder: '어떤 부분이 이해가 안 되는지, 어떻게 고치려고 하는지 적어 주세요.', bodyRequired: true },
    { key: 'date', label: '발매일 바꾸기', release: 'required', scope: 'before_live',
      fields: [{ key: 'date', label: '바꿀 발매일', kind: 'date', required: true }, { key: 'time', label: '공개 시각 (있으면)', kind: 'text', placeholder: '예: 18:00' }],
      bodyPlaceholder: '바꾸는 이유가 있으면 적어 주세요.' },
    { key: 'missing', label: '발매됐는데 플랫폼에 안 보여요', release: 'required', scope: 'live',
      fields: [
        { key: 'platform', label: '안 보이는 플랫폼', kind: 'select', options: PLATFORMS, required: true },
        { key: 'searched', label: '검색한 이름이나 링크', kind: 'text', placeholder: '예: 서린 여름의 끝' },
      ],
      bodyPlaceholder: '언제부터 안 보였는지 적어 주세요. 플랫폼마다 발매 후 반영까지 하루 이틀 걸릴 수 있어요.' },
    { key: 'cancel', label: '발매 신청 취소', release: 'required', scope: 'before_live',
      fields: [{ key: 'reason', label: '취소 사유', kind: 'select', options: ['정보를 잘못 넣었어요', '일정이 바뀌었어요', '다른 곳에서 발매해요', '기타'], required: true }],
      bodyPlaceholder: '더 전할 내용이 있으면 적어 주세요.' },
    { key: 'other', label: '그 밖의 발매 문의', release: 'optional', scope: 'submitted', fields: [], bodyRequired: true },
  ],
  '수정·테이크다운': [
    { key: 'edit', label: '발매 정보 수정', release: 'required', scope: 'submitted',
      fields: [
        { key: 'field', label: '고칠 항목', kind: 'select', options: ['제목', '아티스트명', '참여진·크레딧', '가사', '커버아트', '장르', '발매일', '기타'], required: true },
        { key: 'from', label: '지금 값', kind: 'text' },
        { key: 'to', label: '바꿀 값', kind: 'text', required: true },
      ],
      bodyPlaceholder: '곡이 여러 개면 어느 곡인지, 바꾸는 이유를 적어 주세요.' },
    { key: 'takedown', label: '음원 내리기 (테이크다운)', release: 'required', scope: 'live',
      fields: [
        { key: 'where', label: '내릴 곳', kind: 'select', options: ['모든 플랫폼', '일부 플랫폼'], required: true },
        { key: 'platforms', label: '일부라면 어느 플랫폼', kind: 'text', placeholder: '예: 멜론, 지니' },
        { key: 'reason', label: '사유', kind: 'select', options: ['재발매·리마스터', '권리 문제', '계약 종료', '기타'], required: true },
      ],
      bodyPlaceholder: '원하는 날짜가 있으면 적어 주세요. 플랫폼마다 내려가기까지 며칠 걸려요.' },
    { key: 'page', label: '다른 아티스트 페이지에 올라갔어요', release: 'required', scope: 'live', fields: [],
      shortcut: { text: '아티스트 페이지 정리는 플랫폼 요청으로 보내면 담당자가 플랫폼에 바로 접수해요.', to: '/inquiries?new=request&group=profile', label: '플랫폼 요청으로' } },
  ],
  '정산·지급': [
    { key: 'amount', label: '정산 금액 확인', release: 'optional', scope: 'live',
      fields: [{ key: 'month', label: '정산 월', kind: 'month', required: true }],
      bodyPlaceholder: '예상과 다른 부분을 적어 주세요. 예: 멜론 스트리밍이 0원으로 나와요.',
      shortcut: { text: '월별·플랫폼별 금액은 정산·지급에서 바로 볼 수 있어요.', to: '/settlement', label: '정산 보기' } },
    { key: 'payout', label: '지급이 안 됐어요', release: 'none',
      fields: [{ key: 'month', label: '지급 받을 정산 월', kind: 'month', required: true }],
      bodyPlaceholder: '지급 예정일과 받은 안내가 있으면 적어 주세요.' },
    { key: 'account', label: '정산 계좌·예금주', release: 'none', fields: [],
      bodyPlaceholder: '예: 예금주가 본인인증 이름과 달라요.',
      shortcut: { text: '계좌 등록·변경은 계정 정보에서 바로 할 수 있어요.', to: '/profile?open=payout', label: '계좌 바꾸기' } },
    { key: 'tax', label: '세금·증빙 서류', release: 'none',
      fields: [
        { key: 'doc', label: '필요한 서류', kind: 'select', options: ['원천징수영수증', '지급명세서', '해외 납세 서류', '기타'], required: true },
        { key: 'period', label: '기간', kind: 'text', placeholder: '예: 2026년 1~6월' },
      ] },
  ],
  '계약·권리': [
    { key: 'contract', label: '계약서·서명', release: 'optional', scope: 'submitted', fields: [],
      shortcut: { text: '받은 계약서와 서명 상태는 계약서에서 바로 볼 수 있어요.', to: '/contracts', label: '계약서 보기' } },
    { key: 'proof', label: '권리·보완 서류', release: 'optional', scope: 'submitted', fields: [],
      shortcut: { text: '요청받은 서류는 권리·보완 서류에서 바로 올릴 수 있어요.', to: '/rights', label: '서류 올리기' } },
    { key: 'dispute', label: '다른 사람이 내 음원을 썼어요', release: 'optional', scope: 'live',
      fields: [{ key: 'url', label: '문제의 링크', kind: 'url', placeholder: 'https://', required: true }],
      bodyPlaceholder: '누가, 어디에, 어떻게 썼는지 적어 주세요.', bodyRequired: true },
    { key: 'sample', label: '샘플·커버곡 허가', release: 'optional', scope: 'submitted',
      fields: [{ key: 'original', label: '원곡 (곡명 · 아티스트)', kind: 'text', required: true }],
      bodyPlaceholder: '어떻게 쓰는지(샘플·커버·리믹스), 원작자 허락을 받았는지 적어 주세요.' },
  ],
  [IDENTITY_CATEGORY]: [
    { key: 'identity', label: '고객센터 실명인증', release: 'none',
      fields: [
        { key: 'why', label: '휴대폰 인증이 안 되는 이유', kind: 'select', options: ['본인 명의 휴대폰이 없어요', '만 19세 미만이에요', '외국인이에요', '인증 중 오류가 나요', '기타'], required: true },
        { key: 'when', label: '연락 가능한 시간', kind: 'text', placeholder: '예: 평일 오후 2~6시' },
      ] },
  ],
  '계정·기타': [
    { key: 'login', label: '로그인·보안', release: 'none',
      fields: [{ key: 'method', label: '로그인 방법', kind: 'select', options: ['이메일·비밀번호', 'Google', 'Apple', '패스키'], required: true }],
      bodyPlaceholder: '어떤 화면에서 무슨 안내가 나오는지 적어 주세요.', bodyRequired: true },
    { key: 'team', label: '팀원·권한', release: 'none', fields: [],
      shortcut: { text: '팀원 초대와 권한은 계정 정보 > 팀에서 바로 바꿀 수 있어요.', to: '/profile', label: '계정 정보' } },
    { key: 'profile', label: '계정 정보 바꾸기', release: 'none',
      fields: [{ key: 'field', label: '바꿀 정보', kind: 'select', options: ['로그인 이메일', '실명', '활동명', '기타'], required: true }],
      bodyPlaceholder: '지금 값과 바꿀 값을 적어 주세요.' },
    { key: 'leave', label: '탈퇴', release: 'none', fields: [],
      bodyPlaceholder: '탈퇴하려는 이유를 알려 주시면 도움이 돼요. 발매된 음원과 남은 정산금은 담당자가 함께 정리해 드려요.' },
    { key: 'other', label: '그 밖의 문의', release: 'optional', scope: 'submitted', fields: [], bodyRequired: true },
  ],
};

export const topicsOf = (category: string): readonly Topic[] => TOPICS[category] ?? [];
export const findTopic = (category: string, key: string | undefined): Topic =>
  topicsOf(category).find(t => t.key === key) ?? topicsOf(category)[0];

/** 이 주제에서 고를 수 있는 발매 */
export function releaseFits(scope: ReleaseScope | undefined, status: string): boolean {
  if (status === 'draft') return false;
  if (scope === 'before_live') return !['live', 'closed', 'rejected', 'cancelled'].includes(status);
  if (scope === 'live') return status === 'live' || status === 'closed';
  return true;
}

/** 빠진 필수 칸 — 첫 칸 이름, 다 채웠으면 null */
export function missingField(t: Topic, values: Record<string, string>, body: string, releaseId: string): string | null {
  if (t.release === 'required' && !releaseId) return '관련 발매';
  for (const f of t.fields) if (f.required && !(values[f.key] ?? '').trim()) return f.label;
  const badUrl = t.fields.find(f => f.kind === 'url' && (values[f.key] ?? '').trim() && !/^https?:\/\/\S+$/i.test((values[f.key] ?? '').trim()));
  if (badUrl) return badUrl.label;
  const anyField = t.fields.some(f => (values[f.key] ?? '').trim());
  if ((t.bodyRequired || !anyField) && !body.trim()) return t.body ?? '내용';
  return null;
}
/** 서버에 보낼 제목·내용 — 제목은 '[주제] 발매 제목', 내용은 고른 칸을 맨 위에 한 줄씩 */
export function composeInquiry(t: Topic, values: Record<string, string>, body: string, releaseTitle: string, subject: string): { subject: string; body: string } {
  const head = `[${t.label}]`;
  const rest = subject.trim() || releaseTitle.trim();
  const lines = t.fields
    .map(f => [f.label, (values[f.key] ?? '').trim()] as const)
    .filter(([, v]) => v)
    .map(([k, v]) => `${k}: ${v}`);
  return {
    subject: (rest ? `${head} ${rest}` : head).slice(0, 200),
    body: [lines.join('\n'), body.trim()].filter(Boolean).join('\n\n').slice(0, 4000),
  };
}
