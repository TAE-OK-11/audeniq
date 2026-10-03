# AUDENIQ 백엔드 기술 점검 — 2026-10-03

범위: 대한민국 서비스, 일본 도쿄 Akamai Connected Cloud 백엔드, Cloudflare Workers/D1/R2. 결제·실제 본인확인 제공자는 미지정이다. 이 변경은 DB·암호화·키 관리·접근 기록·백업·통신 구현을 다루며 약관이나 개인정보 처리방침을 새로 작성하지 않는다. 소스 변경과 운영 서버 적용을 구분해야 한다.

## 처리 정보와 기술적 보호

| 처리 정보 | 확인한 기존 보호 | 이번 변경 |
|---|---|---|
| 로그인 비밀번호 | Argon2 단방향 해시, 원문 비밀번호 DB 저장 없음 | 유지. PostgreSQL SQL 매개변수 로그를 비활성화 |
| 이메일·프로필·조직·권리자 이름·전자 문서·정산 | 세션/CSRF/회원·직원 권한 검사, 테넌트별 접근 검사, API/worker/owner DB 역할 분리 | 일반 DB 로그인에 PUBLIC CONNECT/TEMP 권한 제거, SCRAM 명시, 유휴 트랜잭션 제한 |
| 계좌번호 | AES-256-GCM, 조직 UUID를 AAD로 결합, 마지막 네 자리만 응답 | 키 버전과 필드·조직·헤더 AAD, 키 회전/이전 데이터 재암호화, 환경변수 키의 운영 사용 금지, tmpfs 키 파일, 키 버퍼 zeroize |
| 서명 본인확인 | 실제 제공자 미설정 시 거절, CI 원문 대신 비밀값을 결합한 해시 | 유지. 로그에서 서명 URL 토큰을 제거하고 경로 템플릿만 기록 |
| 직원 개인정보 접근 | 직원 역할 검사, 기존 작업 변경 audit | 조회도 계정·시각·IP·경로·작업·대상 UUID를 전용 접근 로그 테이블에 기록. 기록 실패 시 데이터 응답 거절 |
| 세션·인증 제한 | 세션 토큰/CSRF 값과 인증 제한 식별자의 해시 | 만료·철회 후 24시간 지난 세션과 오래된 인증 제한 창을 owner만 제한된 묶음으로 파기 |
| 음원·커버·업로드 증명 서류 | 비공개 R2, 객체별 짧은 서명 URL, 버킷 제한 토큰 | 외부 HTTPS 강제, 객체 응답 자동 압축 해제 금지 |
| DB 전체 백업 | 기존 custom pg_dump는 파일에 평문 저장 | age 공개키 암호화 스트림, 오류 시 미완료 파일 제거, 별도 복원 검증 도구 |

현재 구조화된 입력에는 주민등록번호·여권번호·운전면허번호·외국인등록번호·카드번호를 받는 필드가 없다. 자유 입력·스캔 서류에 이를 넣는 경우까지 입력 필드 목록만으로 배제할 수는 없다. 실제 운영 데이터·업로드 내용과 취급자의 단말 저장·백업은 별도 확인 대상이다. CI의 해시는 익명화 증명이 아니며 개인정보로 관리한다.

