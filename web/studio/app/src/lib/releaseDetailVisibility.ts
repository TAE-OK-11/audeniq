/** 작성 중에는 입력 경로와 실제 자료가 있는 탭만 노출한다. 변경 기록은 사용자 화면에 두지 않는다 */
export function releaseDetailVisibility(status: string, tracks: number, hasDocuments: boolean) {
  const draft = status === 'draft';
  return {
    overview: true,
    stats: status === 'live' || status === 'closed',
    delivery: !draft || hasDocuments,
    // 수익 분배는 접수 전에만 수정할 수 있어 작성 단계에서도 필요하다.
    // 퍼블리싱(작사·작곡 저작권)은 접수한 앨범의 곡이 있을 때부터
    publishing: !draft && tracks > 0,
    manage: !draft || tracks > 0,
  };
}
