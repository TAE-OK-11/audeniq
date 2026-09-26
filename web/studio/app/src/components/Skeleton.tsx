// 로딩 스켈레톤 — "불러오는 중..." 텍스트 대신 레이아웃을 미리 보여줘 화면 흔들림을 줄인다.
export function SkeletonRows({ count = 3, avatar = true }: { count?: number; avatar?: boolean }) {
  return (
    <div className="aq-skeleton-list" aria-busy="true" aria-label="불러오는 중">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="aq-skeleton-row" style={{ animationDelay: `${i * 70}ms` }}>
          {avatar && <span className="aq-skel aq-skel-avatar" />}
          <span className="aq-skeleton-lines">
            <span className="aq-skel aq-skel-line" style={{ width: `${62 - i * 7}%` }} />
            <span className="aq-skel aq-skel-line is-sub" style={{ width: `${38 + i * 5}%` }} />
          </span>
          <span className="aq-skel aq-skel-chip" />
        </div>
      ))}
    </div>
  );
}

export function SkeletonBlock({ height = 180 }: { height?: number }) {
  return <div className="aq-skel aq-skel-block" style={{ height }} aria-busy="true" aria-label="불러오는 중" />;
}

export function PageSkeleton() {
  return (
    <div className="aq-page-skeleton" aria-busy="true" aria-label="페이지를 불러오는 중">
      <span className="aq-skel aq-skel-line is-eyebrow" />
      <span className="aq-skel aq-skel-title" />
      <span className="aq-skel aq-skel-line" style={{ width: '46%' }} />
      <SkeletonBlock height={160} />
      <SkeletonRows count={3} />
    </div>
  );
}
