const STATUS_MAP: Record<string, { cls: string; label: string }> = {
  LIVE: { cls: 'pill-live', label: '배포 중' },
  DELIVERED: { cls: 'pill-pending', label: '전송됨' },
  STAGE1_PASSED: { cls: 'pill-pending', label: '심사 통과' },
  STAGE1_CORRECTION: { cls: 'pill-blocked', label: '수정 필요' },
  DRAFT: { cls: 'pill-draft', label: '초안' },
};

export function StatusPill({ status }: { status: string }) {
  const s = STATUS_MAP[status] ?? { cls: 'pill-review', label: status };
  return <span className={`pill ${s.cls}`}>{s.label}</span>;
}
