// 화면을 처음 그리는 데는 필요 없는 상호작용 보조 — 첫 화면이 뜬 뒤 한가할 때(또는 처음 누를 때) 한 번에 받는다.
import { initPressFeedback } from './press';
import { initRowNav } from './rowNav';
import { initExpand } from './expand';
import { initTopCover } from './topCover';
import { initEnterNext } from './enterNext';

let started = false;
export function initInteractions() {
  if (started) return;
  started = true;
  // 짧게 톡 눌러도 눌린 모습이 보이게 (.is-pressed)
  initPressFeedback();
  // 탭·칩 줄: 고른 칸이 줄 밖이면 보이게, 화살표 키로 옮기기
  initRowNav();
  // 누르면 나타나는 입력 폼·패널은 높이가 자라며 펼쳐지게
  initExpand();
  // 키보드가 떠 있는 동안 상태 표시줄 칸으로 뒤 내용이 비치지 않게 (iOS 26)
  initTopCover();
  // 폼 밖 입력 화면(발매 신청 등): 키보드의 '다음'으로 다음 칸, 마지막 칸은 '완료'로 키보드 내리기
  initEnterNext();
}
