# 원본 문서 보관과 위·변조 방지

계약서, 배급 신청서, 권리 서류, 서명 요청 문서, 약관 동의 기록은 모두 전자로 만들어진다. 이 문서는 세 가지를 설명한다.

1. 전자문서를 원본으로 인정받으려면 무엇을 보관해야 하는가(대한민국 법령).
2. 위조·변조·부인·삭제에 어떻게 대응하는가.
3. 운영자가 무엇을 설정하는가.

- 코드: `crates/core/src/seal.rs`(봉인), `crates/core/src/archive.rs`(보관·고정·검증)
- 명령: `audeniq-admin seal verify`, `audeniq-admin archive ...`
- 자동 실행: `deploy/archive.sh` (호스트 cron)
- DB: `migrations/0108_document_integrity.sql`

## 1. 법적 요건과 대응

| 근거 | 요구 사항 | 대응 |
|---|---|---|
| 전자문서 및 전자거래 기본법 제5조 제1항 제1호 | 전자문서의 내용을 열람할 수 있을 것 | `archive open KEY`로 보관 원본을 복호화해 다시 볼 수 있다. 화면 열람과 출력은 기존 기능을 그대로 쓴다. |
| 같은 조 제2호 | 작성·송신·수신된 때의 형태 또는 같은 형태로 재현될 수 있는 형태로 보존될 것 | 서명·제출 시점의 내용을 고정된 정규형(JSON)으로 만들어 SHA-256으로 봉인한다. 그 정규형과 첨부 파일 원본(바이트 그대로)을 월마다 보관 버킷에 올린다. |
| 같은 조 제3호 | 작성자, 송신자·수신자, 송수신 일시에 관한 사항이 있으면 함께 보존할 것 | 봉인 기록에 행위자(계정·서명 링크 수신자), 시각, 사건(SIGNED·SUBMITTED·DECLINED)을 남긴다. 서명 문서의 사건 기록(열람·동의·서명)도 함께 보관한다. |
| 상법 제33조 | 영업에 관한 중요서류는 10년 보존 | 계약서·신청서·권리 서류·서명 문서의 보관 기한은 10년이다(`retain_until`). |
| 상법 시행령 제3조 | 전산정보처리조직으로 보존하는 경우 운영 기록과 경위를 남기고, 멸실·훼손에 대비할 것 | 보관 실행 기록(`operations.archive_runs`), 서명된 월별 목록(manifest), 소프트웨어 버전과 실행자를 남긴다. 보관 버킷은 별도 버킷이며 WORM이다. 매월 전체 검증을 한다. |
| 전자상거래법 제6조 | 계약·청약철회 기록 5년 | 약관 동의를 서버에 기록한다(`identity.terms_acceptances`): 판, 시각, 경로(가입·소셜 로그인), IP·브라우저(봉인). 보관 기한은 5년이다. |
| 개인정보 보호법 제21조 | 보유기간이 지나면 지체 없이 파기 | `archive purge-expired`가 보존기한이 지난 보관 객체를 지우고 그 사실을 기록한다. 기한 전에는 DB 트리거가 삭제 표시를 막는다. |
| 개인정보 보호법 제29조 (안전성 확보조치) | 개인정보 암호화·접근 통제 | 보관 객체는 개인정보 키링(AES-256-GCM)으로 봉인한다. 보관 버킷의 자격증명은 API 컨테이너에 두지 않는다. |

### 이번에 새로 채운 빈틈

- **약관 동의의 서버 기록.** 전에는 화면에서만 체크했다. 이제 가입할 때 `terms_version`이 없으면 거절하고, 동의 기록을 봉인한다. 소셜 로그인 가입도 같다(`TERMS_ACCEPTANCE_REQUIRED`).
- **원본의 별도 보관.** DB와 다른 곳(별도 버킷, 가능하면 다른 사업자)에 월마다 원본을 쌓는다.
- **보존기한과 파기.** 법정 기한(10년/5년)을 객체마다 적고, 지나면 파기한다.

### 운영자가 따로 검토할 것 (선택)

- **공인전자문서센터**(전자문서법 제31조의2 이하): 제3자가 보관해 주면 보관 사실이 추정된다. 분쟁 가능성이 큰 계약은 공인전자문서센터 보관을 함께 쓰는 것을 권장한다.
- **신뢰할 수 있는 시각 확인**(RFC 3161 타임스탬프): 우리 앵커는 우리 키로 서명하고 WORM에 기록한다. 시각을 제3자가 확인해 주어야 하면 일일 앵커의 해시를 외부 타임스탬프 기관에 맡긴다. 코드는 앵커 해시만 넘기면 되게 되어 있다.
- **전자서명법상 인증 서비스**: 지금 서명은 전자서명법 제2조의 일반 전자서명이다(본인 확인, 동의 화면, 사건 기록, 봉인). 공인 인증사업자의 서명이 필요한 계약 유형이 생기면 별도로 연동한다.

