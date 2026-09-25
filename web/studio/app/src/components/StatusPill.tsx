// Exact .pill from studio HTML — single style, status conveyed by text
const LABELS: Record<string, string> = {
  LIVE: '배포 중',
  DELIVERED: '전송됨',
  STAGE1_PASSED: '심사 통과',
  STAGE1_CORRECTION: '수정 필요',
  DRAFT: '초안',
};

export function StatusPill({ status }: { status: string }) {
  return <span className="pill">{LABELS[status] ?? status}</span>;
}
