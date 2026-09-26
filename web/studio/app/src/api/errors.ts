export class ApiError extends Error {
  status: number;
  constructor(message: string, status = 0) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/** 사용자에게 보여줄 오류 문구 */
export function errorMessage(e: unknown, fallback = '문제가 생겼어요. 잠시 후 다시 시도해 주세요.'): string {
  return e instanceof Error && e.message ? e.message : fallback;
}
