// 서류 원본 열기 — 아티스트가 올린 파일(PDF·이미지)을 새 탭에서 연다 (GET /api/staff/documents/{id}/file).
// 같은 출처의 로그인 쿠키로 받으므로 따로 토큰이 필요 없고, 열람은 서버 감사 기록에 남는다.
import { API_BASE } from '../api/http';
import { MOCK } from '../lib/mode';
import { useToast } from '../components/Toast';
import { Glyph } from '../components/Glyph';

export function DocFileLink({ id, assetId }: { id: string; assetId?: string | null }) {
  const toast = useToast();
  if (!assetId) return null;
  if (MOCK) {
    return (
      <button type="button" className="adm-btn soft small" onClick={() => toast('체험 데이터에는 원본 파일이 없어요. 실서버에서는 새 탭에서 열려요.', 'info')}>
        원본 보기
      </button>
    );
  }
  return (
    <a className="adm-btn soft small" href={`${API_BASE}/api/staff/documents/${encodeURIComponent(id)}/file`} target="_blank" rel="noopener noreferrer">
      원본 보기 <Glyph name="arrow-up-right" size={12} />
    </a>
  );
}
