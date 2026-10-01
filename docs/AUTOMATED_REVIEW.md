# 배급 전 자동 심사 — 2026-10-01

새 제출본과 아직 검증 패키지가 고정되지 않은 재심사에 적용한다. 이미 고정된 제출본·심사 패키지는 수정하지 않는다. 심사 규칙은 Stage 2 v3, 파일 QC v6, 로컬 출처 검사 v1, 배급 콘텐츠 정책 v1이다.

## 직원 검토 전에 시스템이 처리하는 항목

| 검사 | 시스템 처리 |
| --- | --- |
| 제출본·동의·Stage 1 패키지 | 조직·제출본 ID·본문 해시·동의 해시·모든 트랙의 파일 ID와 SHA-256을 대조한다. 잘못된 패키지나 누락된 검사 참조는 통과할 수 없다. |
| 동의 및 참여자 | 동의의 발매 대상·정책·유효기간, 성인·배급 권리 선언, 크레딧 참여자의 조직 소속을 검사한다. 명확한 오류는 보완 상태로 돌려보낸다. |
| 등록 파일 | 제출 시점의 모든 음원과 커버 해시가 현재 등록 파일과 같은지 검사한다. 1차 검사 후 등록 정보가 변경된 파일은 보류한다. |
| 권리 부여 관계 | 해당 발매와 트랙의 권리만 조사한다. 부모 권리의 누락·취소·다른 조직 소속·순환·과도한 깊이를 검출한다. 독점권의 기간·지역·이용 범위가 겹치는 경우만 충돌로 취급한다. 외부 계약의 진위를 DB 행 존재만으로 승인하지 않는다. |
| 가사 | 실제 가사를 제출본과 동의 범위에 고정한다. 가사가 있는 곡의 작사 크레딧 누락, 연주곡 표시와 가사의 충돌을 보완 요청한다. 명시적인 AI 생성 문구·챗봇 문구는 출처 확인 대상으로 기록한다. |
| 콘텐츠 선언 | 제목의 커버·리믹스·샘플링·AI 생성 문구와 선언을 대조한다. 유니코드 정규화와 단어 경계를 적용한다. `Discovery`처럼 정상적인 단어를 커버곡으로 판정하지 않는다. |
| 음원·커버 출처 | 로컬 ExifTool로 생성 도구 및 IPTC DigitalSourceType 메타데이터를 읽는다. 생성 도구 신호는 직원에게 근거와 함께 전달한다. 음원 정규화 전에 원본 정보를 검사하고 원본·마스터 해시에 연결해 보존한다. |
| DSP 납품 | 기존 DSP별 아트워크·음원·크레딧·장르·일정·파트너·DDEX 검사를 수행한다. Stage 1의 정확한 검사 참조를 사용해 과거 재시도 결과가 섞이지 않도록 한다. |

검사에 수정 항목과 수동 확인 항목이 함께 있으면 수정 항목을 먼저 돌려보낸다. 재제출 시 모든 심사가 다시 수행되므로 수동 확인 항목이 사라진 것으로 취급하지 않는다.

## 정상 건의 자동 진행

현재 제출본이 READY_FOR_DELIVERY이고, Stage 2 v3와 파일 QC v6가 PASS이며, 가사가 제출본에 고정되어 있고, 권리 epoch가 일치하고, 동의가 아직 유효하며, 예외 승인과 특수 콘텐츠 선언이 없는 경우에만 자동 배급 승인을 시도한다. 각 DSP는 READY·ROUTABLE이어야 하며, 전송 문서가 미리보기가 아니어야 한다. 모든 경고와 미지의 검사 항목은 자동 승인 대상에서 제외한다. 안내 항목 중 `DSP_AUDIO_SERVED_DOWNSAMPLED`만 허용한다. DDEX 문서는 저장된 실제 전송 문서와 해시가 같아야 한다.

승인에는 `approval_rule_version=2`, 승인 시간, 근거, `delivery.auto_approved` 감사 기록을 남긴다. 기존 직원 최종 승인에 따른 배급 승인은 `STAFF_FINAL`로 구별해 보존한다. 직원 보류는 문서가 바뀌어도 유지한다. 직원의 결정은 자동 승인 출처를 지운다. 동의·계약서 서명·실제 계약 및 라우팅·권리 epoch·전송 직전 승인 게이트는 계속 적용한다.

