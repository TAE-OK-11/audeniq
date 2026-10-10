// 실시간 연결 이벤트 이름 — 화면 쪽에서 들을 때 동기화 모듈(portalSync)을 끌어오지 않도록 따로 둔다.
/** 문의가 바뀌었다 (detail: { inquiryId }) — 열려 있는 대화가 다시 받는다 */
export const INQUIRY_CHANGED_EVENT = 'aq:inquiry-changed';
/** 새 알림 (detail: Notice) — 화면을 보고 있으면 토스트로 띄운다 */
export const LIVE_NOTICE_EVENT = 'aq:live-notice';
