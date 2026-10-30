// 로딩 스켈레톤 — "불러오는 중..." 텍스트 대신 레이아웃을 미리 보여줘 화면 흔들림을 줄인다.
/** 목록 뼈대 — pattern은 실제 줄 모양에 맞춘다 (토스 Skeleton처럼 화면마다 같은 모양을 미리 보여 줘야 바뀔 때 덜 흔들린다)
 *  list: 커버·제목·배지 (발매·곡 목록) · amount: 둥근 아이콘·제목·오른쪽 금액 (정산·지급 내역) */
export function SkeletonRows({ count = 3, avatar = true, pattern = 'list' }: { count?: number; avatar?: boolean; pattern?: 'list' | 'amount' }) {
  if (pattern === 'amount') {
    return (
      <div className="aq-skeleton-list is-amount" aria-busy="true" aria-label="불러오는 중">
        {Array.from({ length: count }, (_, i) => (
          <div key={i} className="aq-skeleton-row" style={{ animationDelay: `${i * 70}ms` }}>
            <span className="aq-skel aq-skel-avatar" />
            <span className="aq-skeleton-lines">
              <span className="aq-skel aq-skel-line" style={{ width: `${58 - i * 9}%` }} />
              <span className="aq-skel aq-skel-line is-sub" style={{ width: `${34 + i * 6}%` }} />
            </span>
            <span className="aq-skel aq-skel-line aq-skel-amount" style={{ width: `${68 - i * 10}%` }} />
          </div>
        ))}
      </div>
    );
  }
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

/** 제목 한 줄 + 문단 두세 줄 — 안내문·초대장처럼 글로 된 화면 */
export function SkeletonLines() {
  return (
    <div className="aq-page-skeleton" aria-busy="true" aria-label="불러오는 중">
      <span className="aq-skel aq-skel-title" />
      <span className="aq-skel aq-skel-line" style={{ width: '92%' }} />
      <span className="aq-skel aq-skel-line" style={{ width: '84%', marginTop: 10 }} />
      <span className="aq-skel aq-skel-line" style={{ width: '58%', marginTop: 10 }} />
    </div>
  );
}

export function SkeletonBlock({ height = 180, className = '' }: { height?: number; className?: string }) {
  return <div className={`aq-skel aq-skel-block${className ? ` ${className}` : ''}`} style={{ height }} aria-busy="true" aria-label="불러오는 중" />;
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

/** 계정 정보 설정 창(패스키·연결된 계정·AI 앱)의 줄 모양 그대로 — 글자 한 줄 대신, 받으면 그 자리에 줄이 채워진다 */
export function SkeletonAcctRows({ count = 2 }: { count?: number }) {
  return (
    <ul className="aq-acct-list aq-acct-skel" aria-busy="true" aria-label="불러오는 중">
      {Array.from({ length: count }, (_, i) => (
        <li key={i}>
          <span className="aq-skel aq-acct-ico" />
          <span className="aq-skel-lines">
            <span className="aq-skel" style={{ width: `${46 - i * 8}%` }} />
            <span className="aq-skel" style={{ width: `${70 - i * 6}%` }} />
          </span>
          <span />
        </li>
      ))}
    </ul>
  );
}