## 무료 AI 검사 범위와 SynthID

이 구현은 외부 API를 호출하지 않는다. API 키·유료 모델·크레딧·구독이 필요 없다. 새 커버 검사도 로컬 Tesseract(영어·한국어)와 ZBar를 사용한다. 기존 서버에서 ExifTool과 Rust 규칙을 실행하므로 서버 자원은 사용한다.

- `AI_METADATA_SIGNAL`: 생성 도구 또는 알고리즘 생성 메타데이터가 있다. 메타데이터는 조작할 수 있어 확정 판독이 아닌 수동 확인 근거다.
- `AI_DISCLOSURE_SIGNAL`: 가사에 명시적 AI 생성 문구가 있다. 실제 작성자나 모델을 확정하지 않는다.
- `UNKNOWN`: 지원하는 근거가 없다. 사람이 만들었다는 뜻이 아니다.
- `synthid=NOT_CHECKED`: 이 구현은 SynthID 검출을 수행하지 않는다. 일반 문체 점수나 문자열 탐색을 SynthID 결과로 표시하지 않는다.

도구명·IPTC 선언 외에도 [Stable Diffusion WebUI 생성 설정](https://github.com/AUTOMATIC1111/stable-diffusion-webui/blob/master/modules/processing.py)의 Steps·Sampler·CFG scale·Seed 조합과 [ComfyUI PNG 저장 코드](https://github.com/Comfy-Org/ComfyUI/blob/master/nodes.py)의 생성 작업 그래프를 확인한다. 샘플러와 모델·텍스트 인코더 노드가 함께 있는 구조를 검사하며, 일반 이미지 설명이나 ‘고품질’ 같은 문체는 AI 근거로 삼지 않는다. 이 메타데이터도 조작 가능해 수동 확인 신호다.

[Google SynthID](https://deepmind.google/models/synthid/)는 삽입된 워터마크를 검증한다. 이미지·음원 검증은 Gemini 및 검증 포털로 안내되지만, 이번 조사에서 비용 없는 공개 서버 API를 확인하지 못했다. [SynthID Text 공식 문서](https://ai.google.dev/responsible/docs/safeguards/synthid)는 생성 시 설정한 워터마크와 이에 맞는 검출기를 다룬다. 임의의 기존 가사 전체를 판독하는 범용 검출기는 아니다.

[OpenAI Content Provenance API](https://developers.openai.com/api/docs/guides/content-provenance)는 지원하는 OpenAI 이미지·오디오의 SynthID/C2PA 신호를 검사한다. 일반 가사 판독을 제공하지 않으며, 공식 가이드만으로 무료 이용을 보장할 수 없어 연동하지 않았다. 실제 계정 권한이나 요금을 확인하지 않은 상태에서 호출하지 않는다.

## 실행 및 검증

Docker 이미지와 Foundation CI에 `libimage-exiftool-perl`을 포함한다. 네이티브 환경도 ExifTool을 설치해야 한다. `EXIFTOOL_BIN`은 실행 파일 경로를 지정할 때만 사용한다. 분석에는 10초 실행 제한과 256 KiB 출력 제한이 있다. 분석기 실패는 TECHNICAL_RETRY로 기록하며 캐시되지 않는다. 원본 음원 정보를 읽지 못하면 정규화 완료를 재시도해 정보를 잃지 않도록 한다.

ExifTool이 읽지 못하는 음원 컨테이너는 기존 로컬 FFprobe로 태그를 읽는다. TTA 등 기존 무손실 업로드 형식을 유지하며, 두 도구 모두 실패한 경우만 출처 검사 실패로 남긴다. 손상·손실 압축 파일의 기존 보완 응답은 메타데이터 실패에 가려지지 않는다.

Stage 2 검사 기록은 검사 수와 무관하게 INSERT와 SELECT 두 번의 DB 왕복으로 처리한다. 재검사 결과 ID와 감사 이력을 보존한다. 전송 문서 생성 시간은 불변 패키지의 생성 시간으로 고정해 동일 입력의 재시도가 승인을 불필요하게 해제하지 않도록 한다. 작업의 lease와 현재 제출본을 확인하며, 만료된 작업은 결정을 커밋하지 않는다.

음원 출처 캐시는 파일 ID와 마스터 해시를 함께 사용한다. 서로 다른 원본이 동일한 FLAC으로 변환되어도 원본 AI 메타데이터가 다른 업로드의 UNKNOWN 결과에 가려지거나 다른 파일로 전파되지 않는다. 업로드 분석에서 원본 정보를 반영하고 제출 시 그 결과를 재사용한다.

회귀 검증은 실제 PostgreSQL 18과 FFmpeg/ExifTool을 사용하는 격리된 테스트 DB 및 GitHub Foundation CI에서 수행한다. 자동 승인·서명 게이트·직원 보류·경고 차단·재시도 문서 해시·가사 고정·수정 우선 처리·메타데이터 신호와 UNKNOWN의 구분을 확인한다. 운영 DB 마이그레이션과 서비스 배포는 별도다. 운영 p95와 범용 AI 판독 정확도를 측정한 것으로 주장하지 않는다.


## 공개 배급사·DSP 기준 추가 — 2026-10-01

[Apple Music 공식 스타일 가이드](https://help.apple.com/itc/musicstyleguide/en.lproj/static.html), [Spotify 메타데이터](https://support.spotify.com/gw-en/artists/article/metadata-formatting-guidelines/)·[커버 규격](https://support.spotify.com/gw-en/artists/article/cover-art-requirements/), [YouTube Content ID 적격성](https://support.google.com/youtube/answer/2605065?hl=en), [DistroKid 커버](https://support.distrokid.com/hc/en-us/articles/360013534334-What-Are-the-Requirements-for-Album-Artwork)·[가사 반려 안내](https://support.distrokid.com/hc/en-us/articles/360050506673-Correcting-Rejected-Lyrics), [TuneCore 커버](https://support.tunecore.com/hc/en-gb/articles/115006685728-What-are-TuneCore-s-cover-art-formatting-requirements)·[심사 절차](https://support.tunecore.com/hc/en-us/articles/115006692088-My-release-was-flagged-by-TuneCore-s-Content-Review-Team)를 조사했다. 공개 문서는 검토일에 확인한 근거이며 개별 계약의 비공개 납품 규격까지 확인한 것으로 취급하지 않는다.

| 검사 | 적용 범위 | 처리 |
| --- | --- | --- |
| 제목·아티스트의 URL·이메일 | Audeniq 공통 사전 검사 | 보완 요청 |
| 제목의 홍보 문구 | 공통 사전 검사 | 작품 제목일 수도 있어 확인 요청 |
| 일반 장르·SEO 문구로만 된 아티스트명 | Apple/iTunes·Spotify별 목록 | 확인 요청. 밴드명에 &가 있다는 이유로 분리하지 않는다. |
| 곡명의 괄호 안 feat. | Spotify | 별도 참여자 칸으로 보완. Apple에는 이 규칙을 적용하지 않는다. |
| 제목·아티스트의 이모지, 괄호 안 기술·홍보 정보, 500곡 초과 | Apple/iTunes | 보완 요청 |
| 가사의 섹션·반복 지시문, 주변 공백, 연속 빈 줄 | Apple/iTunes | 보완 요청. 가사를 임의로 다시 쓰지 않는다. |
| 가사 첫 글자·줄 끝 표기 | Apple/iTunes | 확인 요청. 예술적 표기와 언어 차이를 고려한다. |
| 강한 비속어와 explicit 미표시 | 공통 사전 검사 | 실제 음원 확인 요청. 19금 표시를 자동 변경하지 않는다. |
| Clean 버전과 explicit 표시의 충돌 | 공통 사전 검사 | 보완 요청 |
| 커버의 실제 QR 판독 | 공통 커버 정책 | 보완 요청. QR 내용은 저장·접속하지 않는다. |
| OCR의 연락처·홍보·가격·스토어·SNS·음질 정보 가능성 | 공통 커버 정책, 음질 문구는 Apple/iTunes | 원본 이미지 확인 요청. OCR만으로 확정 반려하지 않는다. |
| 커버의 명시적 CMYK·그레이·팔레트·알파·채널 비트·ICC·회전 정보 | Spotify | 해당 규격으로 재내보내기 요청. Apple에 Spotify 규칙을 적용하지 않는다. |
| 참조 전체의 독점 권리·원본 녹음 확인 | YouTube Content ID D-27 | 확인 항목 누락은 보완. 일반 YouTube Music D-7에는 요구하지 않는다. |
| 샘플·커버·리믹스·비독점·공공저작물·카라오케·리마스터 신호 | Content ID | 적격성과 참조 제외 구간을 확인. 신고 문구만으로 권리 진위를 확정하지 않는다. |

공통 검사는 Audeniq의 배급 전 정책이다. 특정 DSP의 공개 문구가 모든 DSP 계약에 동일하게 적용된다고 주장하지 않는다. DSP별 규칙은 제출본에 명시적으로 선택된 플랫폼에 대해 Stage 2에서 적용하고, DSP 스테이징에서도 같은 함수로 재확인한다. 이전 제출본에 플랫폼 선택이 없으면 Stage 2에서 새 DSP별 요구를 임의로 추정하지 않는다. 스테이징에서는 각각 확인하며, 명시적 플랫폼 선택과 새 정책 버전이 없는 패키지는 새 자동 승인 대상에서 제외한다.

커버 원본은 수정하지 않는다. 색상 속성·신뢰도 80 이상인 OCR 단어·QR 개수만 검사 결과에 기록한다. 각 로컬 실행에는 15초와 512 KiB 출력 제한, OCR 근거에는 16 KiB 제한을 적용하고 OCR 스레드는 1개로 제한한다. 도구 실패는 TECHNICAL_RETRY다. 결과가 없다는 이유로 사람이 제작했다거나 이미지가 모든 정책에 적합하다고 판단하지 않는다. Stage 1의 정확한 검사 참조만 읽어 새 OCR/QR 근거를 재사용하며, 해당 제출본에 참조되지 않은 과거 결과는 사용하지 않는다. 출처·적용 DSP·보완/검토 유형을 검사 상세에 기록하고 한국어 안내를 해당 신청서 항목에 연결한다.

새 제출본은 트랙 아티스트명도 동의 범위에 고정하고 배급 패키지에서 그 이름을 사용한다. 자동 승인 출처는 2이며 Stage 2 v3, QC v6, 콘텐츠 정책 v1 및 명시적 플랫폼 선택을 요구한다. 마이그레이션 0067은 기존 승인 출처를 유지하고 새 출처를 허용한다. 재스테이징은 기존 시스템 승인도 현재 규칙으로 다시 평가한다. 직원의 보류·직접 결정과 계약·서명·전송 게이트는 유지한다.

Docker와 Foundation CI에 Tesseract, 영어·한국어 언어 데이터 및 ZBar를 설치했다. 네이티브 환경에서도 이 도구가 필요하다. TESSERACT_BIN/ZBARIMG_BIN은 운영자가 실행 파일 경로를 지정하는 설정이다. qrencode와 폰트는 GitHub 회귀 테스트용이며 운영 이미지에 필요하지 않다. 실제 OCR/QR, 가사 형식, explicit 표시, Content ID 선언, 정확한 검사 참조와 DSP별 범위 및 정상 건 자동 승인을 GitHub에서 검증한다.

확인할 수 없는 범위는 계속 직원 검토가 필요하다: 외부 음원·사진·상표의 실제 권리, AI 목소리 사칭의 당사자 허가, 모든 언어의 가사와 음원 일치, 누드·폭력·로고의 시각적 판단, 아직 발생하지 않은 스트리밍 조작, 비공개 계약별 편집 기준. 이 항목을 검사 완료나 자동 PASS로 표시하지 않는다. 추가 유료 API 호출은 없다.
