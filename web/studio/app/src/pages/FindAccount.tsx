import { useState } from 'react';
import { Link } from '../lib/router';
import { AuthLayout } from '../components/AuthLayout';

// 계정 복구 API와 메일 발송이 연결되기 전에는 입력을 수집하거나 발송을 약속하지 않는다.
export function FindAccount() {
  const [tab, setTab] = useState<'id' | 'password'>('id');

  return (
    <AuthLayout title="계정 이용에 도움이 필요하신가요?" sub="계정 찾기와 비밀번호 재설정은 고객지원에서 안내해 드려요.">
      <div className="tabs aq-tabs" role="tablist" aria-label="계정 찾기 구분">
        <button type="button" role="tab" className="tab" aria-selected={tab === 'id'}
          onClick={() => setTab('id')}>아이디 찾기</button>
        <button type="button" role="tab" className="tab" aria-selected={tab === 'password'}
          onClick={() => setTab('password')}>비밀번호 찾기</button>
      </div>
      <div className="auth-done" role="tabpanel">
        <span className="aq-done-icon" aria-hidden="true">i</span>
        <p>{tab === 'id' ? '가입한 이메일이 기억나지 않으면' : '비밀번호를 재설정해야 한다면'} 고객지원에 문의해 주세요. 자동 복구와 메일 발송은 아직 제공되지 않아요.</p>
        <a href="mailto:audeniq.official@gmail.com" className="button auth-submit">고객지원에 이메일 보내기</a>
      </div>

      <div className="auth-links">
        <Link to="/login">로그인</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
