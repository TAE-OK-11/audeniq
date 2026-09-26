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
  UPLOAD_TYPE_UNSUPPORTED: '지원하지 않는 파일 형식이에요. 음원은 WAV·FLAC, 커버는 JPG·PNG만 올릴 수 있어요.',
  UPLOAD_EMPTY: '빈 파일이에요. 파일을 다시 확인해 주세요.',
  UPLOAD_AUDIO_TOO_LARGE: '음원 파일이 너무 커요. 최대 512MB까지 올릴 수 있어요.',
  UPLOAD_IMAGE_TOO_LARGE: '커버 이미지가 너무 커요. 최대 20MB까지 올릴 수 있어요.',
  UPLOAD_CONTENT_MISMATCH: '파일 내용이 확장자와 달라요. 원본 WAV·FLAC(커버는 JPG·PNG) 파일을 올려 주세요.',
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
};

export function messageForCode(code: string, status = 0): string {
  return CODE_MESSAGES[code] ?? (status >= 500 ? CODE_MESSAGES.INTERNAL_ERROR : `요청을 처리하지 못했어요. (${code || status})`);
}

/** 사용자에게 보여줄 오류 문구 */
export function errorMessage(e: unknown, fallback = '문제가 생겼어요. 잠시 후 다시 시도해 주세요.'): string {
  return e instanceof Error && e.message ? e.message : fallback;
}
