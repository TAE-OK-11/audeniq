# 은행·플랫폼 로고

은행·DSP 칸에 실제 로고를 보여 준다. 파일은 `web/studio/app/public/static/brands/`, 목록은 `web/studio/app/src/lib/brands.ts`, 그리는 곳은 `components/BrandMark.tsx`(DSP)·`components/BankLogo.tsx`(은행)이다.
로고는 각 회사의 상표이며, 해당 서비스·금융기관을 알아보게 하는 용도로만 쓴다. 모양·색을 바꾸지 않는다.

## 넣는 방법 (새 로고·교체)

1. 공식 파일을 `public/static/brands/<키>.png`(정사각형, 128px 이상) 또는 `.svg`로 둔다.
2. `src/lib/brands.ts`에 한 줄 추가: 앱 아이콘처럼 칸을 꽉 채우는 그림은 `kind: 'app'`, 여백이 필요한 로고는 `'image'`, 단색 SVG는 `'glyph'`(+ `color`).
3. 키는 은행은 `BankLogo.tsx`의 세 번째 값(예: `shinhan`), DSP는 `Upload.tsx`의 `DSP_COLOR` 키(예: `melon`).

## 지금 들어간 로고와 출처

| 키 | 이름 | 출처 |
|---|---|---|
| `spotify` | Spotify | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://developer.spotify.com/documentation/general/design-and-branding/#using-our-logo |
| `apple` | Apple Music | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.apple.com/itunes/marketing-on-music/identity-guidelines.html#apple-music-icon |
| `youtube` | YouTube Music | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://partnermarketinghub.withgoogle.com/#/brands |
| `youtube-cid` | YouTube(Content ID) | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.youtube.com/howyoutubeworks/resources/brand-resources/#logos-icons-and-colors |
| `tidal` | TIDAL | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://tidal.com/press |
| `deezer` | Deezer | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://deezerbrand.com/document/37#/-/logo |
| `pandora` | Pandora | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.pandoraforbrands.com |
| `soundcloud` | SoundCloud | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://press.soundcloud.com |
| `audiomack` | Audiomack | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://styleguide.audiomack.com |
| `netease` | NetEase Cloud Music | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://y.music.163.com/m |
| `napster` | Napster | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.napster.com/us/wp-content/uploads/sites/5/2022/03/logo-napster.svg |
| `iheart` | iHeartRadio | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://brand.iheart.com/logo |
| `meta` | Meta(Instagram·Facebook) | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.facebook.com/brand/resources/meta/company-brand |
| `tiktok` | TikTok | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://tiktok.com |
| `snapchat` | Snapchat | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.snapchat.com/brand-guidelines |
| `beatport` | Beatport | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://support.beatport.com/hc/en-us/articles/4412316336404-Beatport-Logos-and-Images |
| `itunes` | iTunes | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://commons.wikimedia.org/wiki/File:ITunes_logo.svg |
| `triller` | Triller | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://triller.co |
| `kuaishou` | Kuaishou | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.kuaishou.com/official/material-lib |
| `mixcloud` | Mixcloud | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://www.mixcloud.com/about |
| `twitch` | Twitch | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://brand.twitch.tv |
| `peloton` | Peloton | 공식 브랜드 가이드 기반 SVG(simple-icons) — https://press.onepeloton.com/#logos |
| `melon` | 멜론 | App Store 공식 앱 아이콘 — 멜론(Melon) (Kakao M Corp.) — https://apps.apple.com/kr/app/%EB%A9%9C%EB%A1%A0-melon/id415597317 |
| `genie` | 지니 | App Store 공식 앱 아이콘 — 지니뮤직 - genie (GENIE MUSIC CORP.) — https://apps.apple.com/kr/app/%EC%A7%80%EB%8B%88%EB%AE%A4%EC%A7%81-genie/id858266085 |
| `flo` | FLO | App Store 공식 앱 아이콘 — FLO - 플로 (Dreamus Company) — https://apps.apple.com/kr/app/flo-%ED%94%8C%EB%A1%9C/id1129048043 |
| `bugs` | 벅스 | App Store 공식 앱 아이콘 — 벅스 - Bugs (NHN BUGS Corporation) — https://apps.apple.com/kr/app/%EB%B2%85%EC%8A%A4-bugs/id348555322 |
| `vibe` | VIBE | App Store 공식 앱 아이콘 — NAVER VIBE (바이브) (NAVER Corp.) — https://apps.apple.com/kr/app/naver-vibe-%EB%B0%94%EC%9D%B4%EB%B8%8C/id1338631589 |
| `amazon` | Amazon Music | App Store 공식 앱 아이콘 — Amazon Music: Songs & Podcasts (AMZN Mobile LLC) — https://apps.apple.com/us/app/amazon-music-songs-podcasts/id510855668 |
| `anghami` | Anghami | App Store 공식 앱 아이콘 — Anghami: Play Music & Podcasts (Anghami) — https://apps.apple.com/us/app/anghami-play-music-podcasts/id545395155 |
| `awa` | AWA | App Store 공식 앱 아이콘 — AWA : 2億曲の音楽や歌が聴き放題の人気音楽アプリ (AWA Co. Ltd.) — https://apps.apple.com/jp/app/awa-2%E5%84%84%E6%9B%B2%E3%81%AE%E9%9F%B3%E6%A5%BD%E3%82%84%E6%AD%8C%E3%81%8C%E8%81%B4%E3%81%8D%E6%94%BE%E9%A1%8C%E3%81%AE%E4%BA%BA%E6%B0%97%E9%9F%B3%E6%A5%BD%E3%82%A2%E3%83%97%E3%83%AA/id980578855 |
| `boomplay` | Boomplay | App Store 공식 앱 아이콘 — Boomplay: Music & Live Stream (TRANSSNET MUSIC LIMITED) — https://apps.apple.com/us/app/boomplay-music-live-stream/id1437251983 |
| `claro-musica` | Claro Música | App Store 공식 앱 아이콘 — Claro Música (iMusica LLC) — https://apps.apple.com/us/app/claro-m%C3%BAsica/id843577241 |
| `jiosaavn` | JioSaavn | App Store 공식 앱 아이콘 — JioSaavn – Music & Podcasts (Saavn Media Limited) — https://apps.apple.com/in/app/jiosaavn-music-podcasts/id441813332 |
| `joox` | JOOX | App Store 공식 앱 아이콘 — JOOX音樂 - AI聽歌識曲播放器 (Tencent Mobility Limited) — https://apps.apple.com/hk/app/joox%E9%9F%B3%E6%A8%82-ai%E8%81%BD%E6%AD%8C%E8%AD%98%E6%9B%B2%E6%92%AD%E6%94%BE%E5%99%A8/id914406371 |
| `kkbox` | KKBOX | App Store 공식 앱 아이콘 — KKBOX｜音樂＆Podcast (KKCOMPANY TECHNOLOGIES PTE. LTD.) — https://apps.apple.com/tw/app/kkbox-%E9%9F%B3%E6%A8%82-podcast/id300915900 |
| `qobuz` | Qobuz | App Store 공식 앱 아이콘 — Qobuz : Musique & Editorial (qobuz) — https://apps.apple.com/fr/app/qobuz-musique-editorial/id946429340 |
| `tencent` | QQ Music(Tencent) | App Store 공식 앱 아이콘 — QQ音乐 - 听我想听 (Tencent Technology (Shenzhen) Company Limited) — https://apps.apple.com/cn/app/qq%E9%9F%B3%E4%B9%90-%E5%90%AC%E6%88%91%E6%83%B3%E5%90%AC/id414603431 |
| `trebel` | TREBEL | App Store 공식 앱 아이콘 — TREBEL Music - Download Songs (M&M Media, Inc.) — https://apps.apple.com/us/app/trebel-music-download-songs/id912390687 |
| `yandex` | Yandex Music | App Store 공식 앱 아이콘 — Яндекс Музыка, книги, подкасты (DIRECT CURSUS COMPUTER SYSTEMS TRADING LLC) — https://apps.apple.com/ru/app/%D1%8F%D0%BD%D0%B4%D0%B5%D0%BA%D1%81-%D0%BC%D1%83%D0%B7%D1%8B%D0%BA%D0%B0-%D0%BA%D0%BD%D0%B8%D0%B3%D0%B8-%D0%BF%D0%BE%D0%B4%D0%BA%D0%B0%D1%81%D1%82%D1%8B/id520797969 |
| `canva` | Canva | App Store 공식 앱 아이콘 — Canva(캔바): 디자인, 사진편집, 동영상 편집 (Canva Pty Ltd) — https://apps.apple.com/kr/app/canva-%EC%BA%94%EB%B0%94-%EB%94%94%EC%9E%90%EC%9D%B8-%EC%82%AC%EC%A7%84%ED%8E%B8%EC%A7%91-%EB%8F%99%EC%98%81%EC%83%81-%ED%8E%B8%EC%A7%91/id897446215 |
| `acuon` | 애큐온저축은행 | App Store 공식 앱 아이콘 — 애큐온저축은행 (acuon Savings Bank) — https://apps.apple.com/kr/app/%EC%95%A0%ED%81%90%EC%98%A8%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89/id1304908041 |
| `bnk` | BNK(부산·경남은행) | App Store 공식 앱 아이콘 — BNK부산은행모바일뱅킹 (Busan Bank) — https://apps.apple.com/kr/app/bnk%EB%B6%80%EC%82%B0%EC%9D%80%ED%96%89%EB%AA%A8%EB%B0%94%EC%9D%BC%EB%B1%85%ED%82%B9/id1445137607 |
| `bookook` | 부국증권 | App Store 공식 앱 아이콘 — BEST-M Plus (BOOKOOK SECURITIES CO, LTD) — https://apps.apple.com/kr/app/best-m-plus/id1166765542 |
| `cape` | 케이프투자증권 | App Store 공식 앱 아이콘 — 케이프투자증권 Orda M Plus (CAPE Investment & Securities Co., Ltd.) — https://apps.apple.com/kr/app/%EC%BC%80%EC%9D%B4%ED%94%84%ED%88%AC%EC%9E%90%EC%A6%9D%EA%B6%8C-orda-m-plus/id1170365757 |
| `citi` | 한국씨티은행 | App Store 공식 앱 아이콘 — 씨티모바일 (CITIBANK KOREA INC.) — https://apps.apple.com/kr/app/%EC%94%A8%ED%8B%B0%EB%AA%A8%EB%B0%94%EC%9D%BC/id1179759666 |
| `db` | DB(저축은행·증권) | App Store 공식 앱 아이콘 — DB증권 MTS(알파증권) (DB Securities Co., Ltd.) — https://apps.apple.com/us/app/db%EC%A6%9D%EA%B6%8C-mts-%EC%95%8C%ED%8C%8C%EC%A6%9D%EA%B6%8C/id1603371564 |
| `dongwon` | 동원제일저축은행 | App Store 공식 앱 아이콘 — DW BANK (동원제일저축은행) (DONGWON JEIL SAVINGS BANK CO.,LTD.) — https://apps.apple.com/kr/app/dw-bank-%EB%8F%99%EC%9B%90%EC%A0%9C%EC%9D%BC%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89/id1593502750 |
| `forest` | 산림조합 | App Store 공식 앱 아이콘 — 산림조합 스마트뱅킹 (NATIONAL FORESTRY COOPERATIVE FEDERATION) — https://apps.apple.com/kr/app/%EC%82%B0%EB%A6%BC%EC%A1%B0%ED%95%A9-%EC%8A%A4%EB%A7%88%ED%8A%B8%EB%B1%85%ED%82%B9/id1251840731 |
| `hanwha` | 한화투자증권 | App Store 공식 앱 아이콘 — 한화투자증권 MTS (대표MTS) (HANWHA INVESTMENT & SECURITIES Co.,Ltd.) — https://apps.apple.com/kr/app/%ED%95%9C%ED%99%94%ED%88%AC%EC%9E%90%EC%A6%9D%EA%B6%8C-mts-%EB%8C%80%ED%91%9Cmts/id1200606784 |
| `hyundai` | 현대차증권 | App Store 공식 앱 아이콘 — 현대차증권 내일 (HYUNDAI MOTOR SECURITIES CO., LTD.) — https://apps.apple.com/kr/app/%ED%98%84%EB%8C%80%EC%B0%A8%EC%A6%9D%EA%B6%8C-%EB%82%B4%EC%9D%BC/id6444633082 |
| `im` | iM뱅크 | App Store 공식 앱 아이콘 — iM뱅크 - 아이엠뱅크 모바일뱅킹 (iM Bank Co., Ltd.) — https://apps.apple.com/kr/app/im%EB%B1%85%ED%81%AC-%EC%95%84%EC%9D%B4%EC%97%A0%EB%B1%85%ED%81%AC-%EB%AA%A8%EB%B0%94%EC%9D%BC%EB%B1%85%ED%82%B9/id1067748687 |
| `kakao` | 카카오뱅크 | App Store 공식 앱 아이콘 — 카카오뱅크 (KakaoBank Corp.) — https://apps.apple.com/kr/app/%EC%B9%B4%EC%B9%B4%EC%98%A4%EB%B1%85%ED%81%AC/id1258016944 |
| `kb` | KB(국민은행 등) | App Store 공식 앱 아이콘 — KB스타뱅킹 (Kookmin Bank Co., Ltd.) — https://apps.apple.com/kr/app/kb%EC%8A%A4%ED%83%80%EB%B1%85%ED%82%B9/id373742138 |
| `kbank` | 케이뱅크 | App Store 공식 앱 아이콘 — 케이뱅크 (kbank) — https://apps.apple.com/kr/app/%EC%BC%80%EC%9D%B4%EB%B1%85%ED%81%AC/id1178872627 |
| `kdb` | KDB산업은행 | App Store 공식 앱 아이콘 — 한국산업은행 「스마트KDB」 (The Korea Development Bank) — https://apps.apple.com/kr/app/%ED%95%9C%EA%B5%AD%EC%82%B0%EC%97%85%EC%9D%80%ED%96%89-%EC%8A%A4%EB%A7%88%ED%8A%B8kdb/id392572957 |
| `kyobo` | 교보증권 | App Store 공식 앱 아이콘 — 교보증권 Win.K (Kyobo Securities. Co. Ltd) — https://apps.apple.com/kr/app/%EA%B5%90%EB%B3%B4%EC%A6%9D%EA%B6%8C-win-k/id1282214166 |
| `meritz` | 메리츠증권 | App Store 공식 앱 아이콘 — 메리츠증권 - 메리츠 SMART (Meritz Securities Co. Ltd.) — https://apps.apple.com/kr/app/%EB%A9%94%EB%A6%AC%EC%B8%A0%EC%A6%9D%EA%B6%8C-%EB%A9%94%EB%A6%AC%EC%B8%A0-smart/id1104272974 |
| `mg` | 새마을금고 | App Store 공식 앱 아이콘 — MG더뱅킹 (KOREAN FEDERATION OF COMMUNITY CORPORATIVE) — https://apps.apple.com/kr/app/mg%EB%8D%94%EB%B1%85%ED%82%B9/id425089902 |
| `moa` | 모아저축은행 | App Store 공식 앱 아이콘 — 모아저축은행 디지털뱅크 (Moa Savings Bank Co., Ltd.) — https://apps.apple.com/kr/app/%EB%AA%A8%EC%95%84%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89-%EB%94%94%EC%A7%80%ED%84%B8%EB%B1%85%ED%81%AC/id1580681304 |
| `ok` | OK저축은행 | App Store 공식 앱 아이콘 — OK 저축은행 (OK SAVINGS BANK co., ltd.) — https://apps.apple.com/kr/app/ok-%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89/id973344869 |
| `osb` | OSB저축은행 | App Store 공식 앱 아이콘 — OSB저축은행 스마트뱅킹 (OSB SAVINGS BANK Co., Ltd.) — https://apps.apple.com/kr/app/osb%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89-%EC%8A%A4%EB%A7%88%ED%8A%B8%EB%B1%85%ED%82%B9/id845039320 |
| `pepper` | 페퍼저축은행 | App Store 공식 앱 아이콘 — 디지털페퍼 (페퍼저축은행) (Pepper Savings Bank Co.,LTD) — https://apps.apple.com/kr/app/%EB%94%94%EC%A7%80%ED%84%B8%ED%8E%98%ED%8D%BC-%ED%8E%98%ED%8D%BC%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89/id1597250071 |
| `pureun` | 푸른저축은행 | App Store 공식 앱 아이콘 — 푸른저축은행 스마트뱅킹 (pureun mutual savings bank) — https://apps.apple.com/kr/app/%ED%91%B8%EB%A5%B8%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89-%EC%8A%A4%EB%A7%88%ED%8A%B8%EB%B1%85%ED%82%B9/id1454339390 |
| `samsung` | 삼성증권 | App Store 공식 앱 아이콘 — 삼성증권 mPOP (SAMSUNG SECURITIES Co., LTD.) — https://apps.apple.com/kr/app/%EC%82%BC%EC%84%B1%EC%A6%9D%EA%B6%8C-mpop/id1150231646 |
| `sc` | SC제일은행 | App Store 공식 앱 아이콘 — SC제일은행 모바일뱅킹 (Standard Chartered Bank) — https://apps.apple.com/kr/app/sc%EC%A0%9C%EC%9D%BC%EC%9D%80%ED%96%89-%EB%AA%A8%EB%B0%94%EC%9D%BC%EB%B1%85%ED%82%B9/id1457418899 |
| `smart` | 스마트저축은행 | App Store 공식 앱 아이콘 — 스마트저축은행 smart-i (Smart Savings Bank) — https://apps.apple.com/kr/app/%EC%8A%A4%EB%A7%88%ED%8A%B8%EC%A0%80%EC%B6%95%EC%9D%80%ED%96%89-smart-i/id1552329994 |
| `suhyup` | 수협은행 | App Store 공식 앱 아이콘 — 수협 파트너뱅크 개인 (Suhyup Bank) — https://apps.apple.com/us/app/%EC%88%98%ED%98%91-%ED%8C%8C%ED%8A%B8%EB%84%88%EB%B1%85%ED%81%AC-%EA%B0%9C%EC%9D%B8/id1229985989 |
| `toss` | 토스(뱅크·증권) | App Store 공식 앱 아이콘 — 토스 (Viva Republica) — https://apps.apple.com/kr/app/%ED%86%A0%EC%8A%A4/id839333328 |
| `yegaram` | 예가람저축은행 | App Store 공식 앱 아이콘 — 예스뱅킹 (Yegaram Savings Bank) — https://apps.apple.com/us/app/%EC%98%88%EC%8A%A4%EB%B1%85%ED%82%B9/id1410057443 |
| `korea` | 한국투자(저축은행·증권) | 공식 홈페이지 아이콘 — https://www.truefriend.com |
| `daol` | 다올(저축은행·증권) | 공식 홈페이지 아이콘 — https://www.daolsb.com |
| `ls` | LS증권 | 공식 홈페이지 아이콘 — https://www.ls-sec.co.kr |
| `hana` | 하나(은행 등) | 공식 홈페이지 아이콘 — https://www.kebhana.com |

