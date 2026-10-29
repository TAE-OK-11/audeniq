// 약관·방침의 이름·판(version)·주소 — 가입·로그인 요청이 동의한 판을 서버에 남길 때 쓴다.
// 원문(content/*.txt, 100KB가 넘는다)은 약관 화면에서만 lib/terms가 불러온다. 첫 화면이 원문까지 받지 않게 나눠 둔다.
// 원문을 고치면 여기 version을 올린다 (배급 계약서·동의 기록이 어느 판에 동의했는지 남긴다).
export const TERMS_TITLE = 'AUDENIQ 음원 배급 서비스 이용약관';
export const TERMS_VERSION = 'AUD-TERMS 2026.10.1';

/** 앱에서 볼 수 있는 약관 — /terms/<키> */
export type LegalKey = 'service' | 'settlement' | 'privacy';
export const LEGAL_META: Record<LegalKey, { title: string; short: string; version: string; path: string }> = {
  service: { title: TERMS_TITLE, short: '이용약관', version: TERMS_VERSION, path: '/terms' },
  settlement: { title: 'AUDENIQ 정산금 지급 및 계좌 확인 약관', short: '정산 약관', version: 'AUD-SETTLE 2.1', path: '/terms/settlement' },
  privacy: { title: 'AUDENIQ 개인정보 처리방침', short: '개인정보 처리방침', version: 'AUD-PRIVACY 1.0-draft', path: '/privacy' },
};