이 절은 법률 자문이 아니다. 실제 운영 전에 법무 검토를 받는다.

## 2. 위협별 대응

| 위협 | 대응 | 확인 방법 |
|---|---|---|
| 문서 위조·변조 | 서명·제출 순간 문서 정규형의 SHA-256과 Ed25519 디지털 서명(봉인)을 남긴다. 공개키는 `GET /api/document-keys`에 공개한다. | `seal verify`가 현재 DB 내용을 다시 정규화해 봉인 해시와 비교한다(`DOCUMENT_CHANGED`). 서명도 공개키로 검증한다(`BAD_SIGNATURE`). |
| 출력본·PDF 유출 | 출력할 때마다 원본 검증코드를 새로 발급하고, 출력자·기기·세션을 기록한다(`docs/PRINT_PROVENANCE.md`). 모든 쪽에 출력자 워터마크(가린 이메일 + 코드)를 대각선으로 반복해 찍는다. | 종이에 찍힌 코드를 `/verify` 또는 관리 화면에 넣으면 어느 계정이 언제 어느 기기로 뽑은 사본인지 나온다. |
| 전자서명 부인 | 봉인 기록에 누가(계정 또는 서명 링크 수신자), 무엇에(문서 내용 해시), 언제, 무엇을(서명·거절·제출) 했는지 남긴다. 약관 동의도 봉인한다. 서명 요청은 열람·동의·서명 사건 기록을 함께 남긴다. | `seal verify`, 보관 원본(`archive open`)으로 당시의 내용과 행위자를 보인다. |
| DB 공격·기록 조작 | 봉인 기록은 해시 체인이다(앞 기록의 해시를 포함). 수정·삭제는 DB 트리거가 막는다. API 계정은 추가와 조회만 할 수 있다. 체인의 끝을 매일 WORM 버킷에 서명해 고정한다(앵커). | `archive verify`는 앵커를 DB가 아니라 버킷에서 읽어 비교한다. 누가 DB 권한을 얻어 봉인까지 다시 쓰면 `CHAIN_DIFFERS_FROM_ANCHOR`, 앵커를 지우면 `ANCHOR_MISSING`. |
| 랜섬웨어·삭제 공격 | 원본·월별 목록·앵커·DB 백업·PITR(WAL)을 WORM 버킷에 쌓는다(S3 Object Lock COMPLIANCE 또는 R2 버킷 잠금). 루트 계정도 기한 전에는 지울 수 없다. 백업은 age로 암호화하고, 개인키는 서버 밖에 둔다. | `archive verify --deep`이 매월 모든 보관 객체를 내려받아 해시를 확인한다. 복구 절차는 아래 4절. |

**서명 키.** 봉인 서명 키는 키링 판마다 따로 파생한다(HMAC `audeniq:document-signing-key:v1`). 키를 교체해도 옛 공개키는 계속 공개해 옛 봉인을 검증한다. 키링이 없는 개발 환경에서는 해시 체인만 남고 서명은 비어 있다(`unsigned_seals`).

## 3. 보관 버킷 구조

```
documents/<종류>/<연도>/<문서 id>/<내용 해시 16자>.json[.sealed]   봉인 당시 문서 정규형 (내용 판마다 하나)
files/<연도>/<문서 id>/<파일 해시 32자>[.sealed]                  첨부 파일 원본 (바이트 그대로)
manifests/<YYYY-MM>.json, .sig.json                               누적 목록 + 서명(앞 달 목록 해시를 포함한 체인)
anchors/<YYYY-MM-DD>.json                                         봉인 체인 끝 (서명)
backups/<pg_dump 파일>.age                                         일일 DB 백업 사본 (35일)
pitr/base/, pitr/wal/                                             PITR 기준 백업과 WAL (35일, age)
```

`.sealed` 객체는 개인정보 키링으로 봉인돼 있다. 키링 없이 버킷만 빼가도 읽을 수 없다. 목록과 앵커에는 id와 해시만 있다.

## 4. 설정

### 버킷

1. 작업용 버킷(`S3_BUCKET`)과 **다른 버킷**을 만든다. 가능하면 다른 사업자나 리전에 둔다. 예: 작업은 R2, 보관은 AWS S3.
2. WORM을 건다.
   - **AWS S3**: 버킷을 만들 때 Object Lock을 켠다. `ARCHIVE_OBJECT_LOCK=true`로 두면 객체마다 COMPLIANCE 모드 보존기한(10년/5년/35일)을 건다. 기본 보존 규칙을 따로 둘 필요는 없다.
   - **Cloudflare R2**: 버킷 잠금 규칙(Bucket Lock)을 접두어별로 둔다. 예: `documents/`·`files/`·`manifests/`·`anchors/` 10년, `backups/`·`pitr/` 35일. 그리고 `ARCHIVE_OBJECT_LOCK=false`.
