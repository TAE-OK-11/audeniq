// 계약·권리 서류 공유 스토어 — 라이브 db.contracts 대응
// Contracts(계약서)와 Rights(권리·보완 서류)가 같은 문서를 공유한다.
import { createStore } from '../lib/store';
import { MOCK } from '../lib/mode';
import type { ElectronicRightsRecord } from '../lib/rightsDocument';

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
  /** 실서버 낙관적 잠금 버전 */
  rowVersion?: number;
  electronic?: ElectronicRightsRecord | null;
  /** 배급 계약서(AUD-DIST 2.0): 담당자가 정한 거래 조건과 서명 때 체크할 확인 항목 */
  agreementTerms?: AgreementTermsRecord | null;
  /** 서명할 때 체크한 확인 항목 */
  confirmations?: { items: { id: string; text: string; required: boolean; checked: boolean }[]; content_hash: string } | null;
}

export interface AgreementTermsRecord {
  form: string; terms_version: string; exclusivity: 'NON_EXCLUSIVE' | 'EXCLUSIVE'; fee_bps: number; user_bps: number;
  confirmations: { id: string; text: string; required: boolean }[];
}

// 체험용 계약서 — 서버(crates/core/src/agreement.rs)가 만드는 AUD-DIST 2.0 본문과 같은 모양
const AGREEMENT_BOXES: AgreementTermsRecord['confirmations'] = [
  { id: 'grant', required: true, text: '이 계약의 대상 콘텐츠를 저장·복제·전송하고, DSP에 전달하며, DSP와 배급 파트너가 배급에 필요한 범위에서 이용하게 하고, 메타데이터·앨범아트를 전달하고, 배급 관련 행정업무를 수행할 권한을 회사에 부여합니다.' },
  { id: 'rights_own', required: true, text: '배급에 필요한 권리를 직접 보유하거나, 권리자로부터 필요한 허락을 받았습니다.' },
  { id: 'rights_scope', required: true, text: '저작권(작사·작곡), 저작인접권, 실연자의 권리, 마스터(음반제작자)의 권리를 확인했습니다.' },
  { id: 'third_party', required: true, text: '샘플·비트·앨범아트·사진·이미지·영상 등 제3자 자료를 쓸 권리를 확보했습니다.' },
  { id: 'coauthors', required: true, text: '다른 사람과 함께 만든 콘텐츠라면 필요한 동의와 위임을 받았습니다.' },
  { id: 'documents_true', required: true, text: '제출한 위임서·계약서·라이선스 등 자료는 모두 진실합니다.' },
  { id: 'no_misuse', required: true, text: '타인의 명의나 콘텐츠를 허락 없이 사용하지 않았습니다.' },
  { id: 'terms', required: true, text: 'AUDENIQ 음원 배급 서비스 이용약관을 확인했고, 이 계약에서 정하지 않은 사항에 약관이 적용되는 데 동의합니다.' },
];
const AGREEMENT_TERMS: AgreementTermsRecord = {
  form: 'AUD-DIST 2.0', terms_version: 'AUD-TERMS 2026.10', exclusivity: 'NON_EXCLUSIVE', fee_bps: 800, user_bps: 9200, confirmations: AGREEMENT_BOXES,
};
const AGREEMENT_CONTENT = [
  'AUDENIQ 음원 배급 계약서', '',
  '서식 AUD-DIST 2.0 · 적용 약관 AUDENIQ 음원 배급 서비스 이용약관 (AUD-TERMS 2026.10)',
  '신청서 번호 AUD-20260920-ABCDEF', '',
  '주식회사 AUDENIQ(이하 “회사”)와 아래 이용자는 아래 릴리즈의 디지털 음원 배급에 관하여 다음과 같이 계약합니다.', '',
  '제1조 (계약 당사자)', '회사: 주식회사 AUDENIQ', '이용자: 서린 (개인) · 아티스트 서린', '서명자: 서린 (아티스트 본인)', '',
  '제2조 (계약 대상)', '이 계약은 아래 릴리즈 한 건에 적용합니다. 트랙 목록은 별첨 1과 같습니다.', '발매명: 첫 번째 싱글 (싱글)', '아티스트: 서린', 'UPC/EAN: 8800000000011', '발매 예정일: 2026-10-01', '',
  '제3조 (배급 형태와 지역)', '배급 형태: 비독점 — 이용자는 대상 콘텐츠를 다른 경로로도 배급할 수 있습니다. 다만 같은 DSP에 중복 배급해 생긴 문제는 이용자가 해결합니다.', '배급 지역: 전 세계', '',
  '제4조 (대상 DSP와 서비스 범위)', '대상 DSP: Spotify, Apple Music, YouTube Music', '서비스 범위: 기본 음원 배급. UGC 권리관리(YouTube Content ID, TikTok·Meta 등)와 그 밖의 부가서비스는 이 계약에 포함하지 않으며, 신청하는 경우 별도로 정합니다.', '',
  '제5조 (계약기간)', '이용자가 서명한 날부터 이용자가 해지를 요청할 때까지로 합니다. 해지 요청과 그 후의 처리는 약관 제18장(계약 종료)에 따릅니다.', '',
  '제6조 (배급수수료)', '배급수수료: 회사가 DSP와 배급 파트너로부터 실제로 받은 대상 콘텐츠의 수익 중 회사 8% / 이용자 92%', '',
  '제7조 (정산)', '정산 수령인: 서린 (개인)', '지급 통화: 대한민국 원(KRW)', '정산 주기, 공제·조정, 환수, 지급 보류와 최소지급액의 일반 기준은 약관 제15장(정산)에 따릅니다.', '',
  '제8조 (개별 특약)', '없음', '',
  '제9조 (배급 권한의 부여)', '이용자는 대상 콘텐츠에 관하여 약관 제11조부터 제13조까지에서 정한 범위의 이용허락과 업무 수행 권한을 회사에 부여하며, 서명할 때 별첨 2의 해당 항목에 직접 체크하여 다시 확인합니다. 이 계약으로 저작권 등 권리 자체가 회사에 양도되지 않습니다.', '',
  '제10조 (권리의 최종 확인)', '이용자는 서명할 때 별첨 2의 권리 확인 항목에 직접 체크합니다. 확인한 내용이 사실과 달라 생긴 손해는 약관 제93조에 따라 이용자가 책임집니다.', '',
  '제11조 (약관의 적용)', '이 계약에서 정하지 않은 사항은 AUDENIQ 음원 배급 서비스 이용약관(AUD-TERMS 2026.10)에 따릅니다. 이 계약과 약관의 내용이 서로 다르면 이 계약을 우선합니다.', '',
  '제12조 (전자계약)', '이 계약은 전자문서로 작성하고 전자서명으로 체결합니다. 체결일은 이용자가 서명한 날이며, 회사는 담당자 승인으로 계약 내용을 확정한 뒤 이용자의 서명을 받습니다. 계약서 번호·버전·서명 기록은 함께 보관됩니다.', '',
  '별첨 1 · 트랙 목록', '1. 첫 번째 싱글 (ISRC KRA262600001)', '2. 첫 번째 싱글 (Inst.) (ISRC KRA262600002)', '',
  '별첨 2 · 서명 시 확인 항목',
  ...AGREEMENT_BOXES.map(b => `□ (필수) ${b.text}`),
].join('\n');

