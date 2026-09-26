// 서버·기기 문제 알림 — API 호출부가 이벤트를 보내고 SystemStatus가 창·배너로 보여 준다.
export const SERVER_ISSUE_EVENT = 'aq:server-issue';

export type ServerIssue =
  /** 서버·게이트웨이가 응답하지 않음 (502/503/504, 연결 실패) */
  | { kind: 'down'; status: number; code: string }
  /** 서버가 점검 중이라고 답함 (503 MAINTENANCE) */
  | { kind: 'maintenance'; status: number; code: string };

export function reportServerIssue(issue: ServerIssue) {
  try {
    window.dispatchEvent(new CustomEvent<ServerIssue>(SERVER_ISSUE_EVENT, { detail: issue }));
  } catch { /* 이벤트를 못 보내도 요청 오류는 그대로 전달된다 */ }
}

/** 응답 상태·코드가 서버 장애·점검인지 */
export function classifyServerIssue(status: number, code: string): ServerIssue | null {
  if (code === 'MAINTENANCE' || code === 'UNDER_MAINTENANCE') return { kind: 'maintenance', status, code };
  if (status === 502 || status === 503 || status === 504 || code === 'BACKEND_UNAVAILABLE' || code === 'DATABASE_UNAVAILABLE') {
    return { kind: 'down', status, code: code || `HTTP_${status}` };
  }
  return null;
}
