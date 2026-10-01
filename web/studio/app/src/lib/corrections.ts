// 보완 요청 코드 → 신청서(위자드)에서 고쳐야 할 단계와 입력칸.
// 서버 검사 코드(docs/API.md submission checks)와 담당자 요청 코드를 함께 다룬다.
import type { Correction } from '../api/types';

/** 위자드 단계 번호 (pages/Upload.tsx의 STEPS 순서) */
export const WIZ_STEP = { info: 0, tracks: 1, cover: 2, distribution: 3, rights: 4, review: 5 } as const;
export const WIZ_STEP_NAMES = ['발매 정보', '트랙 등록', '커버아트', '배급 설정', '권리 확인', '최종 확인'];

interface Target {
  step: number;
  /** 입력칸 id. `{k}`는 트랙 순번(0부터)으로 바뀐다 */
  field?: string;
  label: string;
  /** 서버가 설명을 주지 않을 때 보여줄 기본 안내 */
  hint: string;
}

const AUDIO: Omit<Target, 'hint'> = { step: WIZ_STEP.tracks, field: 'trackFile-{k}', label: '음원 파일' };
const COVER: Omit<Target, 'hint'> = { step: WIZ_STEP.cover, field: 'coverFile', label: '커버아트' };

const TARGETS: Record<string, Target> = {
  // 발매 정보
  TITLE: { step: WIZ_STEP.info, field: 'f-title', label: '발매 제목', hint: '발매 제목 표기를 확인해 주세요.' },
  ARTIST_NAME: { step: WIZ_STEP.info, field: 'f-artist', label: '아티스트명', hint: '아티스트명 표기를 확인해 주세요.' },
  ARTIST_NAME_PROTECTED: { step: WIZ_STEP.info, field: 'f-artist', label: '아티스트명', hint: '보호된 아티스트명과 겹쳐요. 본인 활동명인지 확인해 주세요.' },
  GENRE: { step: WIZ_STEP.info, field: 'f-genre', label: '장르', hint: '장르를 다시 선택해 주세요.' },
  FIELD_TITLE_MISSING: { step: WIZ_STEP.info, field: 'f-title', label: '발매 제목', hint: '발매 제목을 입력해 주세요.' },
  RELEASE_TITLE_STYLE: { step: WIZ_STEP.info, field: 'f-title', label: '발매 제목', hint: '발매 제목에 (Explicit)·(Clean) 같은 표기나 홍보 문구가 있어요. 19금 여부는 체크 항목으로만 표시해 주세요.' },
  FIELD_LANGUAGE_INVALID: { step: WIZ_STEP.info, field: 'f-language', label: '주요 언어', hint: '주요 언어를 목록에서 다시 선택해 주세요.' },
  FIELD_RELEASE_TYPE_MISMATCH: { step: WIZ_STEP.info, field: 'f-type', label: '발매 유형', hint: '트랙 수에 맞는 발매 유형(싱글·EP·정규)을 선택해 주세요.' },
  // 트랙·음원
  TRACK_REQUIRED: { step: WIZ_STEP.tracks, label: '트랙', hint: '트랙을 1곡 이상 등록해 주세요.' },
  TRACK_ORDER_GAP: { step: WIZ_STEP.tracks, label: '트랙 순서', hint: '트랙 순서에 빈 번호가 있어요. 트랙 목록을 확인해 주세요.' },
  TRACK_TITLE_DUPLICATE: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '같은 곡명이 여러 트랙에 있어요. 곡명을 구분해 주세요.' },
  TRACK_TITLE_HAS_VERSION_INFO: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '곡명에 버전 표기가 들어 있어요. 버전은 ‘버전’ 칸에 따로 입력해 주세요.' },
  TRACK_TITLE_EXPLICIT_MARKER: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '곡명에 (Explicit)·(19금) 같은 표기가 있어요. 곡명에서 빼고 19금 여부는 체크 항목으로 표시해 주세요.' },
  TRACK_TITLE_STYLE: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '곡명이 전부 대문자이거나 이모지·홍보 문구·feat. 표기가 들어 있어요. 피처링은 참여 아티스트 칸에 입력해 주세요.' },
  TRACK_TITLE_SEO_SPAM: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '곡명에 검색용 키워드나 불필요한 표기가 있어요. 곡명만 입력해 주세요.' },
  CREDIT_MISSING: { step: WIZ_STEP.tracks, field: 'tr-{k}-composers', label: '크레딧', hint: '크레딧이 비어 있어요. 작곡·작사 참여자를 입력해 주세요.' },
  TRACK_WRITER_CREDIT_MISSING: { step: WIZ_STEP.tracks, field: 'tr-{k}-composers', label: '작곡·작사', hint: '작곡가 또는 작사가를 한 명 이상 입력해 주세요.' },
  S2_META_CREDITS: { step: WIZ_STEP.tracks, field: 'tr-{k}-composers', label: '크레딧', hint: '작사·작곡 등 크레딧 정보가 비어 있거나 형식이 맞지 않아요.' },
  S2_LYRICS_CREDITS: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyricists', label: '작사 크레딧', hint: '가사가 있는 곡의 작사가를 입력해 주세요.' },
  S2_LYRICS_INSTRUMENTAL: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '가사·연주곡 표시', hint: '가사가 입력된 곡은 연주곡 표시를 해제하거나 가사를 확인해 주세요.' },
  S2_DSP_METADATA_CONTACT: { step: WIZ_STEP.info, field: 'f-title', label: '메타데이터 연락처', hint: '제목·아티스트 이름에 들어간 URL·이메일을 확인하고 광고·연락처 정보는 제거해 주세요.' },
  S2_DSP_METADATA_PROMOTION: { step: WIZ_STEP.info, field: 'f-title', label: '홍보 문구', hint: '제목의 발매 홍보·다운로드 유도 문구를 확인해 주세요.' },
  S2_DSP_ARTIST_GENERIC: { step: WIZ_STEP.info, field: 'f-artist', label: '아티스트 이름', hint: '장르·검색 키워드로만 된 이름인지 확인하고 실제 아티스트 이름을 입력해 주세요.' },
  S2_DSP_TITLE_FEATURED_ARTIST: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '피처링 표기', hint: 'Spotify에 보낼 곡명에서 feat. 표기를 빼고 참여 아티스트 칸에 입력해 주세요.' },
  S2_DSP_EXPLICIT_TAG_REVIEW: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '19금 표시 확인', hint: '가사에 강한 비속어 신호가 있어요. 실제 음원을 확인해 청소년 유해 표시를 검토해 주세요. 시스템이 표시를 임의로 변경하지 않아요.' },
  S2_DSP_CLEAN_TAG_CONFLICT: { step: WIZ_STEP.tracks, field: 'tr-{k}-version', label: 'Clean 버전 표시', hint: 'Clean 버전과 19금 표시가 충돌해요. 실제 녹음에 맞게 버전과 표시를 확인해 주세요.' },
  S2_DSP_LYRICS_FORMAT: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '가사 형식', hint: '가사에서 [Chorus]·반복 지시문과 불필요한 공백을 제거하고 반복되는 가사를 모두 써 주세요.' },
  S2_DSP_LYRICS_STYLE: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '가사 표기', hint: 'Apple 가이드에 맞게 줄 첫 글자와 줄 끝 마침표·쉼표를 확인해 주세요.' },
  S2_DSP_EMOJI_METADATA: { step: WIZ_STEP.info, field: 'f-title', label: '이모지', hint: 'Apple에 보낼 제목·아티스트 이름에서 이모지를 제거해 주세요.' },
  S2_DSP_TITLE_TECHNICAL_INFO: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명 부가 정보', hint: '곡명의 Official Audio·Lossless·Dolby Atmos 같은 부가 정보 표기를 제거해 주세요.' },
  S2_DSP_TRACK_COUNT_LIMIT: { step: WIZ_STEP.tracks, field: 'aqTracks', label: '트랙 수', hint: 'Apple의 앨범당 최대 500곡 제한에 맞게 발매를 나눠 주세요.' },
  S2_DSP_ARTWORK_INSPECTION_REQUIRED: { ...COVER, hint: '커버의 색상·문자·QR 검사를 완료해야 해요. 이전 심사 결과만으로 자동 승인할 수 없어요.' },
  S2_DSP_ARTWORK_QR: { ...COVER, hint: '커버에서 QR 코드가 확인됐어요. QR 코드를 제거한 커버를 올려 주세요.' },
  S2_DSP_ARTWORK_TEXT_REVIEW: { ...COVER, hint: 'OCR에서 연락처·홍보·가격·스토어·SNS·음질 표기 가능성이 확인됐어요. 원본 커버에서 실제 문구를 확인해 주세요.' },
  S2_DSP_SPOTIFY_ARTWORK_ENCODING: { ...COVER, hint: 'Spotify 커버는 24bit sRGB RGB로 내보내 주세요. 색 변환은 실제 픽셀에 적용하고 ICC 프로필·회전 정보는 제거해 주세요.' },
  S2_DSP_CONTENT_ID_DECLARATION: { step: WIZ_STEP.rights, field: 'aqContentIdRights', label: 'Content ID 권리 확인', hint: '신청 지역의 독점 권리와 원본 녹음을 확인하거나 Content ID 선택을 해제해 주세요.' },
  S2_DSP_CONTENT_ID_ELIGIBILITY: { step: WIZ_STEP.rights, field: 'aqContentIdRights', label: 'Content ID 적격성', hint: '커버·샘플·리믹스·공공저작물 등의 참조 등록 가능 여부와 제외 구간을 확인해 주세요.' },
  S2_CREDIT_PARTIES: { step: WIZ_STEP.tracks, field: 'tr-{k}-performers', label: '참여자', hint: '크레딧 참여자를 확인하고 다시 등록해 주세요.' },
  S2_AI_LYRICS_PROVENANCE: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '가사 출처', hint: '가사에 AI 생성 문구가 있어요. AI 활용 여부와 출처를 확인해 주세요. 문구만으로 AI 작성을 확정하지 않아요.' },
  AUDIO_AI_PROVENANCE: { step: WIZ_STEP.rights, field: 'aqSpecialOptions', label: '음원 출처', hint: '음원 메타데이터에 AI 생성 도구 정보가 있어요. AI 활용 여부와 권리 자료를 확인해 주세요.' },
  IMAGE_AI_PROVENANCE: { ...COVER, hint: '커버 메타데이터에 AI 생성 도구 정보가 있어요. 생성 경위와 이용 권리를 확인해 주세요.' },
  S2_ASSET_INTEGRITY: { ...AUDIO, hint: '제출 당시 파일과 등록 파일의 정보가 달라요. 파일을 확인하고 다시 제출해 주세요.' },
  S2_ARTWORK_INTEGRITY: { ...COVER, hint: '심사한 커버와 등록 파일의 정보가 달라요. 커버를 확인하고 다시 제출해 주세요.' },
  AUDIO_REQUIRED: { ...AUDIO, hint: '음원 파일이 없는 트랙이 있어요. 음원을 올려 주세요.' },
  ASSET_MISSING: { ...AUDIO, hint: '음원 파일을 찾을 수 없어요. 다시 올려 주세요.' },
  AUDIO_NOT_VERIFIED: { ...AUDIO, hint: '음원 업로드가 끝나지 않았어요. 파일을 다시 올려 주세요.' },
  AUDIO_NOT_ADMITTED: { ...AUDIO, hint: '사용할 수 없는 음원 파일이에요. 원본 파일을 다시 올려 주세요.' },
  AUDIO_MAGIC_MISMATCH: { ...AUDIO, hint: '음원 파일 형식이 확장자와 달라요. WAV·FLAC 원본으로 다시 올려 주세요.' },
  AUDIO_SAMPLE_FORMAT_UNSUPPORTED: { ...AUDIO, hint: '지원하지 않는 음원 형식이에요. 16bit 이상 무손실(WAV·FLAC·ALAC·AIFF·WavPack·TTA)로 다시 올려 주세요.' },
  AUDIO_SAMPLE_RATE_LOW: { ...AUDIO, hint: '샘플레이트가 낮아요. 44.1kHz 이상으로 다시 올려 주세요.' },
  AUDIO_BIT_DEPTH_LOW: { ...AUDIO, hint: '비트 깊이가 낮아요. 16bit 이상으로 다시 올려 주세요.' },
  AUDIO_CHANNEL_INVALID: { ...AUDIO, hint: '모노 또는 스테레오 음원만 받을 수 있어요.' },
  AUDIO_TOO_SHORT: { ...AUDIO, hint: '음원이 너무 짧아요. 30초 이상인 음원을 올려 주세요.' },
  AUDIO_TRUNCATED: { ...AUDIO, hint: '음원 파일이 중간에 잘려 있어요. 원본 파일을 다시 내보내 올려 주세요.' },
  AUDIO_SILENT: { ...AUDIO, hint: '음원에 소리가 없어요. 올바른 마스터 파일인지 확인해 주세요.' },
  AUDIO_CLIPPING: { ...AUDIO, hint: '음원에 클리핑(소리 깨짐)이 감지됐어요. 마스터 파일을 다시 올려 주세요.' },
  AUDIO_PROBE_FAILED: { ...AUDIO, hint: '음원 파일을 읽을 수 없어요. 원본 WAV·FLAC 파일을 다시 올려 주세요.' },
  AUDIO_FINGERPRINT_FAILED: { ...AUDIO, hint: '음원 분석을 마치지 못했어요. 파일을 다시 올려 주세요.' },
  AUDIO_SIMILAR_TO_EXISTING: { ...AUDIO, hint: '이미 등록된 음원과 매우 비슷해요. 권리 관계를 확인하거나 다른 파일을 올려 주세요.' },
  SHA256_MISMATCH: { ...AUDIO, hint: '업로드한 파일이 손상됐어요. 파일을 다시 올려 주세요.' },
  ASSET_NOT_VERIFIED: { ...AUDIO, hint: '파일 확인이 끝나지 않았어요. 파일을 다시 올려 주세요.' },
  ASSET_NOT_ADMITTED: { ...AUDIO, hint: '사용할 수 없는 파일이에요. 원본 파일을 다시 올려 주세요.' },
  ASSET_REUSED: { ...AUDIO, hint: '다른 발매에서 쓰인 파일이에요. 이 발매용 원본을 올려 주세요.' },
  QC_ANALYSIS_FAILED: { ...AUDIO, hint: '음원 품질 검사를 통과하지 못했어요. 파일을 다시 올려 주세요.' },
  ISRC: { step: WIZ_STEP.tracks, field: 'tr-{k}-isrc', label: 'ISRC', hint: 'ISRC 코드를 확인해 주세요.' },
  ISRC_FORMAT_INVALID: { step: WIZ_STEP.tracks, field: 'tr-{k}-isrc', label: 'ISRC', hint: 'ISRC 형식이 맞지 않아요. 예: KR-ABC-26-00001' },
  ISRC_DUPLICATE: { step: WIZ_STEP.tracks, field: 'tr-{k}-isrc', label: 'ISRC', hint: '다른 곡과 같은 ISRC예요. ISRC를 확인해 주세요.' },
  // 커버아트
  IMAGE_TOO_SMALL: { ...COVER, hint: '커버아트 해상도가 작아요. 3000×3000 이상 정사각형 이미지로 다시 올려 주세요.' },
  IMAGE_NOT_SQUARE: { ...COVER, hint: '커버아트가 정사각형이 아니에요. 1:1 비율 이미지로 다시 올려 주세요.' },
  IMAGE_PROBE_FAILED: { ...COVER, hint: '커버 이미지를 읽을 수 없어요. JPG·PNG 파일로 다시 올려 주세요.' },
  IMAGE_MAGIC_MISMATCH: { ...COVER, hint: '커버 파일 형식이 확장자와 달라요. JPG·PNG 원본으로 다시 올려 주세요.' },
  // 배급 설정
  FIELD_RELEASE_DATE_MISSING: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매 예정일을 입력해 주세요.' },
  FIELD_RELEASE_DATE_INVALID: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매 예정일을 다시 확인해 주세요.' },
  UPC_FORMAT_INVALID: { step: WIZ_STEP.distribution, field: 'f-upc', label: 'UPC', hint: 'UPC는 12~13자리 숫자예요. 없으면 비워 두세요.' },
  S2_RELEASE_DATE_FAR_PAST: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매일이 너무 과거예요. 발매 예정일을 다시 확인해 주세요.' },
  S2_RELEASE_DATE_FAR_FUTURE: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매일이 너무 먼 미래예요. 발매 예정일을 다시 확인해 주세요.' },
  S2_CATALOG_IDENTIFIERS: { step: WIZ_STEP.distribution, field: 'f-upc', label: 'UPC·ISRC', hint: '식별자(UPC·ISRC)가 다른 발매와 겹치거나 형식이 맞지 않아요.' },
  S2_DSP_ELIGIBILITY: { step: WIZ_STEP.distribution, field: 'aqPlatforms', label: '배급 플랫폼', hint: '선택한 플랫폼 중 배급할 수 없는 곳이 있어요.' },
  S2_SPECIAL_FLAGS: { step: WIZ_STEP.rights, field: 'aqSpecialOptions', label: '특수 항목', hint: '커버곡·샘플·AI 활용 등 신고 항목을 확인해 주세요.' },
  S2_UNDECLARED_CONTENT: { step: WIZ_STEP.rights, field: 'aqSpecialOptions', label: '특수 항목', hint: '신고하지 않은 커버곡·샘플·AI 활용이 감지됐어요.' },
  S2_CONTENT_SIGNALS: { step: WIZ_STEP.rights, field: 'aqSpecialOptions', label: '특수 항목', hint: '콘텐츠 신고 항목을 다시 확인해 주세요.' },
  // 권리
  PLINE_MISSING: { step: WIZ_STEP.rights, field: 'f-phonogram', label: '℗ 표기', hint: '음반 제작 권리(℗) 표기를 ‘2026 권리자명’처럼 입력해 주세요.' },
  CLINE_MISSING: { step: WIZ_STEP.rights, field: 'f-copyright', label: '© 표기', hint: '저작권(©) 표기를 ‘2026 권리자명’처럼 입력해 주세요.' },
  S2_RIGHTS_SCOPE: { step: WIZ_STEP.rights, field: 'f-ownership', label: '권리 정보', hint: '권리자 정보와 배급 범위를 확인해 주세요.' },
  S2_DOCS_ORIGIN: { step: WIZ_STEP.rights, field: 'f-ownership', label: '권리 증빙', hint: '권리 증빙 서류의 출처를 확인할 수 없어요.' },
  S2_CONSENT_VALIDITY: { step: WIZ_STEP.review, label: '동의 갱신', hint: '동의 유효기간 또는 제출 자료가 일치하지 않아요. 동의를 다시 진행하고 제출해 주세요.' },
  S2_RIGHTS_DECLARATIONS: { step: WIZ_STEP.rights, field: 'f-ownership', label: '배급 권리·성인 확인', hint: '배급에 필요한 권리와 성인 여부 확인을 완료해 주세요.' },
  // 담당자 검토 의견 (특정 항목이 아닌 발매 전체에 대한 의견)
  REVIEW_NOTE: { step: WIZ_STEP.review, label: '담당자 의견', hint: '담당자 검토 의견을 확인해 주세요.' },
  // 배포 준비 (3단계) — 아티스트 입력보다 시스템 쪽 문제일 때가 많다
  STAGE3_PREPARATION_FAILED: { step: WIZ_STEP.review, label: '배포 준비', hint: '배포 준비를 마치지 못했어요. 내용을 확인하고 다시 접수해 주세요. 같은 안내가 반복되면 문의로 알려 주세요.' },
};

