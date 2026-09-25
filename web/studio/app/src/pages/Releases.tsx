import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { api, type Release } from '../api/client';
import { useAuth } from '../api/auth';
import { StatusPill } from '../components/StatusPill';

export function Releases() {
  const { org } = useAuth();
  const [releases, setReleases] = useState<Release[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!org) return;
    api.listReleases(org.id)
      .then(setReleases)
      .catch(e => setError(e.message))
      .finally(() => setLoading(false));
  }, [org]);

  if (!org) return <p style={{ color: 'var(--muted)' }}>조직을 선택해주세요.</p>;

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20 }}>
        <div><h1 className="section-title" style={{ marginBottom: 8 }}>발매</h1><p className="section-sub" style={{ marginBottom: 0 }}>등록한 음원을 관리하세요.</p></div>
        <Link to="/upload" className="button">+ 새 발매</Link>
      </div>

      {loading && <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>}
      {error && <div className="feedback feedback-error">{error}</div>}

      {!loading && !error && releases.length === 0 && (
        <div className="card" style={{ textAlign: 'center', padding: 48 }}>
          <p style={{ color: 'var(--muted)' }}>아직 발매가 없어요.</p>
          <Link to="/upload" className="button" style={{ marginTop: 16 }}>첫 발매 만들기</Link>
        </div>
      )}

      {!loading && releases.length > 0 && (
        <div className="card" style={{ padding: 28 }}>
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
