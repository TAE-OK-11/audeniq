# 파일 저장소: AWS S3 · Cloudflare R2 · Oracle Cloud

음원·커버·권리 서류 업로드(`S3_*`)와 원본 보관(`ARCHIVE_S3_*`)은 S3 API로 동작한다. 사업자는 엔드포인트 주소로 자동 판별한다. 작업 버킷과 보관 버킷은 서로 다른 사업자여도 된다. 예를 들어 업로드는 R2, 보관은 AWS S3로 둘 수 있다.

| | Cloudflare R2 | AWS S3 | Oracle Cloud (OCI) |
|---|---|---|---|
| `S3_ENDPOINT` | `https://<계정>.r2.cloudflarestorage.com` | `https://s3.<리전>.amazonaws.com` | `https://<네임스페이스>.compat.objectstorage.<리전>.oraclecloud.com` |
| `S3_REGION` | `auto` | `auto`(주소에서 읽음) 또는 리전 | `auto`(주소에서 읽음) 또는 리전. 서울 `ap-seoul-1`, 춘천 `ap-chuncheon-1` |
| 키 | 버킷 범위 API 토큰 | IAM 사용자 액세스 키 | 고객 비밀 키(Customer Secret Key) |
| 브라우저 업로드 CORS | `r2-cors.json` 적용 | `r2-cors.json`을 `CORSRules`로 적용 | **설정할 수 없음**(고정 헤더). 아래 점검 결과로 판단 |
| 격리본 1일 삭제 | 수명 주기 규칙 | 수명 주기 규칙 | 수명 주기 정책 규칙 |
| 보관 버킷 WORM | 버킷 잠금(접두어별) | Object Lock(`ARCHIVE_OBJECT_LOCK=true`) | 보존 규칙(버킷 전체) |

코드: `crates/core/src/storage.rs`의 `Provider`.

- OCI와 AWS는 서명에 실제 리전을 넣어야 한다. `S3_REGION=auto`로 두면 주소에서 리전을 읽는다. 값을 직접 적었는데 주소의 리전과 다르면 서버가 시작하지 않는다(그대로 두면 모든 서명이 실패한다).
- 모든 사업자에서 경로형 주소(`/버킷/키`)를 쓴다. OCI의 S3 호환 API는 경로형 주소를 지원한다.

## 설정 후 반드시 점검

사업자마다 S3 호환 범위가 조금씩 다르다. 실제 버킷에서 업로드가 기대는 동작을 직접 시험한다.

```
docker exec audeniq-production-api-1 audeniq-admin storage probe --origin https://studio.audeniq.com
docker exec -e ARCHIVE_S3_ENDPOINT=… -e ARCHIVE_S3_BUCKET=… -e ARCHIVE_S3_ACCESS_KEY_ID=… \
  -e ARCHIVE_S3_SECRET_ACCESS_KEY=… -e ARCHIVE_OBJECT_LOCK=… \
  audeniq-production-api-1 audeniq-admin storage probe --archive
```

`ok: true`가 아니면 그 버킷을 쓰지 않는다. 종료 코드는 2다.

| 결과 | 뜻 | 조치 |
|---|---|---|
| `SIGNED_PUT_FAILED` | 서명 URL로 올리지 못함 | 키 권한, 리전, 엔드포인트를 확인한다 |
| `SIGNED_PUT_NOT_SINGLE_USE` | 같은 URL로 두 번째 업로드가 됨(`If-None-Match` 무시) | 업로드 버킷으로 쓰지 않는다 |
| `METADATA_NOT_RETURNED` | `x-amz-meta-upload-nonce`가 돌아오지 않음 | 업로드 버킷으로 쓰지 않는다 |
| `COPY_FAILED`, `COPY_DROPS_METADATA` | 격리본 고정(서버 쪽 복사)이 안 됨 | 업로드 버킷으로 쓰지 않는다 |
| `COPY_CHANGES_ETAG` | 복사본 ETag가 다름(R2·AWS에서는 이상) | 엔드포인트를 확인한다 |
| `CORS_PREFLIGHT_REFUSED` | 그 주소의 브라우저가 직접 올릴 수 없음 | CORS를 적용한다. OCI라면 아래를 본다 |
| 경고 `COPY_IGNORES_IF_MATCH` | 조건부 복사를 무시함 | 단회 업로드가 지켜지면 괜찮다(올린 뒤 바뀔 수 없음) |
| `ARCHIVE_NOT_WORM` | 보관 객체가 지워짐 | 잠금·보존 규칙을 건다 |

점검은 `quarantine/probe/` 아래에 작은 객체를 만들고 지운다. 보관 점검의 `probe/` 객체는 보존 규칙 때문에 지워지지 않는다. 크기는 수십 바이트다.

## Cloudflare R2

`deploy/PRODUCTION.md`의 "R2 업로드"를 따른다. 비공개 버킷, 버킷 범위 토큰 2개, `r2-cors.json`, `quarantine/` 1일 수명 주기를 설정한다.

## AWS S3

