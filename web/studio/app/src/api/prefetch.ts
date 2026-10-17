// 누르는 순간 미리 받기 — 손가락이 닿고(pointerdown) 떼어 화면이 바뀌기까지 100ms 남짓을 데이터 받는 데 쓴다.
// 받은 데이터는 화면 데이터 캐시(lib/asyncCache)에 넣어 두어, 상세 화면이 자리표시 없이 바로 열린다.
// 같은 요청이 이미 가고 있으면 그 응답을 함께 기다린다(api/http req).
import { api } from './client';
import { orgId } from './http';
import { cacheHas, cacheSet } from '../lib/asyncCache';
import { prefetchRoute } from '../routes';

/** 발매 상세(데이터 + 화면 코드)를 미리 */
export function prefetchRelease(id: string) {
  prefetchRoute(`/releases/${id}`);
  const key = `${orgId()}|release:${id}`;
  if (!id || cacheHas(key)) return;
  api.getRelease(id).then(r => cacheSet(key, r), () => {});
}
