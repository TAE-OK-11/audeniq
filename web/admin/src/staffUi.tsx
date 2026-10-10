// 관리자 화면 공통 조각 — 상태 배지, 오류 줄, 고르기 토글, 하위 탭과 스태프 컨텍스트.
// 버튼·배지·칩·탭·카드·빈 상태 같은 모양은 모두 디자인 시스템(src/ui)을 그대로 쓴다 (스튜디오 src/admin/ui.tsx와 같은 방식).
import { createContext, useContext } from 'react';
import type { Duty, Overview, StaffMe } from './api/staff';
import { Badge, Callout, LinkTabs, Segmented, type Tone } from './ui';

export { Badge, ChipGroup, CountUp, Cover, EmptyState, PageHeader, Section, SkeletonList } from './ui';

/** [이름, 색조] 짝으로 된 상태 값 → 배지 */
export function StatusBadge({ value, dot }: { value: [string, Tone]; dot?: boolean }) {
  return <Badge tone={value[1]} dot={dot}>{value[0]}</Badge>;
}

export function ErrorBox({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <Callout tone="danger" role="alert" action={onRetry ? <button type="button" className="ui-btn is-danger is-sm" onClick={onRetry}>다시 시도</button> : undefined}>
      {message}
    </Callout>
  );
}

// ---------- 스태프 컨텍스트 ----------
interface StaffCtx {
  me: StaffMe;
  counts: Overview | null;
  refreshCounts: () => void;
  can: (d: Duty) => boolean;
}
export const StaffContext = createContext<StaffCtx | null>(null);
export function useStaff(): StaffCtx {
  const v = useContext(StaffContext);
  if (!v) throw new Error('useStaff must be used within AdminApp');
  return v;
}

/** 권한이 없는 작업 버튼 옆에 붙이는 안내 */
export function NoDuty({ duty }: { duty: string }) {
  return <div className="ui-callout">현재 역할로는 <b>{duty}</b> 작업을 할 수 없어요. 조회만 가능해요.</div>;
}

/** 한 메뉴 안의 하위 화면 (발매 심사 | 2차 승인, 배급 현황 | DSP 사양) — 밑줄이 고른 탭으로 미끄러진다 */
export function SubTabs({ tabs }: { tabs: { to: string; label: string; count?: number }[] }) {
  return <LinkTabs tabs={tabs} />;
}

/** 고르기 토글 — [값, 이름] 짝 목록 (담당 · 정렬) */
export function Seg<T extends string>({ value, options, onChange, label, size }: {
  value: T;
  options: readonly (readonly [T, string])[];
  onChange: (v: T) => void;
  label: string;
  size?: 'sm' | 'md';
}) {
  return <Segmented value={value} options={options.map(([k, l]) => ({ value: k, label: l }))} onChange={onChange} label={label} size={size} />;
}
