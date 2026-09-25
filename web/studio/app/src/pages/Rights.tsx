import { useState } from 'react';

interface Doc {
  id: string;
  title: string;
  release: string;
  status: 'needed' | 'review' | 'fix';
  date: string;
}

const STATUS_LABEL: Record<Doc['status'], { label: string; chip: string }> = {
  needed: { label: '제출 필요', chip: 'draft' },
  review: { label: '검토 중', chip: 'review' },
  fix: { label: '보완 요청', chip: 'needs' },
};

const INITIAL: Doc[] = [
  { id: 'd1', title: '마스터 음원 권리 확인서', release: '첫 번째 싱글', status: 'review', date: '2026-09-22' },
  { id: 'd2', title: '작사·작곡 크레딧 증빙', release: '여름 EP', status: 'fix', date: '2026-09-23' },
  { id: 'd3', title: '커버아트 사용 허락서', release: '데모 트랙', status: 'needed', date: '2026-09-24' },
];

export function Rights() {
  const [docs, setDocs] = useState<Doc[]>(INITIAL);
  const [msg, setMsg] = useState('');

  const needed = docs.filter(d => d.status === 'needed').length;
  const review = docs.filter(d => d.status === 'review').length;
  const fix = docs.filter(d => d.status === 'fix').length;

  const submit = (id: string) => {
    setDocs(ds => ds.map(d => (d.id === id ? { ...d, status: 'review' as const } : d)));
    setMsg('서류가 제출됐어요. (테스트 모드)');
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">RIGHTS &amp; DOCUMENTS</p>
          <h1>권리·보완 서류</h1>
          <p>발매별 권리 증빙과 AUDENIQ의 보완 요청을 한곳에서 처리하세요.</p>
        </div>
        <button type="button" className="button secondary" onClick={() => setMsg('서류 제출 화면은 테스트 모드에서 생략됩니다.')}>
          권리 서류 제출 ↗
        </button>
      </div>

      {msg && <div className="notice success" style={{ marginBottom: 16 }}>{msg}</div>}

      <div className="aq-rights-summary">
        <div><span>제출할 서류</span><strong>{needed}</strong></div>
        <div><span>검토 중</span><strong>{review}</strong></div>
        <div><span>보완 요청</span><strong>{fix}</strong></div>
      </div>

      <div className="data-list">
        {docs.map(d => {
          const st = STATUS_LABEL[d.status];
          return (
            <div key={d.id} className="track-row">
              <span className="cover cover-small" aria-hidden="true">📁</span>
              <div>
                <span className="row-name">{d.title}</span>
                <span className="row-sub">{d.release} · {d.date}</span>
              </div>
              <div className="row-end">
                <span className={`status-chip ${st.chip}`}>{st.label}</span>
                {d.status !== 'review' && (
                  <button type="button" className="link-btn" onClick={() => submit(d.id)}>
                    제출하기
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        보완 요청이 있으면 요청 사유와 필요한 서류를 확인한 뒤 새 원본을 제출해 주세요.
      </div>
    </>
  );
}
