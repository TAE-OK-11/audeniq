import { Component, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
  /** 값이 바뀌면 오류 상태를 초기화 (예: 라우트 경로) */
  resetKey?: string;
}

interface State {
  error: Error | null;
}

const isChunkError = (e: Error) =>
  /Loading chunk|dynamically imported module|Importing a module script failed|Failed to fetch/i.test(e.message);

/**
 * 렌더 오류·lazy 청크 로드 실패(배포 후 구 청크 404 등) 시 흰 화면 대신 복구 안내를 보여준다.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error) {
    console.error('[studio] 화면 오류', error);
  }

  componentDidUpdate(prev: Props) {
    if (this.state.error && prev.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  private retry = () => {
    if (this.state.error && isChunkError(this.state.error)) window.location.reload();
    else this.setState({ error: null });
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const chunk = isChunkError(error);
    return (
      <div className="aq-error-state" role="alert">
        <span className="aq-error-icon" aria-hidden="true">!</span>
        <h1>{chunk ? '페이지를 불러오지 못했어요.' : '화면을 표시하는 중 문제가 생겼어요.'}</h1>
        <p>{chunk ? '새 버전이 배포됐거나 네트워크가 불안정할 수 있어요.' : '입력한 내용은 저장돼 있어요. 다시 시도해 주세요.'}</p>
        <div className="row-actions">
          <button type="button" className="button" onClick={this.retry}>{chunk ? '새로고침' : '다시 시도'}</button>
          <a className="button secondary" href={import.meta.env.BASE_URL}>홈으로</a>
        </div>
      </div>
    );
  }
}
