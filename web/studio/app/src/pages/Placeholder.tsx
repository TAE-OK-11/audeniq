export function Placeholder({ eyebrow, title, desc }: { eyebrow: string; title: string; desc: string }) {
  return (
    <>
      <p className="eyebrow">{eyebrow}</p>
      <h1 className="section-title">{title}</h1>
      <p className="section-sub">{desc}</p>
      <div className="card">
        <h3>준비 중</h3>
        <p>디자인 테스트 버전에서는 이 화면이 아직 준비되지 않았어요.</p>
      </div>
    </>
  );
}
