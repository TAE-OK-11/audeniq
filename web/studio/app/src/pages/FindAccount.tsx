import { useState } from 'react';
import { Link } from '../lib/router';
import { AuthLayout } from '../components/AuthLayout';

// 백엔드에 계정 찾기 API가 아직 없어서 프론트엔드 흐름만 구현.
// API가 생기면 이 파일의 submit 부분만 교체하면 된다.
export function FindAccount() {
  const [tab, setTab] = useState<'id' | 'password'>('id');
  const [name, setName] = useState('');
  const [phone, setPhone] = useState('');
  const [email, setEmail] = useState('');
  const [done, setDone] = useState<string | null>(null);

  function submitId(e: React.FormEvent) {
    e.preventDefault();
    // TODO: POST /api/auth/find-id { name, phone } 연동 — API 준비 전까지 실제 발송 없음
    setDone(`계정 찾기 기능은 현재 준비 중이에요.\n가입하신 이메일이 기억나지 않으면 문의하기로 연락해 주세요.`);
  }

  function submitPw(e: React.FormEvent) {
    e.preventDefault();
    // TODO: POST /api/auth/reset-password { email } 연동 — API 준비 전까지 실제 발송 없음
    setDone(`비밀번호 재설정 기능은 현재 준비 중이에요.\n급하시면 문의하기로 연락해 주세요.`);
  }

  const reset = () => { setDone(null); };

  return (
    <AuthLayout title="계정을 찾아드릴게요." sub="가입할 때 입력한 정보를 입력해 주세요.">
      <div className="tabs aq-tabs" role="tablist" aria-label="계정 찾기 구분">
        <button type="button" role="tab" className="tab" aria-selected={tab === 'id'}
          onClick={() => { setTab('id'); reset(); }}>아이디 찾기</button>
        <button type="button" role="tab" className="tab" aria-selected={tab === 'password'}
          onClick={() => { setTab('password'); reset(); }}>비밀번호 찾기</button>
      </div>

      {done ? (
        <div className="auth-done">
          <span className="aq-done-icon" aria-hidden="true">i</span>
          <p style={{ whiteSpace: 'pre-line' }}>{done}</p>
          <Link to="/login" className="button auth-submit">로그인으로 돌아가기</Link>
        </div>
      ) : tab === 'id' ? (
        <form onSubmit={submitId}>
          <div className="field">
            <label htmlFor="findName">이름</label>
            <input id="findName" type="text" required autoComplete="name"
              placeholder="이름을 입력해 주세요"
              value={name} onChange={e => setName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="findPhone">휴대폰 번호</label>
            <input id="findPhone" type="tel" required autoComplete="tel"
              placeholder="010-0000-0000"
              value={phone} onChange={e => setPhone(e.target.value)} />
          </div>
          <button className="button auth-submit" type="submit">아이디 찾기</button>
        </form>
      ) : (
        <form onSubmit={submitPw}>
          <div className="field">
            <label htmlFor="findEmail">이메일</label>
            <input id="findEmail" type="email" required autoComplete="email"
              placeholder="가입한 이메일을 입력해 주세요"
              value={email} onChange={e => setEmail(e.target.value)} />
          </div>
          <button className="button auth-submit" type="submit">재설정 링크 보내기</button>
        </form>
      )}

      <div className="auth-links">
        <Link to="/login">로그인</Link>
        <span aria-hidden="true">·</span>
        <Link to="/signup">회원가입</Link>
      </div>
    </AuthLayout>
  );
}
