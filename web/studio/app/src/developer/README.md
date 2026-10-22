# 개발자 앱 (src/developer)

AUDENIQ API(`/api/v1`)를 OAuth 2.0으로 쓰는 앱을 등록·관리하는 화면이에요.
지금은 스튜디오 프로필(개발자 → 개발자 앱)에 있지만 나중에 개발자 플랫폼으로 분리해요.
분리할 때 이 폴더만 옮기면 되도록, 스튜디오의 다른 화면은 이 폴더를 가져다 쓰지 않아요
(`pages/Profile.tsx`가 `DeveloperApps`를 띄우는 것만 빼고).

- `api.ts` — `/api/developer/apps` 클라이언트
- `DeveloperApps.tsx` — 목록·만들기·관리 화면

자세한 내용과 분리 순서는 docs/OAUTH_API.md에 있어요.
