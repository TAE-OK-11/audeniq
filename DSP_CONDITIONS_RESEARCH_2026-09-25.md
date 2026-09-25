# DSP 공통 기술/정책 조건 리서치 (2026-09-25 기준)

## 조사 범위·방법·한계
- 해외 6사(Spotify, Apple Music, YouTube Music, Amazon Music, Deezer, TIDAL): 공식 문서(Spotify for Artists 헬프, Spotify Metadata Style Guide, FUGA DSP 정책 가이드, Deezer Creator Support 등) + 신뢰 가능한 2차 출처
- 국내 5사(Melon, Genie, FLO, Bugs, Vibe): **유통사 대상 공식 기술 가이드가 공개 웹에 없음** (파트너 포털 비공개). KPIA 공식 심의 안내, 청소년보호법령, RouteNote Korea 공식 블로그, DistroKid 한국어 고객센터 등 2차 출처로 보완
- 각 조건은 **공식 확인 / 2차 출처**를 구분 표기. 미확인은 제외
- 참고: **Vibe는 2026-12-31 서비스 종료 공식 발표** — 신규 대응 불필요

## 1. 오디오 스펙 (공통 조건)
- **무손실 포맷, 44.1kHz/16bit 이상**: Deezer 공식(FLAC 선호, 16bit/44.1kHz/2ch), Apple 24bit/44.1kHz+ 권장(2차), Spotify·YouTube·Amazon·TIDAL 모두 WAV/FLAC 고품질 마스터 요구(공식/2차), 국내 5사 공통 관행(WAV/FLAC 44.1kHz/16bit). 저비트레이트 MP3·오디오 워터마크·클리핑은 반려 사유
- **라우드니스 −14 LUFS 내외**: Spotify 공식 −14 dB LUFS(ITU 1770) + True Peak −1 dBTP 이하(마스터가 −14 초과 시 −2 dB). Apple −16(2차), Deezer −15(2차), YouTube/Amazon/TIDAL −14(2차, TIDAL은 앨범 단위). 실무 공통 기준: **−14 LUFS ±1, TP −1 dBTP 이하**
- **최소 길이**: 공식 문서상 명시된 DSP 없음. 단, 무음·노이즈만 있는 트랙은 Spotify 공식 반려

## 2. 커버 아트워크 (공통 조건)
- **정사각형 1:1**: Spotify 공식(640~10000px), Apple 3000×3000px 이상(정책), Deezer 공식(PNG/JPEG, 500~4096px, 32MB 이하), 국내 공통 관행 3000×3000px. 실무 공통 기준: **3000×3000px 이상 정사각형**
- **금지 요소** (Spotify 공식 + Apple/Spotify FUGA 정책, 국내 RouteNote 공통): URL·연락처, 플랫폼/경쟁사 로고, 홍보 문구("Out Now", "Exclusive"), 가격 정보, QR코드, 흐릿함·픽셀화·업스케일, 아트워크-메타데이터 철자 불일치(Apple은 반려 사유로 명시)
- **저작권**: 이미지·폰트 상업적 이용 권한 증빙 필수, 무단 전재·AI 이미지 가이드 위반 시 유통 불가(국내 RouteNote 공식)

## 3. 메타데이터 필수 항목 (공통 조건)
- **ISRC(트랙) + UPC/EAN(앨범)**: Spotify/Apple/Deezer 공식 필수, YouTube 공식(ISRC + UPC/EAN/GRid), 국내는 유통사가 생성(2차). 형식이 틀리면 반려(Deezer는 UPC/ISRC 불일치를 반려 사유로 명시)
- **장르, 발매일, 지역 가용성, 언어 코드**: Spotify/Apple/Deezer 공식 필수. Apple은 언어 코드를 메타데이터 언어 기준으로 요구(오디오 언어와 별개)
- **Explicit 표시**: Spotify 공식(제목에 직접 "E" 표기 금지, 플래그로만), Apple 공식(오표기 시 반려), Deezer 공식(앨범 커버·가사 모두). 국내는 **청소년유해매체물 표시 법적 의무**(아래 6번)
- **작사/작곡 크레딧**: Deezer 공식 필수(트랙당 Composer/Lyricist 등 1개 이상), Apple은 실명(풀네임) 강제(이니셜·별칭 불가), 국내 유통 관행상 작사/작곡/편곡 크레딧 요구
- **P-line/℗, C-line/©**: DDEX ERN 실무상 필수(Deezer DDEX 3.x/4.x 기반), 국내 유통 관행

