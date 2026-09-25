import { useState } from 'react';

interface Contract {
  id: string;
  title: string;
  date: string;
  signed: boolean;
}

const INITIAL: Contract[] = [
  { id: 'c1', title: '배급 계약서', date: '2026-09-20', signed: true },
  { id: 'c2', title: '발매 신청서', date: '2026-09-24', signed: false },
];

export function Contracts() {
  const [contracts, setContracts] = useState<Contract[]>(INITIAL);
  const [signedMsg, setSignedMsg] = useState('');

  const sign = (id: string) => {
    setContracts(cs => cs.map(c => (c.id === id ? { ...c, signed: true } : c)));
    setSignedMsg('서명이 접수됐어요. (테스트 모드)');
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">AUDENIQ AGREEMENTS</p>
          <h1>계약서</h1>
          <p>AUDENIQ과 체결하는 배급 계약과 발매 신청서를 확인하고 서명하세요.</p>
        </div>
      </div>

      <div className="studio-doc-path">
        <div>
          <span className="studio-path-n">01</span>
          <strong>신청 정보 확인</strong>
          <small>발매·권리자 정보 자동 정리</small>
        </div>
        <div>
          <span className="studio-path-n">02</span>
          <strong>계약 내용 확인</strong>
          <small>배급 범위와 필수 조항 확인</small>
        </div>
        <div>
          <span className="studio-path-n">03</span>
          <strong>서명·접수</strong>
          <small>서명 후 AUDENIQ에 즉시 접수</small>
        </div>
      </div>

      {signedMsg && <div className="notice success" style={{ marginBottom: 16 }}>{signedMsg}</div>}

      <div className="data-list">
        {contracts.map(c => (
          <div key={c.id} className="track-row">
            <span className="cover cover-small" aria-hidden="true">📄</span>
            <div>
              <span className="row-name">{c.title}</span>
              <span className="row-sub">{c.date}</span>
            </div>
            <div className="row-end">
              {c.signed ? (
                <span className="status-chip live">서명 완료</span>
              ) : (
                <button type="button" className="button secondary" onClick={() => sign(c.id)}>
                  서명하기
                </button>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="notice" style={{ marginTop: 25 }}>
        발매 신청 정보는 계약서에 자동으로 정리돼요. 내용을 확인한 뒤 서명만 진행하면 돼요.
      </div>
    </>
  );
}
