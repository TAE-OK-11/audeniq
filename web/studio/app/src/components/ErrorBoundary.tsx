import { Component, type ReactNode } from 'react';
import { ErrorScreen, incidentMeta } from './ErrorScreen';

interface Props {
  children: ReactNode;
  /** 값이 바뀌면 오류 상태를 초기화 (예: 라우트 경로) */
  resetKey?: string;
  /** 앱 전체를 감싸는 경계 (헤더 없이 로고와 함께 화면 전체에 표시) */
  fullPage?: boolean;
}

interface State {
  error: Error | null;
  meta: string[];
}

const isChunkError = (e: Error) =>
  /Loading chunk|dynamically imported module|Importing a module script failed|Failed to fetch/i.test(e.message);

/**
 * 렌더 오류·lazy 청크 로드 실패(배포 후 구 청크 404 등) 시 흰 화면 대신 복구 안내를 보여준다.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, meta: [] };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error, meta: incidentMeta(isChunkError(error) ? 'APP_UPDATED' : 'CLIENT_RENDER') };
  }

  componentDidCatch(error: Error) {
    console.error('[studio] 화면 오류', error);
  }

  componentDidUpdate(prev: Props) {
    if (this.state.error && prev.resetKey !== this.props.resetKey) {
      this.setState({ error: null, meta: [] });
    }
  }

  private retry = () => {
    if (this.state.error && isChunkError(this.state.error)) window.location.reload();
    else this.setState({ error: null, meta: [] });
  };

  render() {
    const { error, meta } = this.state;
    if (!error) return this.props.children;
    const home = import.meta.env.BASE_URL;
    if (isChunkError(error)) {
      return (
        <ErrorScreen
          kind="update" fullPage={this.props.fullPage}
          eyebrow="새 버전"
          title={<>스튜디오가 <em>새 버전</em>으로<br />업데이트됐어요.</>}
          description="새로고침하면 바로 이어서 쓸 수 있어요. 네트워크가 불안정할 때도 이 화면이 보일 수 있어요."
          actions={[{ label: '새로고침', onClick: this.retry, primary: true }, { label: '홈으로', href: home }]}
          meta={meta}
        />
      );
    }
    return (
      <ErrorScreen
        kind="crash" fullPage={this.props.fullPage}
        eyebrow="화면 오류"
        title={<>화면을 그리다가<br /><em>문제가 생겼어요.</em></>}
        description="저장한 내용은 그대로 있어요. 다시 시도해도 같은 화면이 나오면 아래 오류 코드와 함께 문의해 주세요."
        actions={[{ label: '다시 시도', onClick: this.retry, primary: true }, { label: '홈으로', href: home }]}
        meta={meta}
      />
    );
  }
}