## 4. 제목/아티스트명 정책 (공통 조건)
- **키워드 스태핑(메타데이터 게이밍) 금지**: Spotify 공식(SEO 용어·타이틀 키워드 금지), Apple 공식(제네릭 타이틀·광고 문구·이모지 금지), 국내 공통(RouteNote: 장르명·트렌드 키워드·유명 아티스트명 도용 금지)
- **feat. 분리 표기**: Spotify 공식(별도 아티스트 필드 + 역할 구분), Deezer 공식(메인/피처링 분리). 제목에 "feat."를 때려박는 방식 지양
- **버전 표기 표준화**: Spotify 공식("Year Remaster" 형식, "Digital Remaster" 금지, 불필요한 "Original Version" 금지, 버전 필드 사용), Apple 공식(불필요 버전 정보·"Exclusive"·경쟁사 언급 금지)
- **아티스트 필드 오용 금지**: 레이블·유통사명을 아티스트 필드에 기재 금지(Spotify/Apple 공식), 실제 표기와 다른 그룹명+개인명 병합 금지(Apple)

## 5. 공통 반려 사유
| 반려 사유 | 해당 DSP |
|---|---|
| 동일 오디오 중복 발매(오디오 지문 충돌) | Spotify, Deezer, 국내 5사 공통 |
| 저품질 오디오(클리핑·글리치·침묵 패딩·마스터링 미완성) | Spotify, Apple |
| 메타데이터 오류·불일치(ISRC/UPC 불일치, 아트워크-메타데이터 철자 차이) | Apple, Deezer |
| AI 생성물 무단 사용·아티스트 사칭 | Spotify, Apple, TIDAL(사칭 자동 차단·완전 AI 생성 트랙 로열티 제외) |
| 커버곡 권리 미비 | TIDAL Upload 금지, 국내는 원작자 커버 동의서 추가 요구 |
| 비공식 녹음(부트렉·라디오립, 공식 디스코그래피 유통사가 아닌 경우) | Spotify, Apple |
| 청소년유해/Explicit 미표시 | Spotify·Apple·Deezer(플래그), 국내(법적 표시 의무) |
| 발매일 2주 전 미제출(일정 차질) | Deezer |

## 6. 국내 특이사항: 청소년유해매체물 심의 (법적 의무)
- 근거: KPIA 공식 안내(PDF), 청소년보호법 시행령 [별표 4]
- 유통사(납품 주체)가 **자율적으로 유해 여부 판단 가능**(청소년보호법 제11조) → 19금 판정을 유통사가 내릴 수 있음
- 표시 의무 위반 시 시정명령·과태료, 2년 이하 징역 또는 2천만원 이하 벌금 / 청소년에게 판매·제공 시 3년 이하 징역 또는 3천만원 이하 벌금
- 표시 방법: 음반·음악영상물 **"19세 미만 청취 불가"**(적색 바탕 흰색), 음악파일·음악영상파일 **"19세 미만 이용 불가"**
- 심의 기준(개별): 선정적 묘사, 성행위 묘사, 폭력·자살 미화, 범죄 미화, **욕설·비속어 남용, 술·담배 권유/조장, 마약 복용 욕구·효과·제조방법 기술** 등
- 사전 음반심의는 1996년 폐지, 현재는 사후 심의 + 자율규제 체계

## 7. 공통 조건 체크리스트