/** 담당자가 보완 요청에서 직접 고르는 항목 (페이지 → 항목). `track`이면 트랙도 고른다.
 *  서버에는 검토 의견의 check_code로 `FIX_…` 또는 `FIX_…@<트랙 id>`가 저장된다. */
const STAFF_FIX: Record<string, Target & { track?: boolean }> = {
  FIX_TITLE: { step: WIZ_STEP.info, field: 'f-title', label: '발매 제목', hint: '발매 제목을 확인해 주세요.' },
  FIX_ARTIST: { step: WIZ_STEP.info, field: 'f-artist', label: '아티스트명', hint: '아티스트명을 확인해 주세요.' },
  FIX_TYPE: { step: WIZ_STEP.info, field: 'f-type', label: '발매 유형', hint: '발매 유형을 확인해 주세요.' },
  FIX_GENRE: { step: WIZ_STEP.info, field: 'f-genre', label: '장르', hint: '장르를 확인해 주세요.' },
  FIX_LANGUAGE: { step: WIZ_STEP.info, field: 'f-language', label: '주요 언어', hint: '주요 언어를 확인해 주세요.' },
  FIX_LABEL: { step: WIZ_STEP.info, field: 'f-label', label: '레이블', hint: '레이블 표기를 확인해 주세요.' },
  FIX_NOTES: { step: WIZ_STEP.info, field: 'f-notes', label: '앨범 소개', hint: '앨범 소개를 확인해 주세요.' },
  FIX_TRACK_TITLE: { step: WIZ_STEP.tracks, field: 'tr-{k}-title', label: '곡명', hint: '곡명을 확인해 주세요.', track: true },
  FIX_TRACK_VERSION: { step: WIZ_STEP.tracks, field: 'tr-{k}-version', label: '곡 버전', hint: '버전 표기를 확인해 주세요.', track: true },
  FIX_AUDIO: { step: WIZ_STEP.tracks, field: 'trackFile-{k}', label: '음원 파일', hint: '음원 파일을 다시 올려 주세요.', track: true },
  FIX_COMPOSERS: { step: WIZ_STEP.tracks, field: 'tr-{k}-composers', label: '작곡', hint: '작곡가 정보를 확인해 주세요.', track: true },
  FIX_LYRICISTS: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyricists', label: '작사', hint: '작사가 정보를 확인해 주세요.', track: true },
  FIX_ARRANGERS: { step: WIZ_STEP.tracks, field: 'tr-{k}-arrangers', label: '편곡', hint: '편곡자 정보를 확인해 주세요.', track: true },
  FIX_PERFORMERS: { step: WIZ_STEP.tracks, field: 'tr-{k}-performers', label: '참여 아티스트', hint: '참여 아티스트 정보를 확인해 주세요.', track: true },
  FIX_LYRICS: { step: WIZ_STEP.tracks, field: 'tr-{k}-lyrics', label: '가사', hint: '가사를 확인해 주세요.', track: true },
  FIX_ISRC: { step: WIZ_STEP.tracks, field: 'tr-{k}-isrc', label: 'ISRC', hint: 'ISRC를 확인해 주세요.', track: true },
  FIX_COVER: { ...COVER, hint: '커버아트를 다시 올려 주세요.' },
  FIX_RELEASE_DATE: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매 예정일을 확인해 주세요.' },
  FIX_UPC: { step: WIZ_STEP.distribution, field: 'f-upc', label: 'UPC', hint: 'UPC를 확인해 주세요.' },
  FIX_PLATFORMS: { step: WIZ_STEP.distribution, field: 'aqPlatforms', label: '배급 플랫폼', hint: '배급 플랫폼을 확인해 주세요.' },
  FIX_SPECIAL: { step: WIZ_STEP.rights, field: 'aqSpecialOptions', label: '특수 항목', hint: '커버곡·샘플·AI 활용 등 신고 항목을 확인해 주세요.' },
  FIX_OWNERSHIP: { step: WIZ_STEP.rights, field: 'f-ownership', label: '음원 권리자', hint: '음원 권리자를 확인해 주세요.' },
  FIX_PLINE: { step: WIZ_STEP.rights, field: 'f-phonogram', label: '℗ 표기', hint: '℗ 표기를 확인해 주세요.' },
  FIX_CLINE: { step: WIZ_STEP.rights, field: 'f-copyright', label: '© 표기', hint: '© 표기를 확인해 주세요.' },
  FIX_OTHER: { step: WIZ_STEP.review, label: '기타', hint: '담당자 요청 내용을 확인해 주세요.' },
};
Object.assign(TARGETS, STAFF_FIX);

