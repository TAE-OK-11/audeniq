# 언어별 검색 의도와 키워드

2026-10-10 작성. 검색량·경쟁도 수치는 검증 가능한 자료(검색 콘솔·키워드 도구 데이터)가 없어 적지 않았다. 아래는 AUDENIQ의 실제 사업 범위(출시 준비 중인 음원 배급·음악 퍼블리싱·레이블 운영)와 각 언어권 음악 업계에서 쓰는 표현을 기준으로 정한 **의도 지도**다. 측정이 시작되면 검색 콘솔의 실제 검색어로 이 표를 교체·보강한다.

## 원칙

- 페이지 본문에 이미 있는 사실만으로 답한다. 키워드 나열 영역·숨긴 텍스트·검색엔진별 다른 본문은 만들지 않는다.
- 영어 키워드를 직역하지 않고 각 언어권의 업계 용어를 쓴다(예: 일본어 "音楽配信"·"音楽出版", 독일어 "Musikvertrieb"·"Musikverlag").
- 브랜드 질의(AUDENIQ, 어데닉)는 모든 언어의 title·FAQ 첫 질문·JSON-LD alternateName으로 연결한다.
- "출시 준비 중" 상태가 핵심 정보다. "지금 발매 가능?" 질의에는 모든 언어에서 "아직 아니다"로 먼저 답한다.

## 대상 의도

| 의도 | 대표 질문 | 답하는 위치 |
| --- | --- | --- |
| 브랜드 정의 | AUDENIQ/어데닉이 뭐야? | 홈 FAQ 1, /about/, llms.txt |
| 배급사 여부 | AUDENIQ는 음원 배급사야? | 홈 FAQ 2 |
| 이용 가능 여부 | 지금 발매·신청 가능? | 홈 FAQ 3, 히어로 상태 문구, 배급 안내 FAQ |
| 지원 플랫폼 | 어떤 DSP에 배급돼? | 홈 FAQ 4(미확정 명시) |
| 개념 비교 | 음원 배급과 음악 퍼블리싱 차이 | 홈 FAQ 5, /publishing/ |
| 퍼블리싱 | AUDENIQ Publishing은 뭐야? | 홈 FAQ 6, /publishing/ |
| 권리 | 배급하면 권리가 넘어가? | 홈 FAQ 7, 배급 안내 |
| 이전 | 기존 음원 옮길 수 있어? | 홈 FAQ 8, 배급 안내 |
| 대상 | 인디 아티스트·레이블도 돼? | 홈 FAQ 9, /label-services/ |

## 언어별 핵심 표현

| 언어 | 일반(비브랜드) 검색 표현 | 페이지 반영 |
| --- | --- | --- |
| 한국어 | 음원 배급, 음원 유통, 인디 음원 발매, 음악 퍼블리싱, 저작권료 관리, 독립 레이블 정산, 배급사 이전 | title·description·FAQ·안내 페이지 5종 |
| English | independent music distribution, music distribution for indie artists, digital music distribution, independent music publishing, music publishing administration, music rights management, royalty reporting | /en/ title·description, en 안내 페이지 5종 |
| 日本語 | 音楽配信 ディストリビューション, インディーズ 配信, 楽曲リリース, 音楽出版, 著作権使用料 管理, インディーレーベル | /ja/ |
| 简体中文 | 数字音乐发行, 独立音乐人 发行, 音乐出版, 词曲版权管理, 版税, 独立厂牌 | /zh-cn/ (Baidu 대응은 아래 참고) |
| 繁體中文 | 數位音樂發行, 獨立音樂人, 音樂出版, 詞曲版權管理, 版稅, 獨立廠牌 | /zh-tw/ |
| Español | distribución digital de música, distribuidora de música para artistas independientes, editorial / edición musical, regalías, sellos independientes | /es/ |
| Português (BR) | distribuição digital de música, distribuidora de música, edição musical, editora musical, royalties, selo independente | /pt-br/ |
| Français | distribution musicale, distributeur de musique numérique, édition musicale, droits d’auteur, label indépendant | /fr/ |
| Deutsch | digitaler Musikvertrieb, Musikvertrieb für Independent-Künstler, Musikverlag, Tantiemen, Indie-Label | /de/ |
| Bahasa Indonesia | distribusi musik digital, agregator musik, penerbitan musik, royalti, musisi independen, label independen | /id/ |
| Tiếng Việt | phân phối nhạc số, phát hành nhạc, xuất bản âm nhạc, tiền bản quyền, nghệ sĩ độc lập, hãng đĩa độc lập | /vi/ |
| ไทย | จัดจำหน่ายเพลงดิจิทัล, ปล่อยเพลงลงสตรีมมิง, มิวสิคพับลิชชิ่ง, ค่าลิขสิทธิ์, ศิลปินอิสระ, ค่ายเพลงอิสระ | /th/ |
| हिन्दी | म्यूज़िक डिस्ट्रीब्यूशन, स्वतंत्र कलाकार, म्यूज़िक पब्लिशिंग, रॉयल्टी, इंडी लेबल | /hi/ |
| العربية | التوزيع الرقمي للموسيقى, شركة توزيع موسيقي, النشر الموسيقي, عائدات حقوق المؤلف, الفنانين المستقلين | /ar/ |

## 지역 검색엔진 메모

- Naver: 한국어 루트가 대상. Yeti 허용, sitemap·RSS 제출은 Search Advisor 계정 필요.
- Baidu: hreflang을 사용하지 않고 중국 본토 접속 품질·ICP 비안이 노출에 영향을 준다. 이번 범위에서는 기술적 접근(robots 허용, 간체 페이지 SSR)만 보장했다.
- Yandex: hreflang·sitemap을 지원하며 IndexNow 참여 엔진이다. Webmaster 등록은 계정이 필요하다.
- Seznam·Naver 등 IndexNow 참여 엔진은 api.indexnow.org 한 번 제출로 공유된다.

## 번역 품질

모든 번역은 한국어 원문 문장 단위로 대응하며(`content/i18n/<code>.json`의 키가 한국어 원문), 원문에 없는 사실을 추가하지 않았다. 원어민 검토 전이라도 공개할 수 있는 수준으로 작성했지만, 출시 전 각 언어 원어민 검토를 권장한다. 검토 중인 언어는 `content/locales.json`에서 `"status": "draft"`로 바꾸면 URL·sitemap·hreflang·언어 메뉴에서 즉시 빠진다.