| # | 공통 조건 | 요구 DSP | 출처 |
|---|---|---|---|
| A1 | 무손실 WAV/FLAC, 44.1kHz/16bit 이상 (MP3·워터마크·클리핑 불가) | Spotify, Apple, Deezer, TIDAL, Amazon, YouTube, 국내 5사 | Spotify(정규화 문서), Deezer Creator 공식, Apple(2차), 국내 RouteNote/DistroKid(2차) |
| A2 | 라우드니스 −14 LUFS 내외, True Peak −1 dBTP 이하 | Spotify(공식 −14), Apple(−16·2차), YouTube/Amazon/TIDAL(−14·2차), Deezer(−15·2차) | https://support.spotify.com/us/artists/article/loudness-normalization/ · https://www.izotope.com/community/blog/mastering-for-streaming-platforms |
| C1 | 아트워크 정사각형 1:1, 3000×3000px 이상 | Spotify(공식 640~10000), Apple(3000+·정책), Deezer(공식 500~4096), 국내 관행 | https://support.spotify.com/us/artists/article/cover-art-requirements/ · https://creatorsupport.deezer.com/hc/en-us/articles/5928712530077-How-To-Add-Music-To-Deezer-Through-A-Label |
| C2 | 아트워크 금지: URL·로고·홍보문구·QR·흐릿함·메타데이터 불일치 | Spotify, Apple, 국내 공통 | 위 Spotify URL · https://support.fuga.com/hc/en-us/articles/26837448629268-Apple-Music-Content-Policy-Guidelines |
| M1 | ISRC(트랙) + UPC/EAN(앨범) 필수, 형식 정확 | Spotify, Apple, Deezer, YouTube, 국내 5사 | https://support.spotify.com/us/artists/article/metadata-formatting-guidelines/ · Deezer Creator 공식 · https://support.google.com/youtube/answer/6007071?hl=en-GB |
| M2 | Explicit/19금 표시 필수 (제목 직접 표기 금지) | Spotify, Apple, Deezer + 국내 법적 의무 | Spotify 메타데이터 가이드 · https://kpia.or.kr 안내 PDF · https://www.law.go.kr 시행령 별표4 |
| M3 | 장르·발매일·지역 가용성·언어 코드 | Spotify, Apple, Deezer | Spotify 메타데이터 가이드 · Deezer Creator 공식 |
| M4 | 작사/작곡 크레딧 (실명, Deezer는 트랙당 1개 이상 필수) | Deezer(공식), Apple(실명 강제), 국내 관행 | Deezer Creator 공식 · FUGA Apple 정책 가이드 |
| M5 | P-line/℗, C-line/© | DDEX 기반 전 DSP 실무, 국내 유통 관행 | Deezer DDEX 3.x/4.x 공식 문서 |
| T1 | 키워드 스태핑/메타데이터 게이밍 금지 | Spotify, Apple, 국내 공통 | Spotify Metadata Style Guide v2.2.0(PDF) · https://routenote.com/kr/blog/아티스트가-절대-피해야-할-부정-스트리밍-메타데/ |
| T2 | feat. 분리 표기 (별도 아티스트 필드·역할) | Spotify, Deezer | Spotify 메타데이터 가이드 · Deezer Creator 공식 |
| T3 | 버전 표기 표준화 ("Year Remaster" 등, 불필요 표기 금지) | Spotify, Apple | Spotify Style Guide v2.2.0 · FUGA Apple 정책 가이드 |
| R1 | 동일 오디오 중복 발매 금지 | Spotify, Deezer, 국내 5사 | https://support.fuga.com/hc/en-us/articles/30023468817300-Spotify-Content-Policy-Guidelines · https://routenote.com/kr/blog/중복-배포-컴필레이션-발매-유튜브-수익-창출dsp의-모/ |
| R2 | AI 생성물 정책 (무단 사용·사칭 금지, 저작권 표기) | Spotify, Apple, TIDAL | FUGA Spotify 정책 가이드 · https://www.itechpost.com/articles/236510/20260630/tidal-label-ai-generated-music-remove-fraudulent-music-streaming-platform.htm |
| R3 | 커버곡 권리 증빙 (국내: 원작자 커버 동의서 추가 요구) | TIDAL, 국내 5사 | https://tidal.com/terms?eu=true · https://routenote.com/kr/blog/합법적인-커버곡-유통법-기계적-라이선스의-모든/ |
| R4 | 국내: 청소년유해매체물 표시·심의 기준 준수 | Melon, Genie, FLO, Bugs (+Vibe, 종료 예정) | KPIA 안내 PDF · 청소년보호법 시행령 [별표 4] |

### 출처 URL 전체 목록
**해외 (공식/준공식)**
- https://support.spotify.com/us/artists/article/loudness-normalization/
- https://support.spotify.com/us/artists/article/cover-art-requirements/
- https://support.spotify.com/us/artists/article/metadata-formatting-guidelines/
- https://assets.ctfassets.net/jtdj514wr91r/2MCgL0vUEcl8MJijHPcLG1/17e8114c81872a85a34dca6b9f4e5f41/Spotify_Music_Metadata_Style_Guide_V2.2.0.pdf
- https://support.fuga.com/hc/en-us/articles/30023468817300-Spotify-Content-Policy-Guidelines
- https://support.fuga.com/hc/en-us/articles/26837448629268-Apple-Music-Content-Policy-Guidelines
- https://creatorsupport.deezer.com/hc/en-us/articles/5928712530077-How-To-Add-Music-To-Deezer-Through-A-Label
- https://creatorsupport.deezer.com/hc/en-us/articles/5927556644125-How-To-Add-Your-Own-Independent-Music-To-Deezer
- https://support.google.com/youtube/answer/6007071?hl=en-GB
- https://support.google.com/youtube/answer/13486873
- https://www.izotope.com/community/blog/mastering-for-streaming-platforms (정규화 수치 2차 정리)
- https://tidal.com/terms?eu=true
- https://www.itechpost.com/articles/236510/20260630/tidal-label-ai-generated-music-remove-fraudulent-music-streaming-platform.htm