[안전성 확보조치 기준 제7조](https://www.law.go.kr/LSW/admRulSideInfoP.do?admRulSeq=2100000265956&chrClsCd=010201&dashNo=&docCls=jo&joBrNo=00&joNo=0007&urlMode=admRulScJoRltInfoR)는 비밀번호 단방향 저장, 해당 식별·결제 정보 저장 암호화와 인터넷 전송 암호화를 규정한다. 모든 일반 프로필 필드에 DB 암호화를 일률적으로 의무화하거나 특정 상용 KMS 제품을 지정하는 조항으로 해석하지 않았다. 계좌는 애플리케이션 암호문이 DB·WAL·덤프에 저장되므로 디스크 암호화 설정 하나에 의존하지 않는다.

R2의 기본 서버 측 저장 암호화는 [공식 데이터 보안 문서](https://developers.cloudflare.com/r2/reference/data-security/)에 설명되어 있다. 객체 접근 권한·공개 버킷 여부·지역·법적 보유 기간을 이 기능만으로 보장하지는 않는다. PostgreSQL 전체 디스크와 공급자 스냅샷의 암호화 여부는 실제 Akamai 호스트 설정을 확인해야 한다. PostgreSQL 18에 존재하지 않는 TDE 설정을 추가하지 않았다.

[안전성 확보조치 기준 제8조](https://www.law.go.kr/LSW/admRulSideInfoP.do?admRulSeq=2100000265956&chrClsCd=010201&dashNo=&docCls=jo&joBrNo=00&joNo=0008&urlMode=admRulScJoRltInfoR)의 접속 기록 보관 요구에 대비해 직원 접근 로그는 기본 2년을 설정했다(운영 선택). runtime에는 INSERT만 부여하며 UPDATE/TRUNCATE를 거절하고 보유 기간 이전 DELETE를 차단한다. DB 소유자의 권한까지 제거하는 암호학적 WORM은 아니다. DB 로그를 읽는 운영자 권한과 외부 불변 보관·정기 점검은 운영 설정 대상이다. 경로의 대상 UUID는 실제 문서·발매 등 DB 기록과 연결해 조사한다. 목록 조회의 개별 대상 추적과 공급자 관리 콘솔 접근 로그도 운영 점검에 포함한다.

[개인정보 파기 안내](https://easylaw.go.kr/CSP/CnpClsMain.laf?ccfNo=2&cciNo=2&cnpClsNo=3&csmSeq=1257&popMenu=ov)에 따라 처리 목적 종료·보유 기간 경과 자료는 파기하고 법적 보존 대상은 분리 관리해야 한다. 이번 자동 파기는 인증용 일시 자료와 보관 기한이 끝난 접근 로그에 한정한다. 원장·계약·봉인된 문서·객체를 임의의 5년 규칙으로 삭제하지 않는다. 해당 데이터의 실제 보유 근거와 법적 보존/분리 관리·백업 만료까지 확정되지 않아 전체 개인정보 파기가 완성됐다고 주장할 수 없다.

## KMS 키 준비와 회전

운영자는 **AWS KMS 또는 GCP Cloud KMS**를 선택한다. AWS는 기존 대칭 키 ARN과 AWS CLI 자격, GCP는 기존 `ENCRYPT_DECRYPT` CryptoKey 전체 리소스 이름과 `gcloud` ADC 자격이 필요하다. 공급자·리전·IAM 설정은 운영자가 준비하며 이 도구가 클라우드 키·유료 자원·계정을 생성하지 않는다. Akamai 서버는 그대로 두고 선택한 KMS를 HTTPS로 호출한다.

`deploy/kms-keyring.py`의 AWS 경로는 공식 CLI로 TLS·인증서 검증·SigV4를 처리한다. GCP 경로는 `gcloud auth application-default print-access-token`의 출력을 내부에서 읽고 `https://cloudkms.googleapis.com` REST API에 TLS 1.2 이상과 인증서 검증을 적용한다. redirect는 거절한다. 디스크에는 KMS로 감싼 키만 두고, API용 평문 키 파일은 host tmpfs에 쓴다. API 컨테이너와 worker는 AWS/GCP 자격을 받지 않는다.

AWS [KeyId](https://docs.aws.amazon.com/kms/latest/APIReference/API_Decrypt.html)와 GCP CryptoKey 이름을 고정한다. [GCP Encrypt 응답](https://cloud.google.com/kms/docs/reference/rest/v1/projects.locations.keyRings.cryptoKeys/encrypt)의 버전 리소스가 지정한 CryptoKey에 속하는지 확인하고, 요청 검증 플래그와 암복호화 응답 CRC32C를 확인한다. [무결성 지침](https://cloud.google.com/kms/docs/data-integrity-guidelines)에 따라 평문 checksum은 저장하지 않는다. AWS [EncryptionContext](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html)와 GCP AAD에는 서비스·용도·계좌 키 버전만 넣는다.

새 감싼 키 묶음은 `version: 2`이며 각 키에 `provider: aws|gcp`를 저장한다. 기존 `version: 1` AWS 묶음도 그대로 읽고, 다음 변경 때 공급자를 명시한 형식으로 전환한다. 한 묶음에 과거 AWS 버전과 새 GCP 버전이 함께 있을 수 있다. `materialize`는 각 키의 공급자를 읽으므로 양쪽 자격이 필요하며 장애 시 다른 공급자를 임의로 대신 호출하지 않는다. API가 읽는 복호화 키 파일의 형식은 기존과 동일하다.

1. 기존 계좌가 있으면 기존 `PAYOUT_ACCOUNT_KEY`를 정확히 버전 1로 보존한다. 안전한 관리 절차로 `/dev/shm/legacy-payout-key`에 64 hex 값을 작성하고 mode 0600을 설정한다. 값을 명령 인자·셸 기록·출력에 넣지 않는다.
2. 선택한 공급자로 가져온다. `AUDENIQ_KMS_PROVIDER=aws|gcp`, `AUDENIQ_KMS_KEY_ID`는 AWS ARN 또는 GCP의 `projects/PROJECT/locations/LOCATION/keyRings/RING/cryptoKeys/KEY`이다. 두 변수는 host의 키 준비 작업에만 전달한다:

   ```bash
   python3 deploy/kms-keyring.py wrap-legacy --provider "$AUDENIQ_KMS_PROVIDER" --bundle /opt/audeniq/payout-wrapped-keyring.json --key-id "$AUDENIQ_KMS_KEY_ID" --legacy-key-file /dev/shm/legacy-payout-key
   ```

   기존 계좌가 없는 신규 설치는 아래 `generate`부터 시작한다.
3. 새 버전을 생성하고 복호화 키 파일을 준비한다:

   ```bash
   python3 deploy/kms-keyring.py generate --provider "$AUDENIQ_KMS_PROVIDER" --bundle /opt/audeniq/payout-wrapped-keyring.json --key-id "$AUDENIQ_KMS_KEY_ID"
   python3 deploy/kms-keyring.py materialize --bundle /opt/audeniq/payout-wrapped-keyring.json
   ```

4. `production.env`의 `PAYOUT_KEYRING_DIR=/dev/shm/audeniq-secrets`를 확인한다. 파일은 root:10001, 0640, 디렉터리는 root:10001, 0750이다. production API와 배포 스크립트는 파일이 없거나 잘못된 권한·형식·tmpfs이면 거절한다. API 시작 시 키를 메모리에 읽어 요청마다 KMS 호출하지 않는다. 회전 후 API를 재시작해 새 키를 사용한다.
5. owner DB 연결과 해당 키 파일을 제공한 운영 CLI로 `audeniq-admin --operator NAME privacy reencrypt-accounts`를 반복 실행한다. 한 번에 100개를 트랜잭션으로 재암호화하며 숫자를 출력하지 않는다. `reencrypted: 0`이 될 때까지 실행한다. API 계정에는 이 작업 권한이 없다.
6. 기존 데이터·암호화된 이전 백업이 모두 만료/전환됐는지 확인하기 전에는 과거 키 버전을 제거하지 않는다. 키 분실은 복구 불능이므로 감싼 키 묶음과 KMS 복구·IAM 절차를 DB 백업과 별도로 관리한다. 전환과 복원 검증 후 기존 환경변수 키와 임시 legacy 파일을 정리한다.

재부팅 시 tmpfs는 사라진다. 운영 host의 systemd 등에서 `materialize` 성공 후 Compose를 시작하도록 순서를 설정해야 한다. KMS 장애·IAM 실패·다른 키 응답·CRC 오류이면 키 파일을 교체하지 않는다. 배포 스크립트가 준비되지 않은 키 때문에 API를 교체하지 않도록 사전 검사한다. AWS runtime 주체는 해당 ARN의 `kms:Decrypt`와 정확한 context만, 키 준비 주체는 별도 `kms:Encrypt`/`kms:GenerateDataKeyWithoutPlaintext` 권한만 사용한다. GCP runtime 주체는 해당 CryptoKey의 `cloudkms.cryptoKeyVersions.useToDecrypt`, 키 준비 주체는 `cloudkms.cryptoKeyVersions.useToEncrypt`가 필요하다. 전체 계정의 키 관리·삭제 권한을 runtime에 주지 않는다.

GCP ADC는 host에만 준비한다. 운영에서는 서비스 계정 또는 Workload Identity Federation 자격을 사용할 수 있다. [ADC 토큰 명령](https://cloud.google.com/sdk/gcloud/reference/auth/application-default/print-access-token)에 설명된 quota project가 필요한 자격은 host에 `GOOGLE_CLOUD_QUOTA_PROJECT`를 지정한다. 해당 프로젝트의 `serviceusage.services.use` 권한도 필요하다. 토큰을 직접 명령 인자·셸 기록·환경 파일에 복사하지 않는다.

이미 사용 중인 계좌 AES 키를 **AWS ↔ GCP로 옮기는 경우**에는 다음을 실행한다:

```bash
python3 deploy/kms-keyring.py rewrap --provider "$AUDENIQ_KMS_PROVIDER" --key-id "$AUDENIQ_KMS_KEY_ID" --bundle /opt/audeniq/payout-wrapped-keyring.json
python3 deploy/kms-keyring.py materialize --bundle /opt/audeniq/payout-wrapped-keyring.json
```

`rewrap`은 키 버전·AES 키 값·active 버전을 그대로 두고 KMS 포장만 바꾸므로 DB 재암호화가 필요하지 않다. 모든 원본 키를 복호화하고 새 KMS로 감싸는 작업이 성공한 뒤에만 묶음을 원자적으로 교체한다. 이전 묶음·백업 복구에 필요한 과거 공급자의 키와 권한은 전환/복원 검증 전까지 유지한다. 계좌용 AES 키 자체를 새로 만드는 `generate`는 위 재암호화 CLI 절차를 따르는 별도 회전이다. GCP의 기존 KMS 버전도 과거 묶음·백업이 사용하는 동안 유지한다.

## 백업과 제한된 파기

백업 host에는 `age`가 필요하다. 별도 안전한 관리 환경에서 생성한 age 공개키를 `BACKUP_RECIPIENTS_FILE`에 넣는다. cron 또는 systemd 작업에도 이 환경변수를 명시한다. Compose의 `production.env`는 host의 cron에 자동 전달되지 않는다. 복호화 개인키는 백업 host 밖에 보관한다. `backup.sh`는 `pg_dump | age`의 두 프로세스 성공을 확인하고 `.dump.age`만 완성본으로 남긴다. 오류 후 보유 백업을 지우지 않는다. 기본 보유 14일은 운영 선택이다. 기존 평문 `.dump`, 기존 서버 스냅샷, 이미 복사된 백업은 이 코드가 자동으로 암호화하지 않는다.

`BACKUP_IDENTITY_FILE=/secure/identity ./restore-backup.sh FILE.dump.age --verify`는 복호화 스트림과 archive 목차를 검사하며 DB를 수정하지 않는다. `--restore-empty`는 운영 DB 이름을 거절하고 운영자가 만든 빈 별도 DB만 허용한다. 실제 복원 데이터·API 회귀 검사는 격리된 DB에서 추가로 수행한다.

owner DB 연결로 다음을 주기적으로 실행한다:

```bash
audeniq-admin --operator scheduled-maintenance privacy purge-transient
```

한 번에 각 테이블 10,000행을 넘지 않고 실행자·개수만 audit에 남긴다. 인증 창의 15분 제한은 유지되며 파기는 24시간 뒤 실행한다. 파기 대상이 많은 경우 작업을 반복하고 실패를 모니터링한다. 계정 탈퇴·법적 보존 자료와 전체 파기를 이 명령 하나로 처리한다고 해석하지 않는다.

## 외부 HTTPS와 응답 처리

공개 Worker는 평문 요청을 처리하기 전에 거절하며 HTTPS 업그레이드 redirect로 처리하지 않는다. 내부 Compose HTTP·PostgreSQL 구간은 기존 비공개 네트워크를 유지한다. 외부 S3/파트너/OAuth URL은 검증된 TLS 1.2 이상으로 요청하고 redirect를 따라가지 않는다. 개발용 HTTP 예외는 명시적으로 허용한 loopback에만 적용되며 production에서는 비활성화한다.

`deploy/cloudflare-network.py`는 기본으로 읽기와 검토용 계획만 출력한다. WAF·Compression Rules 편집 권한을 가진 zone 토큰으로 검토한 설정을 적용해야 CDN에서도 HTTP를 origin 이전에 차단하고 JSON zstd를 협상한다. Workers Fetch hop은 Brotli/gzip만 지원하므로 zstd를 무조건 전달하지 않는다. API 스트림은 재직렬화하지 않으며 JSON 입력은 바이트 한도 안에서 읽는다. 동시 점검 공지 조회는 한 DB 요청으로 합친다.

Studio/관리자 정적 파일은 빌드할 때 Brotli sidecar를 만들고 요청 시 품질값·Range·MIME·ETag를 보존한다. 로컬 실제 빌드에서 Studio 압축 가능 파일 3,829,928 → 1,097,556 bytes(71.3% 감소), 관리자 1,012,939 → 274,096 bytes(72.9% 감소)를 측정했다. 운영 지연이나 처리량 개선 비율은 부하 테스트로 측정하지 않았으므로 수치로 주장하지 않는다.

## 검증과 적용 상태

- 로컬 Worker/전송 회귀 37개, Survey 25개, Studio 86개, Admin 14개 통과. Studio/Admin 타입 검사와 실제 빌드 통과.
- [GitHub Actions Foundation](https://github.com/TAE-OK-11/audeniq/actions/runs/37110993569)에서 Rust 형식·Clippy·전체 바이너리 빌드, PostgreSQL 통합 테스트를 포함한 439개 테스트 통과(3개 skipped). Docker API/worker 기동·Workers 번들·Studio 브라우저 검사도 통과했다.
- AWS/GCP 선택·기존 AWS 묶음 호환·혼합 공급자·양방향 rewrap·키 버전 AAD·GCP TLS/CRC32C/ADC·원자적 교체·파일 권한과 실제 age 암복호화 테스트 22개가 로컬에서 통과했다. Foundation의 독립 `kms-backup` 작업에서도 실행한다. KMS/인증 네트워크 호출은 테스트 대역을 사용하며 실제 IAM/KMS 연결 성공을 뜻하지 않는다.
- Studio/Admin Actions와 Rust 의존성 보안 검사도 통과했다. 로컬 Studio/Admin Wrangler 배포 dry-run을 확인했다.
- 운영 서버·Cloudflare zone에는 아직 적용하지 않았다. KMS ARN/자격·기존 키 이전·재부팅 준비·age 수신자와 Cloudflare 규칙 권한이 실제로 준비돼야 운영 적용을 완료할 수 있다.
- 일본 보관 및 기타 Cloudflare 처리 위치는 [개인정보 보호법 제28조의8](https://www.law.go.kr/LSW/lsSideInfoP.do?docCls=jo&joBrNo=08&joNo=0028&lsiSeq=283839&urlMode=lsScJoRltInfoR)의 국외 이전 검토 대상이다. 암호화 구현만으로 법적 근거·위탁 계약·실제 공급자 국가를 확인할 수 없다. 약관 작성 범위와 구분하되 이 운영 의무를 충족했다고 단정하지 않는다.