export interface StaffFixOption { code: string; step: number; label: string; track: boolean }
/** 관리자 보완 요청 화면의 항목 목록 (위자드 단계 순) */
export const STAFF_FIX_OPTIONS: StaffFixOption[] = Object.entries(STAFF_FIX)
  .map(([code, t]) => ({ code, step: t.step, label: t.label, track: !!t.track }));

/** 담당자 지정 항목 → 서버 check_code (`FIX_AUDIO@<트랙 id>`) */
export const staffFixCode = (code: string, trackId?: string) => (trackId ? `${code}@${trackId}` : code);
/** 서버 check_code → 담당자 지정 항목 (아니면 null) */
export function parseStaffFix(checkCode: string): { code: string; trackId?: string } | null {
  const [code, trackId] = checkCode.split('@');
  if (!(code in STAFF_FIX)) return null;
  return trackId ? { code, trackId } : { code };
}

const FALLBACK: Target = { step: WIZ_STEP.review, label: '기타', hint: '요청 내용을 확인하고 필요한 항목을 고쳐 주세요.' };

export interface ResolvedCorrection extends Correction {
  step: number;
  label: string;
  /** 실제 DOM id (트랙 순번 반영) */
  field?: string;
}

export function correctionTarget(code: string): Target {
  return TARGETS[code] ?? FALLBACK;
}

