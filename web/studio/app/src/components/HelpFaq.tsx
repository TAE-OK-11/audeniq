// 자주 묻는 질문 — 문의를 쓰기 전에 스스로 답을 찾을 수 있게 (문의 화면 위). 내용은 서비스 규칙(발매 검수 기간·정산 약관)과 맞춘다
import type { ReactNode } from 'react';
import { Link } from '../lib/router';
import { LEAD_DAYS } from '../lib/dsp';

const FAQ: { q: string; a: ReactNode }[] = [
  {
    q: '발매를 신청하면 언제 플랫폼에 나오나요?',
    a: <>검수와 플랫폼 납품에 {LEAD_DAYS.normal}일이 필요해서 발매일은 신청한 날로부터 {LEAD_DAYS.normal}일 뒤부터 고를 수 있어요. 급하면 발매 신청의 ‘신속 발매 요청’으로 최소 {LEAD_DAYS.express}일 뒤 발매를 요청할 수 있어요(가능 여부는 확인 후 안내해요). 플랫폼마다 화면에 보이는 시점은 조금씩 다를 수 있어요.</>,
  },
  {
    q: '보완 요청을 받았어요. 어떻게 고치나요?',
    a: <>홈의 ‘확인할 작업’이나 <Link to="/releases?status=needs">발매 목록</Link>에서 ‘보완하기’를 누르면 고칠 칸으로 바로 가요. 고친 뒤 다시 접수하면 다시 검토해요.</>,
  },
  {
    q: '정산은 언제 되나요?',
    a: <>운영팀이 확인한 정산금을 넣어 드려요. 들어오면 <Link to="/settlement">정산</Link> 화면 잔액에 바로 쌓이고, 원할 때 지급을 요청해 받을 수 있어요.</>,
  },
  {
    q: '정산금은 어떻게 받나요?',
    a: <>정산 계좌를 등록한 뒤 <Link to="/settlement">정산</Link>에서 ‘수익 받기’로 출금을 신청해요. 수수료(30,000원 이하는 500원, 초과는 무료)와 실제로 받는 금액은 신청 화면에서 미리 보여 드려요. 최소 출금 금액이 있을 때는 정산 화면의 ‘수익 받기’에서 함께 안내해요.</>,
  },
  {
    q: '계약서와 권리 서류는 어디서 보나요?',
    a: <>메뉴의 <Link to="/contracts">계약서</Link>와 <Link to="/rights">권리·보완 서류</Link>에서 서명하거나 제출한 문서를 모두 볼 수 있어요.</>,
  },
];

export function HelpFaq() {
  return (
    <section className="aq-faq" aria-labelledby="faqTitle">
      <h2 id="faqTitle">자주 묻는 질문</h2>
      <div className="aq-faq-list">
        {FAQ.map(f => (
          <details key={f.q} className="aq-faq-item">
            <summary>{f.q}<span className="aq-faq-chev" aria-hidden="true" /></summary>
            <p>{f.a}</p>
          </details>
        ))}
      </div>
      <p className="aq-faq-foot">찾는 답이 없으면 위의 ‘새 문의’로 남겨 주세요.</p>
    </section>
  );
}
