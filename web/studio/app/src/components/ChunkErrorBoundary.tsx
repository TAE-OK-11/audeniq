import { Component, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * lazy 청크 로드 실패(배포 후 구 청크 404 등) 시 흰 화면 대신
 * 새로고침 안내를 보여주는 경계.
 */
export class ChunkErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch() {
    // 조용한 실패 — 사용자에게는 새로고침 안내만 표시
  }

  private reload = () => {
    window.location.reload();
  };

  render() {
    if (this.state.error) {
      return (
        <div className="view" style={{ padding: '60px 20px', textAlign: 'center' }}>
          <h1 style={{ fontSize: 22, marginBottom: 12 }}>페이지를 불러오지 못했어요.</h1>
          <p style={{ color: 'var(--portal-muted)', marginBottom: 24 }}>
            새 버전이 배포됐거나 네트워크가 불안정할 수 있어요.
          </p>
          <button type="button" className="button" onClick={this.reload}>
            새로고침
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
