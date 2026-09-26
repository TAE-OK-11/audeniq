// 계약·권리 서류 공유 스토어 — 라이브 db.contracts 대응
// Contracts(계약서)와 Rights(권리·보완 서류)가 같은 문서를 공유한다.
import { createStore } from '../lib/store';

export interface ConsentRecord { time: string; action: string; version: string }
export interface ReviewRecord { status: string; time: string; detail: string }

export interface DocRecord {
  id: string;
  kind: 'agreements' | 'rights';
  title: string;
  releaseId?: string;
  releaseTitle: string;
  version: string;
  created: string;
  content: string;
  fileName: string;
  /** 첨부 원본 File — 직렬화할 수 없어 현재 세션 메모리에만 보관 */
  fileBlob?: File | null;
  fileType?: string;
  uploadedAt?: string;
  approvedAt?: string;
  checked: boolean;
  checkedAt: string;
  consentHistory: ConsentRecord[];
  reviewHistory: ReviewRecord[];
  reviewStatus: 'awaiting_documents' | 'review' | 'prepared' | 'approved' | 'needs';
  reviewNote: string;
  signerName: string;
  localSignatureData: string;
  localSignatureAt: string;
}

const AGREEMENT_CONTENT = [
  'AUDENIQ 디지털 음원 배급 신청·계약서',
  '',
  '1. 신청인 및 발매 정보',
  '아티스트: 서린',
  '발매명: 첫 번째 싱글',
  '발매 유형: 싱글',
  '발매 희망일: 2026-10-01',
  '레이블 표기: AUDENIQ',
  '수록곡: 1. 첫 번째 싱글 / 2. 첫 번째 싱글 (Inst.)',
  '',
  '2. 권리자 및 배급 범위',
  '마스터 권리자: 서린',
  '℗ 표기: 2026 서린',
  '© 표기: 2026 서린',
  '배급 지역: 전 세계',
  '배급 플랫폼: Spotify, Apple Music, YouTube Music',
  '추가 확인 항목: 일반 발매',
  '',
  '3. 신청인의 확인',
  '신청인은 제출한 음원, 가사, 커버아트, 크레딧 및 메타데이터를 배급할 적법한 권한을 보유하고 있으며, 제3자의 권리가 포함된 경우 필요한 허락을 확보했음을 확인합니다.',
  '',
  '4. AUDENIQ 배급 계약',
  '신청인은 위 발매 정보를 기준으로 AUDENIQ에 디지털 음원 배급을 신청하고, 계약서에 안내된 배급 범위·정산·수정·테이크다운 및 권리 보증 조항에 동의합니다. 보완이 필요한 경우 AUDENIQ는 관련 자료를 요청할 수 있습니다.',
  '',
  '위 신청 정보는 자동으로 작성됐습니다. 내용을 확인한 뒤 신청인 서명만 진행해 주세요.',
].join('\n');

const INITIAL_DOCS: DocRecord[] = [
  {
    id: 'doc1', kind: 'agreements',
    title: '첫 번째 싱글 · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseId: 'r1', releaseTitle: '첫 번째 싱글', version: '1.0', created: '2026-09-20',
    content: AGREEMENT_CONTENT, fileName: '', checked: true, checkedAt: '2026-09-20 14:32',
    consentHistory: [{ time: '2026-09-20 14:32', action: '내용 확인', version: '1.0' }],
    reviewHistory: [{ status: '검토 완료', time: '2026-09-21 10:05', detail: 'AUDENIQ 담당자 검토 완료' }],
    reviewStatus: 'approved', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
  },
  {
    id: 'doc2', kind: 'agreements',
    title: '여름 EP · AUDENIQ 디지털 음원 배급 신청·계약서',
    releaseId: 'r2', releaseTitle: '여름 EP', version: '1.0', created: '2026-09-22',
    content: '', fileName: '', checked: false, checkedAt: '',
    consentHistory: [],
    reviewHistory: [{ status: '접수 요청', time: '2026-09-22 09:10', detail: '신청서가 작성됐어요. 담당자 검토 접수는 전송 후 시작됩니다.' }],
    reviewStatus: 'review', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
  },
  {
    id: 'd1', kind: 'rights',
    title: '첫 번째 싱글 · 마스터 음원 권리 확인서',
    releaseId: 'r1', releaseTitle: '첫 번째 싱글', version: '1.0', created: '2026-09-22',
    content: '', fileName: '', checked: false, checkedAt: '',
    consentHistory: [], reviewHistory: [],
    reviewStatus: 'review', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
  },
  {
    id: 'd2', kind: 'rights',
    title: '여름 EP · 작사·작곡 및 커버곡 이용 허락서',
    releaseId: 'r2', releaseTitle: '여름 EP', version: '1.0', created: '2026-09-23',
    content: '', fileName: 'credit_proof.pdf', checked: false, checkedAt: '',
    consentHistory: [], reviewHistory: [],
    reviewStatus: 'needs', reviewNote: '서명란에 서명자 이름이 빠져 있어요. 보완 후 다시 제출해 주세요.',
    signerName: '', localSignatureData: '', localSignatureAt: '',
  },
  {
    id: 'd3', kind: 'rights',
    title: '데모 트랙 · 커버아트 이용 허락서',
    releaseId: 'r3', releaseTitle: '데모 트랙', version: '1.0', created: '2026-09-24',
    content: '', fileName: '', checked: false, checkedAt: '',
    consentHistory: [], reviewHistory: [],
    reviewStatus: 'awaiting_documents', reviewNote: '', signerName: '', localSignatureData: '', localSignatureAt: '',
  },
];

