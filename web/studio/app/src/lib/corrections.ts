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
  // 트랙·음원
  S2_META_CREDITS: { step: WIZ_STEP.tracks, field: 'tr-{k}-composers', label: '크레딧', hint: '작사·작곡 등 크레딧 정보가 비어 있거나 형식이 맞지 않아요.' },
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
  // 커버아트
  IMAGE_TOO_SMALL: { ...COVER, hint: '커버아트 해상도가 작아요. 3000×3000 이상 정사각형 이미지로 다시 올려 주세요.' },
  IMAGE_NOT_SQUARE: { ...COVER, hint: '커버아트가 정사각형이 아니에요. 1:1 비율 이미지로 다시 올려 주세요.' },
  IMAGE_PROBE_FAILED: { ...COVER, hint: '커버 이미지를 읽을 수 없어요. JPG·PNG 파일로 다시 올려 주세요.' },
  IMAGE_MAGIC_MISMATCH: { ...COVER, hint: '커버 파일 형식이 확장자와 달라요. JPG·PNG 원본으로 다시 올려 주세요.' },
  // 배급 설정
  S2_RELEASE_DATE_FAR_PAST: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매일이 너무 과거예요. 발매 예정일을 다시 확인해 주세요.' },
  S2_RELEASE_DATE_FAR_FUTURE: { step: WIZ_STEP.distribution, field: 'f-releaseDate', label: '발매일', hint: '발매일이 너무 먼 미래예요. 발매 예정일을 다시 확인해 주세요.' },
  S2_CATALOG_IDENTIFIERS: { step: WIZ_STEP.distribution, field: 'f-upc', label: 'UPC·ISRC', hint: '식별자(UPC·ISRC)가 다른 발매와 겹치거나 형식이 맞지 않아요.' },
  S2_DSP_ELIGIBILITY: { step: WIZ_STEP.distribution, field: 'aqPlatforms', label: '배급 플랫폼', hint: '선택한 플랫폼 중 배급할 수 없는 곳이 있어요.' },
  S2_SPECIAL_FLAGS: { step: WIZ_STEP.distribution, field: 'aqSpecialOptions', label: '특수 항목', hint: '커버곡·샘플·AI 활용 등 신고 항목을 확인해 주세요.' },
  S2_UNDECLARED_CONTENT: { step: WIZ_STEP.distribution, field: 'aqSpecialOptions', label: '특수 항목', hint: '신고하지 않은 커버곡·샘플·AI 활용이 감지됐어요.' },
  S2_CONTENT_SIGNALS: { step: WIZ_STEP.distribution, field: 'aqSpecialOptions', label: '특수 항목', hint: '콘텐츠 신고 항목을 다시 확인해 주세요.' },
  // 권리
  S2_RIGHTS_SCOPE: { step: WIZ_STEP.rights, field: 'f-ownership', label: '권리 정보', hint: '권리자 정보와 배급 범위를 확인해 주세요.' },
  S2_DOCS_ORIGIN: { step: WIZ_STEP.rights, field: 'f-ownership', label: '권리 증빙', hint: '권리 증빙 서류의 출처를 확인할 수 없어요.' },
};

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
