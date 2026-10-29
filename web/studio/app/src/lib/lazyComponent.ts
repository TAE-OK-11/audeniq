// 필요할 때 받는 화면 조각 — 페이지(App)와 페이지 안의 창·탭이 함께 쓴다.
// 배포 직후 옛 청크가 사라져 동적 import가 실패하면 한 번만 새로고침해 새 버전을 받는다
import { lazy, type ComponentType } from 'react';
import { claimChunkReload } from './chunkRecovery';

export function lazyComponent<M, P = object>(load: () => Promise<M>, pick: (m: M) => ComponentType<P>) {
  return lazy(async () => {
    try {
      const mod = await load();
      return { default: pick(mod) };
    } catch (e) {
      if (claimChunkReload()) {
        window.location.reload();
        await new Promise(() => {}); // 새로고침될 때까지 대기 (오류 화면 깜빡임 방지)
      }
      throw e;
    }
  });
}
