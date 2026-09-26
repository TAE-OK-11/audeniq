// 화면에서 쓰는 데이터 모델 (목 API와 실제 API 어댑터가 같은 모양으로 돌려준다)

export interface User {
  id: string;
  email: string;
}

export interface Org {
  id: string;
  name: string;
}

export interface Release {
  id: string;
  title: string;
  status: string;
  release_date: string | null;
  created_at: string;
  updated_at?: string;
  track_count: number;
  artist?: string;
  coverData?: string;
  /** 검토 결과 보완이 필요한 항목 (status === 'needs'일 때) */
  corrections?: Correction[];
}

/** 보완 요청 한 건 — code로 신청서의 어느 단계·입력칸을 고쳐야 하는지 찾는다 (lib/corrections.ts) */
export interface Correction {
  code: string;
  /** 담당자·검사가 남긴 요청 내용 */
  message: string;
  /** 특정 곡에 대한 요청이면 해당 트랙 ID */
  trackId?: string;
}

export interface ReleaseDetail extends Release {
  tracks: Track[];
  draft?: ReleaseDraft;
}

export interface DraftTrack {
  id: string; title: string; version: string; isrc: string;
  composers: string; lyricists: string; arrangers: string; performers: string;
  producer: string; lyrics: string; audioName: string; audioSize: number;
  explicit: boolean; duration: string;
  /** 실서버: 업로드 완료된 음원 자산 ID */
  assetId?: string;
  /** 실서버: 트랙 ID (임시 저장 간 매칭용) */
  serverId?: string;
  /** 피처링 아티스트 (제목에 쓰지 않고 따로 전달) */
  featuring?: string;
  /** 가사 없는 연주곡 — 작사·가사 불필요 */
  instrumental?: boolean;
  /** 음원 규격 표기 (예: WAV · 24bit · 48kHz · 스테레오) */
  audioSpec?: string;
}

/** 이미 발매한 아티스트의 플랫폼 프로필 — 동명이인 페이지로 잘못 연결되지 않도록 */
export interface ArtistProfileLinks {
  isNew: boolean;
  spotify: string;
  apple: string;
  melon: string;
}

/** 신청인이 서명해 접수한 신청서 기록 */
export interface ApplicationRecord {
  /** 신청서 번호 (AQ-YYYYMMDD-XXXXXX) */
  no: string;
  /** 서식 버전 */
  form: string;
  submittedAt: string;
  signerName: string;
  /** 신청인 구분 (본인·대리인) */
  signerRole: string;
  /** 서명 이미지 (작은 PNG data URL) */
  signature: string;
  /** 신청 내용 + 서명의 SHA-256 (위·변조 확인용) */
  hash: string;
  /** 동의한 조항 */
  agreements: string[];
}

export interface CoverTrackData {
  trackId: string; originalTitle: string; originalArtist: string; originalWriters: string;
}

export interface ReleaseOptionsData {
  express: boolean; expressAck: boolean; expressReason: string;
  minor: boolean;
  guardian: string; guardianRelation: string; guardianContact: string;
  guardian2: string; guardian2Relation: string; guardian2Contact: string;
  guardianConsentDone: boolean; familyCertName: string; familyCertMethod: string;
  cover: boolean; coverTracks: CoverTrackData[]; coverRightsAck: boolean; coverLicenseFile: string;
  sample: boolean; sampleLicenseFile: string;
  featured: boolean; featuredConsentFile: string;
  ai: boolean; aiTool: string;
  shared: boolean; sharedContractFile: string;
  rerelease: boolean; previousTitle: string; previousId: string;
}

export interface ReleaseDraft {
  artist?: string;
  type: string;
  language?: string;
  genre: string;
  genreCustom?: string;
  label: string;
  upc: string;
  notes: string;
  coverName: string;
  coverData?: string;
  /** 실서버: 업로드 완료된 커버 이미지 자산 ID */
  coverAssetId?: string;
  originalDate?: string;
  territories: string[];
  platforms: string[];
  ownership: string;
  phonogram: string;
  copyright: string;
  rightsChecks: Record<string, boolean>;
  options?: ReleaseOptionsData;
  draftTracks?: DraftTrack[];
  /** 위자드에서 마지막으로 머문 단계 (이어서 작성용) */
  lastStep?: number;
  artistProfile?: ArtistProfileLinks;
  /** 서명해 접수한 신청서 */
  application?: ApplicationRecord;
  history: { text: string; time: string }[];
}

export interface Track {
  id: string;
  title: string;
  duration_ms: number | null;
  isrc: string | null;
  version?: string | null;
  composers?: string | null;
  lyricists?: string | null;
  audioName?: string | null;
  explicit?: boolean;
  assetId?: string | null;
}

/** 위자드 → 서버로 보내는 전체 발매 정보 */
export interface ReleasePayload {
  title: string;
  artist: string;
  type: string;
  language: string;
  genre: string;
  genreCustom: string;
  label: string;
  upc: string;
  notes: string;
  coverName: string;
  coverData: string;
  coverAssetId?: string;
  originalDate: string;
  release_date: string;
  tracks: DraftTrack[];
  territories: string[];
  platforms: string[];
  ownership: string;
  phonogram: string;
  copyright: string;
  rightsChecks: Record<string, boolean>;
  options: ReleaseOptionsData;
  lastStep?: number;
  artistProfile?: ArtistProfileLinks;
  application?: ApplicationRecord;
}

/** DOCUMENT = 권리 증빙 (PDF 또는 스캔 이미지) */
export type UploadKind = 'AUDIO' | 'IMAGE' | 'DOCUMENT';

/** 임시 저장 결과 — 실서버는 새로 만든 트랙의 서버 ID를 함께 돌려준다 */
export interface SaveResult extends Release {
  /** 화면 트랙 ID → 서버 트랙 ID */
  trackServerIds?: Record<string, string>;
}

export interface UploadResult {
  assetId: string;
  sha256?: string;
  container?: string;
}

/** 서버 사전 점검 결과 */
export interface PreflightIssue { code: string; message: string; trackTitle?: string }