const STATUSES: DocRecord['reviewStatus'][] = ['awaiting_documents', 'review', 'prepared', 'approved', 'needs'];

/** 저장된 문서를 현재 형식으로 보정 (필드 누락 시 목록 화면이 깨지는 것 방지) */
function normalizeDoc(raw: unknown): DocRecord | null {
  if (!raw || typeof raw !== 'object') return null;
  const d = raw as Partial<DocRecord>;
  if (typeof d.id !== 'string' || !d.id) return null;
  const s = (v: unknown) => (typeof v === 'string' ? v : '');
  return {
    ...d,
    id: d.id,
    kind: d.kind === 'agreements' ? 'agreements' : 'rights',
    title: s(d.title) || '제목 없는 문서',
    releaseTitle: s(d.releaseTitle),
    version: s(d.version) || '1.0',
    created: s(d.created),
    content: s(d.content),
    fileName: s(d.fileName),
    fileBlob: null,
    checked: !!d.checked,
    checkedAt: s(d.checkedAt),
    consentHistory: Array.isArray(d.consentHistory) ? d.consentHistory : [],
    reviewHistory: Array.isArray(d.reviewHistory) ? d.reviewHistory : [],
    reviewStatus: STATUSES.includes(d.reviewStatus as DocRecord['reviewStatus']) ? d.reviewStatus as DocRecord['reviewStatus'] : 'awaiting_documents',
    reviewNote: s(d.reviewNote),
    signerName: s(d.signerName),
    localSignatureData: s(d.localSignatureData),
    localSignatureAt: s(d.localSignatureAt),
  };
}

const store = createStore<DocRecord[]>(INITIAL_DOCS, {
  persist: 'docs',
  // File 객체는 직렬화할 수 없으므로 저장하지 않는다 (원본은 현재 세션에서만 열람)
  serialize: list => list.map(({ fileBlob: _omit, ...rest }) => rest),
  revive: (raw, fallback) => (Array.isArray(raw) ? raw.map(normalizeDoc).filter((d): d is DocRecord => !!d) : fallback),
});

export const useDocs = store.use;
export const getDocsSnapshot = store.get;

export function getDoc(id: string): DocRecord | undefined {
  return store.get().find(d => d.id === id);
}

export function addDoc(doc: DocRecord): void {
  store.set(list => [doc, ...list]);
}

export function updateDoc(id: string, patch: Partial<DocRecord>): void {
  store.set(list => list.map(d => (d.id === id ? { ...d, ...patch } : d)));
}

export function removeDoc(id: string): void {
  store.set(list => list.filter(d => d.id !== id));
}

/** 발매에 연결된 문서 (releaseId 우선, 없으면 제목으로 매칭) */
export function docsForRelease(list: DocRecord[], releaseId: string, releaseTitle?: string): DocRecord[] {
  return list.filter(d => (d.releaseId ? d.releaseId === releaseId : !!releaseTitle && d.releaseTitle === releaseTitle));
}

/** 라이브 aqDocumentState */
export function docState(c: DocRecord): string {
  if (c.reviewStatus === 'approved') return '검토 완료';
  if (c.reviewStatus === 'needs') return '보완 요청';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return '검토 중';
  if (c.localSignatureAt) return '서명 완료';
  return c.kind === 'rights' ? '서류 제출 필요' : '서명 필요';
}

/** 카드 톤 클래스 */
export function docTone(c: DocRecord): string {
  if (c.reviewStatus === 'needs') return 'is-needs';
  if (c.reviewStatus === 'approved') return 'is-approved';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return 'is-review';
  return '';
}
