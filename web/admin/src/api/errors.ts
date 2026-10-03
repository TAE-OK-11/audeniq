export class ApiError extends Error {
  status: number;
  code: string;
  constructor(message: string, status = 0, code = '') {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }
}

/** 백엔드 오류 코드(`{ error: { code } }`) → 사용자에게 보여줄 한국어 문구 */
const CODE_MESSAGES: Record<string, string> = {
  UNAUTHENTICATED: '로그인이 필요해요.',
  FORBIDDEN: '이 작업을 할 수 있는 권한이 없어요. 새로고침 후 다시 시도해 주세요.',
  NOT_FOUND: '요청한 정보를 찾을 수 없어요. 삭제됐거나 주소가 잘못됐을 수 있어요.',
  CONFLICT: '다른 곳에서 먼저 수정됐어요. 새로고침한 뒤 다시 저장해 주세요.',
  INVARIANT_CONFLICT: '다른 곳에서 먼저 수정됐어요. 새로고침한 뒤 다시 시도해 주세요.',
  INVALID_INPUT: '입력값을 확인해 주세요. 빠진 항목이나 형식이 맞지 않는 값이 있어요.',
  RATE_LIMITED: '시도가 너무 많아요. 몇 분 뒤 다시 시도해 주세요.',
  STORAGE_UNAVAILABLE: '파일 저장소에 잠시 연결할 수 없어요. 잠시 후 다시 시도해 주세요.',
  DATABASE_UNAVAILABLE: '서버가 잠시 바빠요. 잠시 후 다시 시도해 주세요.',
  INTERNAL_ERROR: '서버에서 문제가 생겼어요. 잠시 후 다시 시도해 주세요.',
  REQUEST_HEADERS_TOO_LARGE: '요청이 너무 커요. 새로고침 후 다시 시도해 주세요.',
  TEXT_INVALID_CHARACTERS: '입력한 글자 중 쓸 수 없는 문자(보이지 않는 제어 문자 등)가 있어요.',
  ARTIST_NAME_PROTECTED: '보호된 아티스트명이 포함돼 있어요. 본인 활동명인지 확인해 주세요.',
  IDENTIFIER_IN_USE: '이미 다른 발매에서 쓰고 있는 식별자(ISRC·UPC)예요.',
  UPLOAD_TYPE_UNSUPPORTED: '지원하지 않는 파일 형식이에요. 음원은 WAV·FLAC·ALAC(.m4a)·AIFF·WavPack·TTA, 커버는 JPG·PNG만 올릴 수 있어요.',
  UPLOAD_EMPTY: '빈 파일이에요. 파일을 다시 확인해 주세요.',
  UPLOAD_AUDIO_TOO_LARGE: '음원 파일이 너무 커요. 최대 512MB까지 올릴 수 있어요.',
  UPLOAD_IMAGE_TOO_LARGE: '커버 이미지가 너무 커요. 최대 20MB까지 올릴 수 있어요.',
  UPLOAD_CONTENT_MISMATCH: '파일 내용이 확장자와 달라요. 원본 WAV·FLAC·ALAC(커버는 JPG·PNG) 파일을 올려 주세요.',
  UPLOAD_LOSSY_NOT_ACCEPTED: 'AAC 같은 손실 압축 음원은 배급할 수 없어요. 무손실 원본(WAV·FLAC·ALAC)으로 올려 주세요.',
  UPLOAD_CONVERSION_FAILED: '음원 파일을 FLAC으로 바꾸지 못했어요. 파일을 다시 내보내거나 WAV·FLAC으로 올려 주세요.',
  UPLOAD_CONVERSION_NOT_LOSSLESS: '음원 파일을 무손실로 변환하지 못했어요. WAV·FLAC 원본으로 올려 주세요.',
  UPLOAD_CONVERSION_TIMEOUT: '음원 변환이 너무 오래 걸렸어요. 잠시 후 다시 시도하거나 WAV·FLAC으로 올려 주세요.',
  UPLOAD_CONVERSION_UNAVAILABLE: '지금은 음원 변환을 할 수 없어요. 잠시 후 다시 시도해 주세요.',
  UPLOAD_BUSY: '다른 음원을 처리 중이에요. 잠시 후 등록을 다시 시도해 주세요.',
  UPLOAD_AUDIO_FORMAT_UNSUPPORTED: '16·24bit, 44.1~192kHz, 모노·스테레오 무손실 음원으로 올려 주세요.',
  AUDIO_NOT_VERIFIED: '음원 품질 검사가 아직 끝나지 않았어요. 잠시 후 다시 접수해 주세요.',
  RELEASE_NOT_SUBMITTABLE: '지금 상태에서는 접수할 수 없는 발매예요.',
  MINORITY_REVIEW_REQUIRED: '미성년 아티스트 발매는 담당자 확인 후 접수할 수 있어요. 문의로 연락해 주세요.',
  DECLARATION_REQUIRED: '권리 확인과 연령 확인 항목에 모두 동의해 주세요.',
  CONSENT_NOT_FOUND: '동의 정보를 찾을 수 없어요. 다시 접수해 주세요.',
  CONSENT_EXPIRED: '동의 유효기간이 지났어요. 다시 접수해 주세요.',
  CONSENT_SCOPE_MISMATCH: '동의 후 발매 정보가 바뀌었어요. 다시 접수해 주세요.',
  CONSENT_POLICY_MISMATCH: '동의 약관이 새로 바뀌었어요. 다시 접수해 주세요.',
  SUBMIT_PERMISSION_REQUIRED: '이 작업 공간에서 발매를 접수할 권한이 없어요.',
  IDEMPOTENCY_KEY_REUSED: '이미 처리된 접수 요청이에요. 발매 상태를 확인해 주세요.',
  PREFLIGHT_FAILED: '접수 전 점검을 통과하지 못했어요. 표시된 항목을 보완해 주세요.',
  NOT_IMPLEMENTED: '아직 준비 중인 기능이에요.',
  BACKEND_UNAVAILABLE: '서버에 연결하지 못했어요. 잠시 후 다시 시도해 주세요.',
  MAINTENANCE: '지금은 서버 점검 중이에요. 점검이 끝나면 다시 이용할 수 있어요.',
  // 관리자(스태프) API
  STATUS_UNKNOWN: '알 수 없는 상태 필터예요.',
  NOTE_TOO_LONG: '메모가 너무 길어요. 줄여서 다시 입력해 주세요.',
  DECISION_REASON_REQUIRED: '결정 사유를 입력해 주세요.',
  DECISION_ACTION_UNKNOWN: '알 수 없는 결정이에요.',
  REVIEW_CLAIM_REQUIRED: '먼저 이 심사를 담당해 주세요. 담당자만 결정할 수 있어요.',
  REVIEW_CLAIMED_BY_OTHER: '다른 담당자가 맡은 심사예요. 담당자만 결정할 수 있어요.',
  AGREEMENT_TERMS_REQUIRED: '배급 계약 조건(수수료·배급 형태)을 먼저 입력해 주세요. 입력해야 아티스트가 계약서에 서명할 수 있어요.',
  AGREEMENT_NOT_IN_REVIEW: '검토 중인 배급 계약서가 없어요. 신청서가 접수됐는지 확인해 주세요.',
  AGREEMENT_CONFIRMATION_REQUIRED: '계약서의 필수 확인 항목에 모두 체크해 주세요.',
  RELEASE_NOT_IN_REVIEW: '이미 심사 대기 상태가 아닌 발매예요. 새로고침해 최신 상태를 확인해 주세요.',
  SECOND_APPROVAL_ALREADY_PENDING: '이 수정본에는 이미 2차 승인 요청이 올라가 있어요.',
  NOTHING_TO_CORRECT: '보완 요청할 미해결 검사 항목이 없어요.',
  APPROVAL_NOT_PENDING: '이미 처리됐거나 만료된 승인 요청이에요.',
  SECOND_APPROVER_MUST_DIFFER: '요청한 본인은 승인할 수 없어요. 다른 심사 담당자가 승인해야 해요.',
  REVIEW_NOTE_REQUIRED: '보완·보류 사유를 입력해 주세요.',
  DOCUMENT_STATUS_INVALID: '서류 처리 상태가 올바르지 않아요.',
  INQUIRY_CLOSED: '종료된 문의에는 답변할 수 없어요.',
  STAGING_SUPERSEDED: '새 패키지로 교체된 배급 건이에요. 목록을 새로고침해 주세요.',
  DELIVERY_CONTENT_BLOCKED: '콘텐츠 차단 항목이 있어 승인할 수 없어요.',
  WARNINGS_NOT_ACKNOWLEDGED: '음량·클리핑 권고를 확인했다고 체크해 주세요.',
  RELEASE_NOT_READY_FOR_DELIVERY: '배급 준비 완료 상태인 발매만 식별자를 재발급할 수 있어요.',
  NO_VIRTUAL_IDENTIFIERS: '임시(테스트) 식별자가 없어 재발급할 필요가 없어요.',
  REGISTERED_ISSUER_MISSING: '정식 UPC·ISRC 발급 범위가 아직 등록되지 않았어요.',
  PACKAGE_ALREADY_WITH_PARTNER: '이미 계약 파트너에게 전송된 패키지예요.',
};

export function messageForCode(code: string, status = 0): string {
  return CODE_MESSAGES[code] ?? (status >= 500 ? CODE_MESSAGES.INTERNAL_ERROR : `요청을 처리하지 못했어요. (${code || status})`);
}

/** 사용자에게 보여줄 오류 문구 */
export function errorMessage(e: unknown, fallback = '문제가 생겼어요. 잠시 후 다시 시도해 주세요.'): string {
  return e instanceof Error && e.message ? e.message : fallback;
}
