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

/** 발매 저장이 중간에 실패했다 — 발매·트랙 일부는 이미 서버에 반영됐다.
 *  화면은 releaseId·trackServerIds를 받아 다음 저장이 같은 발매·트랙을 이어서 쓰게 한다
 *  (새 발매를 또 만들거나, 방금 만든 트랙을 보관했다가 다시 만들지 않도록). */
export class PartialSaveError extends ApiError {
  releaseId: string;
  trackServerIds: Record<string, string>;
  constructor(cause: unknown, releaseId: string, trackServerIds: Record<string, string>, tracksSaved: boolean) {
    const reason = cause instanceof Error && cause.message ? cause.message : '잠시 후 다시 시도해 주세요.';
    super(
      tracksSaved ? `트랙 정보는 저장됐지만 발매 정보는 저장되지 않았어요. ${reason}` : `발매 정보를 저장하지 못했어요. ${reason}`,
      cause instanceof ApiError ? cause.status : 0,
      cause instanceof ApiError ? cause.code : '',
    );
    this.name = 'PartialSaveError';
    this.releaseId = releaseId;
    this.trackServerIds = trackServerIds;
  }
}

/** 백엔드 오류 코드(`{ error: { code } }`) → 사용자에게 보여줄 한국어 문구 */
const CODE_MESSAGES: Record<string, string> = {
  UNAUTHENTICATED: '로그인이 필요해요.',
  FORBIDDEN: '이 작업을 할 수 있는 권한이 없어요. 새로고침 후 다시 시도해 주세요.',
  NOT_FOUND: '요청한 정보를 찾을 수 없어요. 삭제됐거나 주소가 잘못됐을 수 있어요.',
  CONFLICT: '다른 곳에서 먼저 수정됐어요. 새로고침한 뒤 다시 저장해 주세요.',
  // 칸반 보드
  TASK_NO_REVIEWER: '검토 열로 옮기려면 먼저 검토자를 정해 주세요.',
  TASK_SUBTASKS_OPEN: '끝나지 않은 하위 업무가 있어 완료할 수 없어요.',
  TASK_ARCHIVED: '보관된 업무예요.',
  TASK_NOT_ON_BOARD: '보드에 없는 업무예요. 하위 업무는 상위 업무 카드에서 다뤄요.',
  COLUMN_NOT_ON_BOARD: '이 보드의 열이 아니에요.',
  NEIGHBOUR_INVALID: '옮길 위치를 찾지 못했어요. 새로고침한 뒤 다시 시도해 주세요.',
  SUBTASK_ON_BOARD: '하위 업무는 보드에 따로 놓을 수 없어요.',
  MOVE_TO_REQUIRED: '열에 카드가 있어요. 카드를 옮길 열을 골라 주세요.',
  MOVE_TO_INVALID: '카드를 옮길 열을 다시 골라 주세요.',
  COLUMN_ORDER_INVALID: '열 목록이 바뀌었어요. 새로고침한 뒤 다시 정렬해 주세요.',
  TOO_MANY_COLUMNS: '열은 20개까지 만들 수 있어요.',
  BOARD_NEEDS_COLUMN: '보드에는 열이 하나 이상 있어야 해요.',
  BOARD_DEFAULT: '기본 보드는 보관할 수 없어요.',
  BOARD_ARCHIVED: '보관된 보드예요.',
  BOARD_NOT_FOUND: '보드를 찾지 못했어요.',
  TOO_MANY_BOARDS: '보드를 더 만들 수 없어요. 쓰지 않는 보드를 보관해 주세요.',
  COLUMN_EXISTS: '같은 이름의 열이 이미 있어요.',
  COLUMN_NOT_FOUND: '열을 찾지 못했어요.',
  NEIGHBOUR_NOT_IN_COLUMN: '옮길 위치가 바뀌었어요. 새로고침한 뒤 다시 시도해 주세요.',
  WIP_LIMIT_RANGE: 'WIP 한도는 1–500 사이로 정해 주세요.',
  ROLE_REQUIRED: '팀 보드는 담당 역할을 정해야 해요.',
  // 문서
  DOC_CONTENT_INVALID: '문서 내용에 저장할 수 없는 부분이 있어요.',
  DOC_ATTR_INVALID: '문서 블록 설정에 저장할 수 없는 값이 있어요.',
  DOC_MARK_INVALID: '지원하지 않는 서식이 있어요.',
  DOC_LINK_INVALID: 'http(s) 또는 mailto 링크만 넣을 수 있어요.',
  DOC_EMBED_INVALID: '넣을 수 없는 AUDENIQ 블록이 있어요.',
  DOC_TOO_LARGE: '문서가 너무 커요. 나눠서 작성해 주세요.',
  DOC_TOO_DEEP: '문서는 5단계까지만 하위로 둘 수 있어요.',
  DOC_MOVE_CYCLE: '문서를 자기 하위 문서 아래로 옮길 수 없어요.',
  DOC_ARCHIVED: '보관된 문서예요. 복원한 뒤 다시 시도해 주세요.',
  DOC_NOT_ARCHIVED: '보관한 문서만 삭제할 수 있어요.',
  DOC_SELF_APPROVAL: '검토를 요청한 사람은 직접 승인할 수 없어요.',
  DOC_STATUS_INVALID: '지금 상태에서는 할 수 없는 작업이에요.',
  DOC_ICON_INVALID: '아이콘은 이모지 하나로 정해 주세요.',
  TEMPLATE_HAS_FILES: '파일이 들어간 문서는 템플릿으로 만들 수 없어요.',
  TEMPLATE_NOT_FOUND: '템플릿을 찾지 못했어요.',
  ATTACHMENT_NOT_READY: '파일 업로드가 끝나지 않았거나 쓸 수 없는 파일이에요.',
  GRANT_DUPLICATE: '같은 대상에 권한이 두 번 들어갔어요.',
  PRINCIPAL_UNKNOWN: '권한 대상을 찾지 못했어요.',
  NOTE_PARENT_INVALID: '답글은 첫 댓글에만 달 수 있어요.',
  NOTE_ANCHOR_INVALID: '댓글 위치 정보가 올바르지 않아요.',
  EMBED_VIEW_UNKNOWN: '지원하지 않는 블록 보기예요.',
  TOO_MANY_EMBEDS: '한 번에 불러올 블록이 너무 많아요.',
  CURRENT_PASSWORD_INVALID: '지금 비밀번호가 맞지 않아요.',
  PASSKEY_INVALID: '패스키를 확인하지 못했어요. 다시 시도해 주세요.',
  PASSKEY_CHALLENGE_INVALID: '시간이 지났어요. 다시 시도해 주세요.',
  PASSKEY_ALREADY_REGISTERED: '이미 등록한 패스키예요.',
  PASSKEY_LIMIT: '패스키는 10개까지 등록할 수 있어요. 쓰지 않는 패스키를 먼저 삭제해 주세요.',
  OAUTH_CANCELLED: '로그인을 취소했어요.',
  OAUTH_STATE_INVALID: '로그인 시간이 지났어요. 다시 시도해 주세요.',
  OAUTH_EMAIL_REQUIRED: '확인된 이메일을 받지 못했어요. 이메일 공유를 허용하거나 이메일로 가입해 주세요.',
  OAUTH_EMAIL_IN_USE: '이 이메일로 가입한 계정이 이미 있어요. 이메일과 비밀번호로 로그인한 뒤 계정 정보 > 로그인에서 연결해 주세요.',
  OAUTH_SIGNUP_REQUIRED: '처음 오셨네요. 아래 이용약관에 동의한 뒤 다시 Google·Apple로 계속하기를 눌러 주세요.',
  OAUTH_ALREADY_LINKED: '이 계정은 이미 다른 AUDENIQ 계정에 연결돼 있어요.',
  OAUTH_PROVIDER_LINKED: '이미 같은 서비스의 계정을 연결했어요. 먼저 연결을 해제해 주세요.',
  OAUTH_LAST_SIGN_IN_METHOD: '로그인할 다른 방법이 없어 연결을 해제할 수 없어요. 비밀번호를 정하거나 패스키를 먼저 등록해 주세요.',
  OAUTH_PROVIDER_NOT_CONFIGURED: '지금은 이 로그인 방법을 쓸 수 없어요.',
  OAUTH_PROVIDER_UNAVAILABLE: '로그인 서비스에 연결하지 못했어요. 잠시 후 다시 시도해 주세요.',
  OAUTH_CODE_REJECTED: '로그인을 확인하지 못했어요. 다시 시도해 주세요.',
  OAUTH_TOKEN_INVALID: '로그인을 확인하지 못했어요. 다시 시도해 주세요.',
  OAUTH_FAILED: '로그인하지 못했어요. 다시 시도해 주세요.',
  ACCOUNT_NOT_ACTIVE: '이용이 제한된 계정이에요. 고객센터로 문의해 주세요.',
  PASSWORD_TOO_WEAK: '너무 흔하거나 추측하기 쉬운 비밀번호예요. 흔한 단어·연속된 글자·반복·이메일 아이디가 들어가지 않게 다른 비밀번호를 정해 주세요.',
  INVARIANT_CONFLICT: '다른 곳에서 먼저 수정됐어요. 새로고침한 뒤 다시 시도해 주세요.',
  INVALID_INPUT: '입력값을 확인해 주세요. 빠진 항목이나 형식이 맞지 않는 값이 있어요.',
  RELEASE_BUSY: '지금 자동 검사가 진행 중이에요. 잠시 후 다시 취소해 주세요.',
  // 서버가 보내지만 문구가 없어 '요청을 처리하지 못했어요. (코드)'로만 보이던 것들
  SIGNATURE_INVALID: '서명 이미지를 확인하지 못했어요. 서명을 지우고 다시 그린 뒤 시도해 주세요.',
  EMAIL_INVALID: '이메일 형식을 확인해 주세요.',
  ACCOUNT_NUMBER_INVALID: '계좌번호를 확인해 주세요. 숫자만 입력해 주세요.',
  PAYOUT_ACCOUNT_REQUIRED: '정산 계좌를 먼저 등록해 주세요.',
  PAYOUT_EXCEEDS_BALANCE: '요청 가능 금액 안에서 입력해 주세요.',
  PAYEE_ON_HOLD: '지급이 잠시 보류된 계정이에요. 문의로 알려 주시면 확인해 드릴게요.',
  APPLICATION_DOCUMENT_FINAL: '이미 확정된 신청서라 다시 기록할 수 없어요.',
  APPLICATION_HASH_INVALID: '신청서 내용을 확인하지 못했어요. 새로고침한 뒤 다시 접수해 주세요.',
  APPLICATION_NO_INVALID: '신청서 번호를 확인하지 못했어요. 새로고침한 뒤 다시 접수해 주세요.',
  DOCUMENT_ASSET_INVALID: '첨부한 서류 파일을 확인하지 못했어요. PDF·JPG·PNG 파일로 다시 올려 주세요.',
  DOCUMENT_NOT_EDITABLE: '지금은 이 서류를 바꿀 수 없어요. 검토가 끝난 뒤 다시 시도해 주세요.',
  ASSET_OBJECT_DRIFT: '올린 파일을 다시 확인해야 해요. 해당 파일을 다시 올린 뒤 접수해 주세요.',
  UPLOAD_REINSPECTION_REQUIRED: '파일 안전 검사를 다시 해야 해요. 해당 파일을 다시 올려 주세요.',
  UPLOAD_SANITIZATION_FAILED: '파일을 안전하게 처리하지 못했어요. 다른 파일로 다시 올려 주세요.',
  UPLOAD_UNSAFE_FILE: '안전하지 않은 파일로 확인돼 올릴 수 없어요.',
  PAYOUT_NOT_CANCELLABLE: '담당자가 승인한 출금은 취소할 수 없어요. 문의로 알려 주세요.',
  RELEASE_NOT_WITHDRAWABLE: '이 상태에서는 발매 신청을 취소할 수 없어요.',
  RELEASE_ALREADY_DELIVERING: '계약서에 서명해 배급이 시작된 발매예요. 취소(테이크다운)는 문의로 요청해 주세요.',
  WITHDRAW_LIMIT_REACHED: '이번 달 직접 취소할 수 있는 3회를 모두 썼어요. 문의로 취소를 요청해 주세요.',
  RATE_LIMITED: '시도가 너무 많아요. 몇 분 뒤 다시 시도해 주세요.',
  SIGNING_REQUEST_REQUIRED: '권리 서류는 권리자가 본인확인 후 직접 서명해야 해요. 서명 요청을 보내 주세요.',
  SIGNING_REQUEST_CLOSED: '이미 서명됐거나 거절·취소된 요청이에요.',
  SIGNING_LINK_EXPIRED: '서명 링크의 유효 기간이 지났어요. 요청한 분께 새 링크를 받아 주세요.',
  SIGNING_CONSENT_REQUIRED: '문서 확인, 전자서명, 개인정보 수집·이용에 모두 동의해야 서명할 수 있어요.',
  IDENTITY_PROVIDER_NOT_CONFIGURED: '본인확인 서비스를 준비하고 있어요. 준비되기 전에는 전자 서명을 할 수 없어요.',
  IDENTITY_NOT_VERIFIED: '본인확인이 끝나지 않았어요. 다시 시도해 주세요.',
  IDENTITY_NAME_MISMATCH: '본인확인한 이름이 문서의 서명자 이름과 달라요. 서명자 본인이 확인해 주세요.',
  IDENTITY_EXPIRED: '본인확인한 지 30분이 지났어요. 다시 본인확인한 뒤 서명해 주세요.',
  IDENTITY_ACCOUNT_MISMATCH: '이 계정은 다른 분 명의로 본인인증이 되어 있어요. 계정 주인만 다시 인증할 수 있어요.',
  SMS_PROVIDER_NOT_CONFIGURED: '문자 인증 서비스를 준비하고 있어요. 계정 도움이 필요하면 고객지원에 문의해 주세요.',
  PHONE_INVALID: '휴대폰 번호를 확인해 주세요. (예: 010-1234-5678)',
  SMS_CODE_INVALID: '인증번호가 맞지 않거나 시간이 지났어요. 인증번호를 다시 받아 주세요.',
  RESET_CODE_INVALID: '인증번호가 맞지 않거나 시간이 지났어요. 이메일·휴대폰 번호를 확인하고 인증번호를 다시 받아 주세요.',
  RESET_TOKEN_INVALID: '비밀번호 재설정 시간이 지났거나 이미 사용했어요. 처음부터 다시 진행해 주세요.',
  TAX_COUNTRY_INVALID: '납세 의무가 있는 국가를 골라 주세요.',
  TIN_INVALID: '납세자번호는 영문·숫자 4~30자로 입력해 주세요.',
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
  UPLOAD_CONTENT_MISMATCH: '파일 내용이 확장자와 달라요. 원본 WAV·FLAC·ALAC·AIFF·WavPack·TTA(커버는 JPG·PNG) 파일을 올려 주세요.',
  UPLOAD_LOSSY_NOT_ACCEPTED: 'AAC 같은 손실 압축 음원은 배급할 수 없어요. 무손실 원본(WAV·FLAC·ALAC·AIFF·WavPack·TTA)으로 올려 주세요.',
  UPLOAD_CONVERSION_FAILED: '음원 파일을 FLAC으로 바꾸지 못했어요. 파일을 다시 내보내거나 WAV·FLAC으로 올려 주세요.',
  UPLOAD_CONVERSION_NOT_LOSSLESS: '음원 파일을 무손실로 변환하지 못했어요. WAV·FLAC 원본으로 올려 주세요.',
  UPLOAD_CONVERSION_TIMEOUT: '음원 변환이 너무 오래 걸렸어요. 잠시 후 다시 시도하거나 WAV·FLAC으로 올려 주세요.',
  UPLOAD_CONVERSION_UNAVAILABLE: '지금은 음원 변환을 할 수 없어요. 잠시 후 다시 시도해 주세요.',
  UPLOAD_BUSY: '다른 음원을 처리 중이에요. 잠시 후 등록을 다시 시도해 주세요.',
  UPLOAD_PARTS_INCOMPLETE: '음원 일부가 아직 올라가지 않았어요. 같은 파일을 다시 선택하면 이어서 올려요.',
  UPLOAD_PARTS_INVALID: '업로드 조각 요청이 올바르지 않아요. 파일을 다시 선택해 주세요.',
  UPLOAD_NOT_RESUMABLE: '이어 올릴 수 없는 업로드예요. 파일을 다시 선택해 주세요.',
  UPLOAD_AUDIO_TRUNCATED: 'WAV 파일이 끝까지 저장되지 않았어요. 마스터를 다시 내보내 전체 파일을 올려 주세요.',
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
  DSP_SELECTION_REQUIRED: '배급할 플랫폼을 하나 이상 선택해 주세요.',
  TERRITORIES_INVALID: '배급 지역을 다시 골라 주세요. 전 세계 배급이 아니면 지역을 하나 이상 선택해야 해요.',
  DSP_SELECTION_INVALID: '플랫폼 선택을 다시 확인해 주세요.',
  DSP_UNAVAILABLE: '선택한 플랫폼 중 현재 배급할 수 없는 곳이 있어요. 플랫폼 선택을 다시 확인해 주세요.',
  SPECIAL_DECLARATION_MISMATCH: '커버곡·샘플링·AI 사용 신고를 다시 확인해 주세요.',
  EXPRESS_ACK_REQUIRED: '신속 발매 안내를 확인해 주세요.',
  AI_DETAILS_REQUIRED: 'AI 도구와 활용 방식을 입력해 주세요.',
  COVER_DETAILS_REQUIRED: '커버곡 원곡 정보와 권리 확인을 입력해 주세요.',
  NOT_IMPLEMENTED: '아직 준비 중인 기능이에요.',
  BACKEND_UNAVAILABLE: '서버에 연결하지 못했어요. 잠시 후 다시 시도해 주세요.',
  MAINTENANCE: '지금은 서버 점검 중이에요. 점검이 끝나면 다시 이용할 수 있어요.',
  // 플레이리스트 피칭
  PITCH_INVALID: '곡 소개는 20–1000자로, 다른 항목은 200자 안으로 적어 주세요.',
  PITCH_TOO_LATE: '피칭은 발매일 7일 전까지만 할 수 있어요. 에디터가 미리 플레이리스트를 준비하거든요.',
  PITCH_ALREADY_SUBMITTED: '이 발매는 이미 피칭했어요. 결과를 기다려 주세요.',
  PITCH_RELEASE_NOT_SUBMITTED: '발매 신청을 마친 뒤에 피칭할 수 있어요.',
  PITCH_NOT_WITHDRAWABLE: '이미 플랫폼에 전달돼 취소할 수 없어요.',
  PITCH_STATUS_INVALID: '바꿀 수 없는 상태예요.',
  PITCH_PLATFORMS_REQUIRED: '전달한 플랫폼을 하나 이상 골라 주세요.',
  PITCH_RESULT_REQUIRED: '선정된 플레이리스트를 적어 주세요.',
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

/** 로그인·회원가입 화면 전용 문구. 일반 API 문구("로그인이 필요해요", "입력값을 확인해 주세요")는 이 화면에서 뜻이 맞지 않아요. */
const AUTH_MESSAGES: Record<'login' | 'signup', Record<string, string>> = {
  login: {
    UNAUTHENTICATED: '이메일 또는 비밀번호가 맞지 않아요. 다시 확인해 주세요.',
    INVALID_INPUT: '이메일 또는 비밀번호가 맞지 않아요. 다시 확인해 주세요.',
    FORBIDDEN: '보안 확인에 실패했어요. 페이지를 새로고침한 뒤 다시 로그인해 주세요.',
    RATE_LIMITED: '로그인 시도가 너무 많아요. 몇 분 뒤 다시 시도해 주세요.',
  },
  signup: {
    INVALID_INPUT: '이메일 형식과 비밀번호(12자 이상)를 확인해 주세요.',
    CONFLICT: '이미 가입된 이메일이에요. 로그인하거나 비밀번호 찾기를 이용해 주세요.',
    FORBIDDEN: '보안 확인에 실패했어요. 페이지를 새로고침한 뒤 다시 시도해 주세요.',
    RATE_LIMITED: '가입 시도가 너무 많아요. 몇 분 뒤 다시 시도해 주세요.',
  },
};

export function authErrorMessage(e: unknown, mode: 'login' | 'signup'): string {
  const code = e instanceof ApiError ? e.code : '';
  return AUTH_MESSAGES[mode][code] ?? errorMessage(e, mode === 'login' ? '로그인하지 못했어요. 잠시 후 다시 시도해 주세요.' : '회원가입하지 못했어요. 잠시 후 다시 시도해 주세요.');
}
