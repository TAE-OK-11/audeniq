// 퍼블리싱을 쓰는 사람인지 — 쓰지 않는 사람에게는 '곡 저작권도 맡기기(선택)' 입구 하나만 보이고,
// 곡 목록·분석·작가 정보 같은 퍼블리싱 화면은 숨긴다 (배급만 쓰는 사람에게 낯선 칸이 늘지 않게).
//  · started: 곡을 하나라도 정리했거나 신청한 적이 있다 → 곡 저작권 목록, 계약 묶음의 저작권 줄, 작가 정보
//  · enrolled: 저작권 관리를 신청한 적이 있다 → 저작권료 분석(음악 리포트 '저작권료')
// 홈·리포트·계약서가 같은 요약(pub-summary)을 함께 쓴다 — 한 번만 받는다
import { pubApi } from '../api/publishing';
import { useAsync } from './useAsync';

export function usePublishingUse() {
  const { data } = useAsync(() => pubApi.summary(), [], { key: 'pub-summary' });
  return {
    /** 요약을 받았는지 — 받기 전에는 아무것도 그리지 않는다(입구가 깜빡였다 사라지지 않게) */
    ready: !!data,
    started: !!data && (data.enrolled || data.works.total > 0),
    enrolled: !!data?.enrolled,
  };
}