## 아직 공식 로고가 없는 곳 (지금은 그린 표시·첫 글자로 보임)

앱 아이콘이 회사 로고가 아니라 상품 이름(예: 신한 ‘super SOL’, 우리 ‘WON’, NH ‘all one’)이거나, 공식 사이트에서 자동으로 받을 수 없던 곳이다. 각 회사 홈페이지의 ‘회사소개 › CI’ 페이지에서 심볼 파일을 받아 위 방법대로 넣는다.

| 키 | 이름 | 공식 홈페이지 |
|---|---|---|
| `shinhan` | 신한은행·신한저축은행·신한투자증권 | https://www.shinhan.com |
| `woori` | 우리은행·우리금융저축은행·우리투자증권 | https://www.wooribank.com |
| `nh` | NH농협은행·NH저축은행·NH투자증권 | https://www.nonghyup.com |
| `ibk` | IBK기업은행·IBK저축은행·IBK투자증권 | https://www.ibk.co.kr |
| `post` | 우체국 | https://www.epostbank.go.kr |
| `cu` | 신협 | https://www.cu.co.kr |
| `jb` | 전북은행 | https://www.jbbank.co.kr |
| `kj` | 광주은행 | https://www.kjbank.com |
| `jeju` | 제주은행 | https://www.e-jejubank.com |
| `sbi` | SBI저축은행 | https://www.sbisb.co.kr |
| `welcome` | 웰컴저축은행 | https://www.welcomebank.co.kr |
| `jt` | JT친애저축은행 | https://www.jtchinae-bank.co.kr |
| `sangsangin` | 상상인 | https://www.sangsanginbank.co.kr |
| `daishin` | 대신증권·대신저축은행 | https://www.daishin.com |
| `kiwoom` | 키움증권·키움YES저축은행 | https://www.kiwoom.com |
| `mirae` | 미래에셋증권 | https://securities.miraeasset.com |
| `sk` | SK증권 | https://www.sks.co.kr |
| `shinyoung` | 신영증권 | https://www.shinyoung.com |
| `eugene` | 유진투자증권 | https://www.eugenefn.com |
| `heungkuk` | 흥국증권 | https://www.heungkuksec.co.kr |
| `yuanta` | 유안타 | https://www.myasset.com |
| `baro` | 바로저축은행 | https://www.barosavings.com |
| `cheongju` | 청주저축은행 | https://www.cjbank.co.kr |
| `line-music` | LINE MUSIC | https://music.line.me |
| `pretzel` | Pretzel | https://www.pretzel.rocks |
| `touchtunes` | TouchTunes | https://www.touchtunes.com |
| `lickd` | Lickd | https://lickd.co |
| `adaptr` | Adaptr | https://www.adaptr.com |
| `styngr` | Styngr | https://styngr.com |
