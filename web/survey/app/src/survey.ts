// 설문 문항·단계 — 기존 정적 HTML(public/index.html)의 문구를 그대로 옮긴 데이터.
// 선택지 값은 배열 순서(0, 1, 2, …)이고, 서버 검증(questions.json, src/worker.js)과 같은 값을 쓴다.
export type QuestionKind = 'single' | 'multi' | 'text';

export interface Question {
  key: string;
  /** 문항 번호 표시 (Q1 …, 베타 문항은 '선택사항') */
  qid: string;
  kind: QuestionKind;
  required: boolean;
  /** 최대 선택 개수 */
  max?: number;
  /** '필수' / '선택' 표시 */
  tag?: string;
  title: string;
  hint?: string;
  /** 선택 개수 표시 */
  count?: boolean;
  twoCols?: boolean;
  options?: string[];
  /** 마지막 선택지(기타)를 고르면 직접 입력칸 */
  other?: boolean;
}

export interface Step {
  kicker: string;
  title: string;
  desc: string;
  questions: Question[];
}

export const STEPS: Step[] = [
  {
    "kicker": "01. ABOUT YOU",
    "title": "음악 활동에 대해",
    "desc": "지금 어떤 음악을 만들고 있는지 알려줘.",
    "questions": [
      {
        "key": "q1",
        "qid": "Q1",
        "kind": "multi",
        "required": true,
        "tag": "필수",
        "title": "현재 어떤 형태로 음악 활동을 하고 계신가요?",
        "hint": "여러 개 선택 가능 · 해당하는 항목을 모두 선택해 주세요",
        "count": true,
        "twoCols": true,
        "options": [
          "솔로 아티스트",
          "싱어송라이터",
          "래퍼 / 힙합 아티스트",
          "밴드",
          "작곡가 / 프로듀서",
          "비트메이커",
          "DJ / 전자음악 아티스트",
          "레이블 / 매니지먼트",
          "취미로 음악 제작 중",
          "데뷔 / 음원 발매 준비 중",
          "기타"
        ],
        "other": true
      },
      {
        "key": "q2",
        "qid": "Q2",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "정식으로 음원을 발매한 경험이 있나요?",
        "hint": "한 가지 선택해 주세요",
        "options": [
          "현재도 지속적으로 발매하고 있다",
          "과거에 발매한 경험이 있다",
          "첫 음원 발매를 준비하고 있다",
          "아직 발매 경험은 없지만 향후 계획이 있다",
          "현재 발매 계획은 없다"
        ]
      },
      {
        "key": "q3",
        "qid": "Q3",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "최근 12개월 동안 정식으로 음원을 얼마나 자주 발매하셨나요?",
        "hint": "한 가지 선택해 주세요",
        "twoCols": true,
        "options": [
          "월 2회 이상",
          "월 1회 정도",
          "2~3개월에 1회",
          "4~6개월에 1회",
          "연 1~2회",
          "최근 12개월 동안 발매한 음원이 없음",
          "아직 정식 음원을 발매한 경험이 없음"
        ]
      },
      {
        "key": "q18",
        "qid": "Q18",
        "kind": "single",
        "required": false,
        "tag": "선택",
        "title": "향후 6개월 이내에 정식 음원을 발매할 계획이 있으신가요?",
        "hint": "한 가지 선택해 주세요 · 향후 발매 계획을 아직 모르시면 건너뛰어도 돼요",
        "options": [
          "1개월 이내",
          "1~3개월 이내",
          "3~6개월 이내",
          "6개월 이후 발매 예정",
          "현재 발매 계획 없음",
          "아직 정해지지 않음"
        ]
      }
    ]
  },
  {
    "kicker": "02. YOUR EXPERIENCE",
    "title": "발매 경험과 불편함",
    "desc": "기존 배급 서비스에서 겪은 경험이 궁금해.",
    "questions": [
      {
        "key": "q4",
        "qid": "Q4",
        "kind": "multi",
        "required": false,
        "tag": "선택",
        "title": "현재 또는 과거에 이용했던 음원 배급 방식은 무엇인가요?",
        "hint": "여러 개 선택 가능 · 해당하는 항목을 모두 선택해 주세요",
        "count": true,
        "twoCols": true,
        "options": [
          "국내 음원 배급사",
          "DistroKid",
          "TuneCore",
          "CD Baby",
          "RouteNote",
          "SoundOn",
          "기타 해외 배급 서비스",
          "소속 레이블 / 기획사를 통한 배급",
          "직접 DSP와 계약",
          "이용한 적 없음",
          "기타"
        ],
        "other": true
      },
      {
        "key": "q5",
        "qid": "Q5",
        "kind": "single",
        "required": false,
        "tag": "선택",
        "title": "현재 또는 이전 배급 서비스에 전반적으로 얼마나 만족하시나요?",
        "hint": "한 가지 선택해 주세요 · 배급 경험이 없다면 건너뛰어도 돼요",
        "options": [
          "1 · 매우 불만족",
          "2 · 불만족",
          "3 · 보통",
          "4 · 만족",
          "5 · 매우 만족"
        ]
      },
      {
        "key": "q6",
        "qid": "Q6",
        "kind": "multi",
        "required": false,
        "max": 5,
        "tag": "선택",
        "title": "기존 음원 배급 서비스를 이용하면서 불편했거나 개선되었으면 했던 점은 무엇인가요?",
        "hint": "여러 개 선택 가능 · 최대 5개까지 · 배급 경험이 없는 경우 자동으로 건너뛰어요",
        "count": true,
        "twoCols": true,
        "options": [
          "배급 수수료가 높음",
          "연간 / 월간 이용료 부담",
          "음원 심사 및 처리가 느림",
          "원하는 날짜에 발매하기 어려움",
          "정산이 느림",
          "최소 출금 금액이 높음",
          "정산 내역을 이해하기 어려움",
          "고객지원 답변이 느림",
          "한국어 지원이 부족함",
          "음원 정보 수정이 어려움",
          "음원 삭제가 번거로움",
          "다른 배급사로 이전하기 어려움",
          "국내 음원 플랫폼 지원이 부족함",
          "해외 음원 플랫폼 지원이 부족함",
          "가사 등록이 불편함",
          "YouTube Content ID 관리가 어려움",
          "스트리밍/매출 통계가 부족함",
          "저작권 또는 분쟁 발생 시 도움을 받기 어려움",
          "특별히 불편한 점 없음",
          "기타"
        ],
        "other": true
      }
    ]
  },
  {
    "kicker": "03. YOUR PRIORITIES",
    "title": "서비스를 고르는 기준",
    "desc": "어떤 조건을 가장 중요하게 생각해?",
    "questions": [
      {
        "key": "q7",
        "qid": "Q7",
        "kind": "multi",
        "required": true,
        "max": 3,
        "tag": "필수",
        "title": "음원 배급사를 선택할 때 가장 중요하게 생각하는 요소는 무엇인가요?",
        "hint": "여러 개 선택 가능 · 최대 3개까지",
        "count": true,
        "twoCols": true,
        "options": [
          "낮은 배급 수수료",
          "빠른 발매 처리",
          "빠른 정산",
          "투명하고 상세한 정산 내역",
          "한국어 고객지원",
          "국내 DSP 지원 범위",
          "해외 DSP 지원 범위",
          "쉬운 음원 업로드",
          "음원 수정 / 삭제 편의성",
          "다른 배급사에서의 이전 편의성",
          "안정성 및 신뢰도",
          "스트리밍 / 매출 분석 기능",
          "마케팅 지원",
          "가사 관련 지원",
          "저작권 / 법률 지원",
          "기타"
        ],
        "other": true
      },
      {
        "key": "q8",
        "qid": "Q8",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "음원을 제출한 후 배급사의 검토 및 승인까지 어느 정도의 시간이 적절하다고 생각하시나요?",
        "hint": "한 가지 선택해 주세요 · DSP에서 실제 발매되는 시점과는 별개예요",
        "options": [
          "24시간 이내",
          "48시간 이내",
          "3영업일 이내",
          "5영업일 이내",
          "1주일 이내",
          "원하는 발매일만 지켜진다면 크게 상관없음"
        ]
      },
      {
        "key": "q9",
        "qid": "Q9",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "가장 선호하는 음원 배급 요금 방식은 무엇인가요?",
        "hint": "한 가지 선택해 주세요",
        "options": [
          "초기/연간 비용 없이 음원 수익의 일부를 배급사가 가져가는 방식",
          "연간 구독료를 내고 음원 수익은 대부분 또는 전부 가져가는 방식",
          "싱글 / 앨범을 발매할 때마다 일정 금액을 지불하는 방식",
          "기본 배급은 무료이고 필요한 부가 기능만 유료로 이용하는 방식",
          "잘 모르겠다",
          "기타"
        ],
        "other": true
      },
      {
        "key": "q10",
        "qid": "Q10",
        "kind": "single",
        "required": false,
        "tag": "선택",
        "title": "가입비와 연회비 없이 기본 음원 배급 서비스를 이용하고, 발생한 음원 수익에서 일정 비율을 수수료로 지불한다면 어느 정도가 적절하다고 생각하시나요?",
        "hint": "한 가지 선택해 주세요",
        "twoCols": true,
        "options": [
          "5% 이하",
          "6~10%",
          "11~15%",
          "16~20%",
          "20% 이상도 서비스가 좋다면 가능",
          "수익 배분 방식 자체를 선호하지 않음",
          "제공 기능과 지원 범위에 따라 다르다"
        ]
      },
      {
        "key": "q15",
        "qid": "Q15",
        "kind": "multi",
        "required": true,
        "tag": "필수",
        "title": "어떤 음원 플랫폼에 배급하고 싶으신가요?",
        "hint": "여러 개 선택 가능 · 해당하는 항목을 모두 선택해 주세요",
        "count": true,
        "twoCols": true,
        "options": [
          "Melon",
          "Genie",
          "FLO",
          "Bugs",
          "Spotify",
          "Apple Music",
          "YouTube Music",
          "Amazon Music",
          "TIDAL",
          "Deezer",
          "Qobuz",
          "기타 국내 음원 플랫폼",
          "기타 해외 음원 플랫폼",
          "잘 모르겠다"
        ]
      }
    ]
  },
  {
    "kicker": "04. WHAT YOU NEED",
    "title": "필요한 기능과 이전 의향",
    "desc": "앞으로 이용하고 싶은 서비스를 선택해 줘.",
    "questions": [
      {
        "key": "q11",
        "qid": "Q11",
        "kind": "multi",
        "required": true,
        "max": 5,
        "tag": "필수",
        "title": "새로운 음원 배급 서비스에서 가장 중요하게 생각하는 기능을 최대 5개 선택해 주세요.",
        "hint": "여러 개 선택 가능 · 최대 5개까지",
        "count": true,
        "twoCols": true,
        "options": [
          "Melon / Genie / Bugs / FLO 등 국내 DSP 배급",
          "Spotify / Apple Music / YouTube Music 등 글로벌 DSP 배급",
          "빠른 음원 심사 및 배급",
          "예약 발매",
          "플랫폼별 실시간 또는 빠른 스트리밍 통계",
          "상세한 매출 / 정산 내역",
          "빠른 출금",
          "공동작업자 간 수익 자동 분배",
          "여러 아티스트 / 레이블 통합 관리",
          "기존 배급사에서 음원 이전 지원",
          "가사 등록 대행",
          "싱크 가사 등록 지원",
          "YouTube Content ID 관리",
          "Spotify for Artists / Apple Music for Artists 관련 지원",
          "메타데이터 자동 오류 검사",
          "발매 일정 관리",
          "플레이리스트 피칭 / 마케팅 지원",
          "저작권 관련 지원",
          "음원 분쟁 발생 시 전문가 / 법률 지원 연계",
          "기타"
        ],
        "other": true
      },
      {
        "key": "q12",
        "qid": "Q12",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "기존 배급사에 등록된 음원을 새로운 배급사로 편리하게 이전할 수 있다면 이용할 의향이 있나요?",
        "hint": "한 가지 선택해 주세요",
        "options": [
          "매우 있다",
          "어느 정도 있다",
          "조건을 비교한 후 결정하겠다",
          "별로 없다",
          "전혀 없다",
          "현재 배급사를 이용하지 않는다"
        ]
      },
      {
        "key": "q16",
        "qid": "Q16",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "음원 배급으로 발생한 수익은 얼마나 자주 정산받고 싶으신가요?",
        "hint": "한 가지 선택해 주세요 · 실제 정산 가능 시점은 플랫폼의 매출 보고 및 지급 일정에 따라 달라질 수 있어요",
        "twoCols": true,
        "options": [
          "월 1회",
          "월 2회",
          "주 1회",
          "정산 가능한 금액을 언제든지 출금",
          "분기별 정산도 괜찮다",
          "정산 내역이 정확하다면 주기는 크게 상관없다",
          "잘 모르겠다"
        ]
      },
      {
        "key": "q17",
        "qid": "Q17",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "기본 배급 수수료를 낮추고 일부 부가 기능을 유료로 제공한다면, 어떤 방식을 선호하시나요?",
        "hint": "한 가지 선택해 주세요",
        "options": [
          "기본 배급 수수료를 낮추고 필요한 부가 기능만 별도 구매",
          "배급 수수료가 높더라도 주요 기능이 기본 포함된 방식",
          "일정한 구독료를 내고 부가 기능을 이용하는 방식",
          "기능과 가격을 비교한 후 결정",
          "잘 모르겠다"
        ]
      }
    ]
  },
  {
    "kicker": "05. ABOUT AUDENIQ",
    "title": "AUDENIQ에 대한 생각",
    "desc": "새로운 음악 배급 서비스에 대한 의견을 들려줘.",
    "questions": [
      {
        "key": "q13",
        "qid": "Q13",
        "kind": "single",
        "required": true,
        "tag": "필수",
        "title": "AUDENIQ과 같은 새로운 국내 음원 배급 서비스가 출시된다면 이용해 볼 의향이 있으신가요?",
        "hint": "한 가지 선택해 주세요",
        "options": [
          "적극적으로 이용해 보고 싶다",
          "기존 서비스와 비교해 보고 결정하겠다",
          "서비스 조건에 따라 이용할 수 있다",
          "현재 이용하는 서비스를 유지할 계획이다",
          "이용할 의향이 없다",
          "잘 모르겠다"
        ]
      },
      {
        "key": "q14",
        "qid": "Q14",
        "kind": "text",
        "required": false,
        "tag": "선택",
        "title": "AUDENIQ에 바라는 점이나 기존 음원 배급 서비스에서 꼭 개선되었으면 하는 점이 있다면 자유롭게 작성해주세요."
      }
    ]
  },
  {
    "kicker": "06. STAY CONNECTED",
    "title": "마지막으로, 베타 참여 안내",
    "desc": "연락처는 선택사항이야. 연락처 없이도 응답할 수 있어.",
    "questions": [
      {
        "key": "beta",
        "qid": "선택사항",
        "kind": "single",
        "required": true,
        "title": "향후 AUDENIQ가 실제 배급 테스트 또는 비공개 베타를 진행하게 될 경우 참여해보고 싶으신가요?",
        "hint": "베타 참여 또는 출시 안내를 원할 때만 연락처를 남겨줘.",
        "options": [
          "적극적으로 참여하고 싶다",
          "자세한 조건을 확인한 후 참여하고 싶다",
          "서비스 출시 소식만 받아보고 싶다",
          "참여 의향 없음"
        ]
      }
    ]
  }
];

/** 여러 개 선택 문항 (제출 시 배열로 보낸다) */
export const MULTI = new Set(STEPS.flatMap(s => s.questions).filter(q => q.kind === 'multi').map(q => q.key));
/** 다른 선택지와 함께 고를 수 없는 선택지 (배급 경험 없음·불편 없음·잘 모르겠다) */
export const EXCLUSIVE: Record<string, string> = { q4: '9', q6: '18', q15: '13' };
/** 배급 경험이 없으면 건너뛰는 문항 */
export const EXPERIENCE_BRANCH = ['q5', 'q6', 'q12'];
export const FIRST = 1;
export const LAST = 18;
