// 서버·기기 문제 알림 — API 호출부가 이벤트를 보내고 SystemStatus가 창·배너로 보여 준다.
export const SERVER_ISSUE_EVENT = 'aq:server-issue';

export type ServerIssue =
  /** 서버·게이트웨이가 응답하지 않음 (502/503/504, 연결 실패) */
  | { kind: 'down'; status: number; code: string }
  /** 서버가 점검 중이라고 답함 (503 MAINTENANCE). 백엔드가 점검 정보를 함께 준다 */
  | { kind: 'maintenance'; status: number; code: string; window?: unknown };

export function reportServerIssue(issue: ServerIssue) {
  try {
    window.dispatchEvent(new CustomEvent<ServerIssue>(SERVER_ISSUE_EVENT, { detail: issue }));
  } catch { /* 이벤트를 못 보내도 요청 오류는 그대로 전달된다 */ }
}

/** 응답 상태·코드가 서버 장애·점검인지.
 * 2026-10-28 수정: 코드를 합치지 않는다. DB 불가(DATABASE_UNAVAILABLE),
 * 상류 불가(502/UPSTREAM_UNAVAILABLE), 타임아웃(504/REQUEST_TIMEOUT),
 * 내부 오류(500/INTERNAL_ERROR)는 모두 서버 장애지만, 표시되는 코드와
 * 상태는 그대로 남겨 원인을 구분할 수 있게 한다. */
export function classifyServerIssue(status: number, code: string): ServerIssue | null {
  if (code === 'MAINTENANCE' || code === 'UNDER_MAINTENANCE') return { kind: 'maintenance', status, code };
  if (
    status === 500 || status === 502 || status === 503 || status === 504
    || code === 'BACKEND_UNAVAILABLE' || code === 'DATABASE_UNAVAILABLE'
    || code === 'UPSTREAM_UNAVAILABLE' || code === 'REQUEST_TIMEOUT'
    || code === 'SERVICE_UNAVAILABLE' || code === 'INTERNAL_ERROR'
  ) {
    return { kind: 'down', status, code: code || `HTTP_${status}` };
  }
  return null;
}
