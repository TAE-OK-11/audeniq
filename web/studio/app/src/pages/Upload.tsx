import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api/client';
import { useAuth } from '../api/auth';

export function Upload() {
  const nav = useNavigate();
  const { org } = useAuth();
  const [title, setTitle] = useState('');
  const [releaseDate, setReleaseDate] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!org) { setError('조직을 선택해주세요'); return; }
    setError('');
    setBusy(true);
    try {
      const r = await api.createRelease(org.id, { title, release_date: releaseDate });
      nav(`/releases/${r.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : '발매 생성 실패');
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="section-title">새 발매</h1>
      {error && <div className="feedback feedback-error">{error}</div>}
      <div className="card" style={{ maxWidth: 560 }}>
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="title">발매 제목</label>
            <input
              id="title" value={title} onChange={e => setTitle(e.target.value)}
              placeholder="예: 첫 번째 싱글" required
            />
          </div>
          <div className="field">
            <label htmlFor="date">발매일</label>
            <input
              id="date" type="date" value={releaseDate}
              onChange={e => setReleaseDate(e.target.value)} required
            />
          </div>
          <button className="btn" disabled={busy}>
            {busy ? '만드는 중...' : '발매 만들기'}
          </button>
        </form>
      </div>
    </>
  );
}