1. 비공개 버킷을 만든다(퍼블릭 액세스 차단을 모두 켠다).
2. IAM 사용자 정책: 그 버킷의 `s3:PutObject`, `s3:GetObject`, `s3:DeleteObject`만 준다. worker용은 `s3:GetObject`만 준다.
3. CORS:
   ```
   aws s3api put-bucket-cors --bucket 버킷 \
     --cors-configuration "{\"CORSRules\": $(cat deploy/r2-cors.json)}"
   ```
4. 수명 주기: `quarantine/` 접두사 1일 후 만료, 완료되지 않은 멀티파트 업로드 2일 후 중단(`AbortIncompleteMultipartUpload`).
5. `S3_ENDPOINT=https://s3.ap-northeast-2.amazonaws.com`, `S3_REGION=auto`로 둔다.

## Oracle Cloud (OCI Object Storage)

1. **버킷**: 공개 여부는 Private으로 둔다. 사전 인증 요청(PAR)은 만들지 않는다.
2. **네임스페이스·리전**: 콘솔의 테넌시 상세(또는 `oci os ns get`)에서 네임스페이스를 확인한다. 엔드포인트는 `https://<네임스페이스>.compat.objectstorage.ap-seoul-1.oraclecloud.com`이다. 새 도메인 `….oci.customer-oci.com`도 된다.
3. **S3 호환 API 구획**: S3 API로 접근하는 버킷은 테넌시의 "Amazon S3 Compatibility API 지정 구획"에 있어야 한다. 기본은 루트 구획이다. 다른 구획에 버킷을 만들었다면 테넌시 상세에서 지정 구획을 바꾼다.
4. **사용자와 키**: 전용 IAM 사용자를 만들고 그 사용자의 고객 비밀 키를 만든다. 비밀 키는 만들 때 한 번만 보인다. 키 값은 `S3_ACCESS_KEY_ID`·`S3_SECRET_ACCESS_KEY`에 넣는다. 정책 예:
   ```
   Allow group audeniq-uploads to manage objects in compartment audeniq where target.bucket.name='audeniq-uploads'
   Allow group audeniq-uploads to read buckets in compartment audeniq where target.bucket.name='audeniq-uploads'
   ```
   worker용 사용자는 `read objects`만 준다.
5. **수명 주기**: 객체 수명 주기 정책 규칙을 둔다. 동작은 삭제, 기간 1일, 접두어 `quarantine/`이다. 완료되지 않은 멀티파트 업로드 규칙(동작 `ABORT`, 2일)도 둔다. OCI는 서비스 권한이 따로 있어야 규칙이 동작한다.
   ```
   Allow service objectstorage-ap-seoul-1 to manage object-family in compartment audeniq
   ```
6. **CORS**: OCI는 CORS 헤더가 고정이라 버킷별로 설정할 수 없다. `storage probe --origin https://studio.audeniq.com`이 `CORS_PREFLIGHT_REFUSED`를 내면 브라우저가 이 버킷에 직접 올릴 수 없다. 그때는 업로드는 R2나 AWS에 두고, OCI는 보관이나 백업에 쓴다.
7. **ETag**: OCI는 복사본에 새 ETag를 준다. 업로드 완료 때 크기, 형식, nonce를 맞춰 보고, 그 뒤 내용 전체를 검사하므로 따로 할 일은 없다.
8. **보관 버킷 WORM**: 보존 규칙(Retention rule)을 걸고, 잠금 시각이 지나면 규칙을 줄이거나 지울 수 없다. `ARCHIVE_OBJECT_LOCK=false`로 둔다. `true`로 두면 서버가 거절한다(OCI는 그 헤더를 무시하기 때문이다). **주의할 점**: OCI 보존 규칙은 버킷 전체에 걸린다. 10년 규칙을 걸면 다음 문제가 생긴다.
   - DB 백업과 PITR(35일)도 10년 동안 지울 수 없다.
   - 약관 동의 기록(5년)도 기한 뒤에 파기되지 않는다(개인정보 보호법 제21조).
   - 그래서 보관 버킷은 접두어별 기한을 걸 수 있는 AWS S3 Object Lock이나 R2 버킷 잠금을 권장한다.

## 화면 보안 정책(CSP)

스튜디오의 `connect-src`는 다음 주소로의 직접 업로드를 허용한다.

- `https://*.r2.cloudflarestorage.com`
- `https://*.amazonaws.com`
- `https://*.oraclecloud.com`
- `https://*.oci.customer-oci.com`

이 값은 `config/studio-*.txt`와 `web/studio/**/public/_headers`에 있다. 다른 사업자를 쓰려면 이 값부터 고쳐야 한다.

## 참고

- [Oracle: Amazon S3 Compatibility API](https://docs.oracle.com/en-us/iaas/Content/Object/Tasks/s3compatibleapi.htm)
- [Oracle: S3 Compatibility API 지원 범위](https://docs.oracle.com/en-us/iaas/Content/Object/Tasks/s3compatibleapi_topic-Amazon_S3_Compatibility_API_Support.htm)