const INITIAL_DOCS: DocRecord[] = [
  {
    id: 'doc1', kind: 'agreements',
    title: '첫 번째 싱글 · AUDENIQ 음원 배급 계약서',
    releaseId: 'r1', releaseTitle: '첫 번째 싱글', version: '2.0', created: '2026-09-20',
    content: AGREEMENT_CONTENT, agreementTerms: AGREEMENT_TERMS, fileName: '', checked: true, checkedAt: '2026-09-20 14:32',
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

// 실서버 모드: 서버에서 불러오고 브라우저에는 남기지 않는다 (계정 간 섞임 방지)
const store = createStore<DocRecord[]>(MOCK ? INITIAL_DOCS : [], {
  persist: MOCK ? 'docs' : undefined,
  // File 객체는 직렬화할 수 없으므로 저장하지 않는다 (원본은 현재 세션에서만 열람)
  serialize: list => list.map(({ fileBlob: _omit, ...rest }) => rest),
  revive: (raw, fallback) => (Array.isArray(raw) ? raw.map(normalizeDoc).filter((d): d is DocRecord => !!d) : fallback),
});

export const useDocs = store.use;
export const getDocsSnapshot = store.get;

export function getDoc(id: string): DocRecord | undefined {
  return store.get().find(d => d.id === id);
}

export function setDocs(list: DocRecord[]): void {
  store.set(list);
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
  if (c.electronic) return c.reviewStatus === 'needs' ? '보완 필요' : c.reviewStatus === 'approved' ? '서명 완료 · 검토 승인' : '서명 완료 · 검토 중';
  if (c.localSignatureAt) return '서명 완료';
  if (c.reviewStatus === 'approved') return c.kind === 'agreements' ? '서명 필요' : '승인';
  if (c.reviewStatus === 'needs') return '보완 필요';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return '검토 중';
  if (c.localSignatureAt) return '서명 완료';
  return c.kind === 'rights' ? '서류 제출 필요' : '서명 필요';
}

/** 카드 톤 클래스 */
export function docTone(c: DocRecord): string {
  if (c.reviewStatus === 'needs') return 'is-needs';
  if (c.kind === 'agreements' && c.reviewStatus === 'approved' && !c.localSignatureAt) return 'is-to-sign';
  if (c.reviewStatus === 'approved') return 'is-approved';
  if (c.reviewStatus === 'review' || c.reviewStatus === 'prepared') return 'is-review';
  return '';
}