/** 서버 검사 코드 중 사용자가 고칠 수 있는 항목인지 */
export function isKnownCorrection(code: string): boolean {
  return code in TARGETS;
}

/** 보완 요청을 위자드 위치로 풀어낸다. trackIds는 신청서의 트랙 순서 */
export function resolveCorrection(c: Correction, trackIds: string[] = []): ResolvedCorrection {
  const t = correctionTarget(c.code);
  const k = Math.max(0, c.trackId ? trackIds.indexOf(c.trackId) : 0);
  return {
    ...c,
    message: c.message || t.hint,
    step: t.step,
    label: t.label,
    field: t.field?.replace('{k}', String(k)),
  };
}

/** 보완 요청 → 신청서의 해당 입력칸으로 바로 가는 주소 */
export function fixPath(releaseId: string, c?: Correction): string {
  const q = new URLSearchParams({ edit: releaseId });
  if (c) {
    q.set('fix', c.code);
    if (c.trackId) q.set('track', c.trackId);
  } else {
    q.set('fix', '1');
  }
  return `/upload?${q.toString()}`;
}

/** '단계 · 항목' 표기 (같은 이름이면 한 번만) */
export function correctionWhere(r: ResolvedCorrection): string {
  const stepName = WIZ_STEP_NAMES[r.step];
  return stepName === r.label ? r.label : `${stepName} · ${r.label}`;
}
