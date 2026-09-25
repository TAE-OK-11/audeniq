import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api, type Release } from '../api/client';
import { StatusPill } from '../components/StatusPill';

export function Releases() {
  const [releases, setReleases] = useState<Release[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.listReleases()
      .then(setReleases)
      .catch(e => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20 }}>
        <h1 className="section-title" style={{ margin: 0 }}>발매</h1>
        <Link to="/upload" className="btn">+ 새 발매</Link>
      </div>

      {loading && <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>}
      {error && <div className="feedback feedback-error">{error}</div>}

      {!loading && !error && releases.length === 0 && (
        <div className="card" style={{ textAlign: 'center', padding: 48 }}>
          <p style={{ color: 'var(--muted)' }}>아직 발매가 없어요.</p>
          <Link to="/upload" className="btn" style={{ marginTop: 16 }}>첫 발매 만들기</Link>
        </div>
      )}

      {!loading && releases.length > 0 && (
        <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
          <table className="table">
            <thead>
              <tr><th>제목</th><th>상태</th><th>트랙</th><th>발매일</th></tr>
            </thead>
            <tbody>
              {releases.map(r => (
                <tr key={r.id}>
                  <td><Link to={`/releases/${r.id}`} style={{ fontWeight: 600 }}>{r.title}</Link></td>
                  <td><StatusPill status={r.status} /></td>
                  <td>{r.track_count}</td>
                  <td>{r.release_date ?? '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