**국내**
- https://kpia.or.kr 안내 PDF (청소년유해매체물 심의 기준·의무사항)
- https://www.law.go.kr 청소년보호법 시행령 [별표 4] (유해표시 방법)
- https://routenote.com/kr/blog/저작권-등록법부터-음반-만료-기한-아트워크-규정까/ (아트워크 저작권)
- https://routenote.com/kr/blog/합법적인-커버곡-유통법-기계적-라이선스의-모든/ (커버곡 동의서)
- https://routenote.com/kr/blog/아티스트가-절대-피해야-할-부정-스트리밍-메타데/ (메타데이터 게이밍)
- https://routenote.com/kr/blog/중복-배포-컴필레이션-발매-유튜브-수익-창출dsp의-모/ (중복 배포)
- https://support.distrokid.com/hc/ko/articles/360013648913-DistroKid%EC%97%90-%EC%97%85%EB%A1%9C%EB%93%9C%ED%95%98%EA%B8%B0-%EC%9C%84%ED%95%B4-UPC-%EB%98%90%EB%8A%94-ISRC%EB%A5%BC-%EC%A0%9C%EA%B3%B5%ED%95%98%EA%B7%B8%EC%95%84%EB%82%98%EC%9A%94 (ISRC/UPC)
- http://www.melon.com/musicstory/inform.htm?mstorySeq=5912 / https://m2.melon.com/musicstory/detail.htm?mstorySeq=16539 (멜론 19금 표기 실례)
- https://www.genie.co.kr/promotion/2016/1021/index?type (지니 FLAC 24bit)
- https://file.bugsm.co.kr/bugscorp/download/bugs_businessReport_23.pdf (벅스 FLAC 무손실)
- https://v.daum.net/v/20260827190840388 (VIBE 2026-12-31 종료)

### 남은 갭 / 후속 제안
1. **국내 5사 공식 납품 스펙(샘플레이트·비트뎁스·아트워크 해상도·메타데이터 필수 필드·제목 표기 규칙)은 전부 미확인** — 파트너 포털이 비공개라 실제 유통 계약 후 입수하거나 DSP 사업제휴 창구에 직접 문의 필요
2. Amazon Music·TIDAL의 유통사 대상 공식 기술 문서도 비공개 — 2차 출처 기반이며 파트너 계약 시 확인 필요
3. Spotify/Apple/Deezer/YouTube의 공식 조건은 파트너 포털 최신 문서와 대조 권장 (특히 Spotify Style Guide 버전 업데이트 여부)

---

## 구현 반영 현황 (F2 코드베이스 기준, 2026-09-25)

| # | 조건 | 구현 상태 |
|---|---|---|
| A1 | 44.1kHz 이상 / 16-bit 이상 | ✅ 구현됨 (SAMPLE_RATE_LOW, AUDIO_BIT_DEPTH_LOW) |
| A2 | −14 LUFS ±1, TP −1 dBTP 이하 | ❌ 미구현 — ffmpeg ebur128/loudnorm으로 측정 가능 |
| C1 | 아트워크 1:1, 3000×3000px 이상 | ❌ 미구현 — 이미지 메타데이터로 검사 가능 |
| C2 | 아트워크 금지 요소 | ⚠️ 부분 — 메타데이터 불일치만 가능, URL/로고/QR은 자동 판정 불가 |
| M1 | ISRC/UPC 형식 | ✅ 구현됨 |
| M2 | Explicit/19금 표시 | ⚠️ 부분 — parental advisory 필드 존재 확인 필요 |
| M3 | 장르·발매일·지역·언어 | ✅ 대부분 (언어 코드 확인 필요) |
| M4 | 작사/작곡 크레딧 실명 | ❌ 미구현 — DDEX 스키마 확인 필요 |
| M5 | P-line/C-line | ❌ 미구현 — DDEX 릴리스 모델에 추가 필요 |
| T1 | 키워드 스태핑 | ✅ 구현됨 (TRACK_TITLE_SEO_SPAM, REVIEW) |
| T2 | feat. 분리 표기 | ✅ 구현됨 (TRACK_TITLE_HAS_VERSION_INFO 감지, REVIEW) |
| T3 | 버전 표기 표준화 | ⚠️ 부분 — 별도 version 필드 부재 |
| R1 | 동일 오디오 중복 | ⚠️ SHA256 기반만 — fingerprint 엔진 없음 |
| R2 | AI 생성물 정책 | ❌ 자동 판정 불가 (선언 기반) |
| R3 | 커버곡 권리 증빙 | ❌ 자동 판정 불가 (선언 기반) |
| R4 | 국내 19금 표시 의무 | ❌ 미구현 — 법적 의무라 국내 DSP 대응 시 필수 |
