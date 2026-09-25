import { useEffect, useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { api, type ReleaseDetail as RD } from '../api/client';
import { useAuth } from '../api/auth';
import { StatusPill } from '../components/StatusPill';

export function ReleaseDetail() {
  const { id } = useParams();
  const { org } = useAuth();
  const [rel, setRel] = useState<RD | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!id || !org) return;
    api.getRelease(org.id, id).then(setRel).catch(e => setError(e.message));
  }, [id, org]);

  if (error) return <div className="feedback feedback-error">{error}</div>;
  if (!rel) return <p style={{ color: 'var(--muted)' }}>불러오는 중...</p>;

  return (
    <>
      <Link to="/releases" style={{ color: 'var(--blue)', fontSize: 14 }}>← 발매 목록</Link>
      <div style={{ display: 'flex', alignItems: 'center', gap: 16, margin: '16px 0 24px' }}>
        <h1 className="section-title" style={{ margin: 0 }}>{rel.title}</h1>
        <StatusPill status={rel.status} />
      </div>
      <div className="card" style={{ padding: 28 }}>
        <table className="table">
          <thead><tr><th>#</th><th>제목</th><th>ISRC</th><th>길이</th></tr></thead>
          <tbody>
            {rel.tracks.map((t, i) => (
              <tr key={t.id}>
                <td>{i + 1}</td>
                <td>{t.title}</td>
                <td style={{ fontFamily: 'monospace', fontSize: 13 }}>{t.isrc ?? '-'}</td>
                <td>{t.duration_ms ? `${Math.floor(t.duration_ms / 60000)}:${String(Math.floor(t.duration_ms / 1000) % 60).padStart(2, '0')}` : '-'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