3. 보관 버킷 전용 자격증명을 만든다(쓰기·읽기). 파기를 돌릴 때만 삭제 권한이 필요하다.
4. `production.env` 또는 `SECRETS_ENV_FILE`에 `ARCHIVE_S3_*`를 채운다. 이 값은 API 컨테이너 환경에 들어가지 않고, `archive.sh`가 명령을 실행할 때만 넘긴다.

### cron (호스트, UTC)

```
30 18 * * *  /opt/audeniq/backup.sh                 # 기존 일일 pg_dump (KST 03:30)
10 19 * * *  /opt/audeniq/archive.sh daily          # 앵커 + 봉인 검증 + 백업 사본 (KST 04:10)
40 19 1 * *  /opt/audeniq/archive.sh monthly        # 지난달 원본 보관 + 전체 검증 (매월 1일)
50 19 2 * *  /opt/audeniq/archive.sh purge          # 보존기한 지난 객체 파기
*/15 * * * * /opt/audeniq/archive.sh wal            # PG_ARCHIVE_MODE=on일 때
20 20 * * 0  /opt/audeniq/archive.sh basebackup     # PITR 기준 백업 (매주)
```

로그는 `>> /var/log/audeniq-archive.log 2>&1`로 남긴다. 검증에서 문제가 나오면 명령이 실패 코드로 끝난다. cron 실패 알림을 받도록 설정한다.

### PITR (시점 복구)

1. `production.env`에 `PG_ARCHIVE_MODE=on`을 두고 postgres를 재시작한다. PostgreSQL이 WAL 조각을 볼륨 안의 `wal-archive/`에 복사한다.
2. `archive.sh wal`이 그 조각을 age로 암호화해 `pitr/wal/`에 올리고 로컬 사본을 지운다.
3. `archive.sh basebackup`을 한 번 바로 실행하고, 그 다음은 매주 돌린다.

**복구 절차** (빈 서버에서, 개인키는 복구할 때만 가져온다):

1. 버킷에서 원하는 시점 이전의 가장 최근 `pitr/base/base-*.tar.gz.age`와 그 뒤의 `pitr/wal/*.age`를 받는다.
2. 기준 백업을 `age -d -i IDENTITY`로 풀고, 새 데이터 디렉터리에 `base.tar.gz`를 푼다.
3. WAL을 풀어 한 디렉터리에 둔다. 다음을 설정한다.
   - `restore_command = 'cp /restore/wal/%f %p'`
   - `recovery_target_time = '2026-10-12 03:00:00+09'`
4. `recovery.signal`을 만들고 postgres를 시작한다. 복구가 끝나면 다음을 확인한다.
   - `audeniq-admin seal verify`
   - `archive verify --deep`
5. DB를 되살릴 수 없을 때도 원본은 남는다. 보관 버킷의 `documents/`·`files/`와 서명된 manifest로 각 문서의 당시 내용과 서명 사실을 보일 수 있다.

### 수동 명령

```
audeniq-admin --operator 이름 seal verify                 # 봉인·서명·체인 검증
audeniq-admin --operator 이름 archive run [YYYY-MM]       # 원본 보관 (기본: 지난달)
audeniq-admin --operator 이름 archive anchor              # 체인 끝 고정
audeniq-admin --operator 이름 archive verify [--deep]     # 앵커·보관 객체 검증
audeniq-admin --operator 이름 archive status
audeniq-admin --operator 이름 archive open KEY [--out FILE]   # 열람 (기록이 남는다)
```

## 5. 한계

- 이 기능 전에 서명·제출된 문서는 월간 보관이 돌 때 먼저 봉인된다(사건 `BACKFILL`). 그 봉인은 봉인한 때부터의 내용을 보증한다. 서명 시점의 내용까지 보증하지는 않는다.
- 앵커는 하루 단위다. 마지막 앵커 뒤, 다음 앵커 전에 DB 권한을 가진 공격자가 기록을 고치면 그 사이의 봉인은 앵커로 잡지 못한다. 해시 체인과 서명 검증은 그대로 동작한다. 더 짧게 하려면 `archive.sh daily`의 앵커만 더 자주 돌린다.
- 워터마크는 유출 경로를 추적하는 수단이지 복제 방지가 아니다. 화면 촬영은 막지 못한다.
