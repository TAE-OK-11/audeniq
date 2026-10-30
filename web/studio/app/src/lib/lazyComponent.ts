// 필요할 때 받는 화면 조각 — 페이지(App)와 페이지 안의 창·탭이 함께 쓴다.
// 배포 직후 옛 청크가 사라져 동적 import가 실패하면 한 번만 새로고침해 새 버전을 받는다
import { lazy, type ComponentType } from 'react';
import { claimChunkReload, isChunkError } from './chunkRecovery';

/** 잠깐 끊긴 회선(지하철·엘리베이터)이면 한 번 더 받아 본다 — 새로고침하면 쓰던 내용이 사라진다 */
const RETRY_MS = 700;

export function lazyComponent<M, P = object>(load: () => Promise<M>, pick: (m: M) => ComponentType<P>) {
  return lazy(async () => {
    let mod: M;
    try {
      mod = await load().catch(async e => {
        if (!navigator.onLine) throw e;
        await new Promise(r => setTimeout(r, RETRY_MS));
        return load();
      });
    } catch (e) {
      // 오프라인이면 새로고침하지 않는다 — 브라우저 오프라인 화면이 뜨고 쓰던 내용이 사라졌다 (오류 화면에서 다시 시도)
      if (navigator.onLine && isChunkError(e) && claimChunkReload()) {
        window.location.reload();
        await new Promise(() => {}); // 새로고침될 때까지 대기 (오류 화면 깜빡임 방지)
      }
      throw e;
    }
    return { default: pick(mod) };
  });
}
