# 운영 서버에서 새 버전 올리기 (복사해서 붙여 넣기)

운영 서버에 SSH로 들어가 아래를 **그대로** 붙여 넣는다. 저장소와 GHCR 이미지가 공개라 토큰은 필요 없다.
자세한 배포 구조는 [PRODUCTION.md](PRODUCTION.md) "GHCR 이미지 배포".

## 1. 새 버전 배포

```bash
cd /opt/audeniq
(
set -e
IMG=ghcr.io/tae-ok-11/audeniq:main
docker pull -q "$IMG"
REV=$(docker image inspect -f '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$IMG")
echo "배포할 커밋: $REV"
mkdir -p pgbouncer
for f in compose.production.yaml bootstrap.sql grants.sql deploy.sh check-cpu.sh pg-upgrade.sh \
         backup.sh restore-backup.sh archive.sh update.sh pgbouncer/Dockerfile pgbouncer/entrypoint.sh; do
  curl -fsSL -o "$f" "https://raw.githubusercontent.com/TAE-OK-11/audeniq/$REV/deploy/$f" \
    || [ "$f" = update.sh ]
done
chmod +x deploy.sh check-cpu.sh pg-upgrade.sh backup.sh restore-backup.sh archive.sh pgbouncer/entrypoint.sh
[ -f update.sh ] && chmod +x update.sh
./deploy.sh "$IMG"
)
```

- 최신 `main` 이미지를 받고 → 그 이미지를 만든 커밋의 배포 파일을 받고 → `deploy.sh`가 마이그레이션·grants·헬스체크까지 한다.
- 중간에 하나라도 실패하면 바로 멈춘다 (기존 버전은 그대로 돈다).
- CPU 최적화 이미지를 쓰면 `IMG`를 `ghcr.io/tae-ok-11/audeniq:main-x86-64-v3`처럼 바꾼다
  (`-x86-64-v3`, `-graviton4`, `-ampere-a4`, `-ampere-a1`).
- 특정 커밋으로: `IMG=ghcr.io/tae-ok-11/audeniq:sha-<커밋 전체 해시>`.
- 같은 일을 스크립트로: `./update.sh` (`--check`은 새 버전 확인만, `--force`는 같은 버전 재배포).

## 2. 반영 확인

```bash
cd /opt/audeniq
docker compose -p audeniq-production -f compose.production.yaml --env-file production.env exec -T postgres \
  psql -U audeniq_owner -d audeniq_prod -Atc "
  SELECT max(version) AS last_migration, bool_and(success) AS all_ok FROM _sqlx_migrations;"
./deploy.sh --status
```

- 첫 줄 `last_migration`이 저장소 `migrations/` 폴더의 마지막 번호와 같고 `all_ok`가 `t`면 DB까지 반영된 것이다.
- `--status`에서 `api`·`worker`가 `running`(api는 `healthy`)이어야 한다.

특정 테이블·권한을 확인하려면 (예: 0155·0156):

```bash
docker compose -p audeniq-production -f compose.production.yaml --env-file production.env exec -T postgres \
  psql -U audeniq_owner -d audeniq_prod -Atc "
  SELECT to_regclass('portal.artist_photos') IS NOT NULL AS photos,
         to_regclass('portal.artist_press_releases') IS NOT NULL AS press_releases,
         has_table_privilege('audeniq_api','portal.artist_press_releases','INSERT') AS api_can_write;"
```

## 3. 문제가 생기면

```bash
cd /opt/audeniq
./deploy.sh --rollback      # 직전 이미지로 (이미 적용된 DB 마이그레이션은 되돌리지 않는다)
docker compose -p audeniq-production -f compose.production.yaml --env-file production.env logs --tail 100 migrate api worker
```

## 4. 관리자 화면에서 CLI 기능 쓰기 (운영 도구 켜기, 한 번만)

`audeniq-admin` CLI로만 하던 일(직원 권한, 보호 아티스트, 파트너, 식별코드 범위, 지급·로열티, 정산금 추가)을
관리자 화면 **운영 도구**에서 하게 한다. 레벨 규칙은 [docs/STAFF_LEVELS.md](../docs/STAFF_LEVELS.md).
1번으로 이 기능이 들어간 버전(compose에 `AUDENIQ_STAFF_OPS_DATABASE_URL`이 있는 버전)을 먼저 배포한 뒤 아래를 그대로 붙여 넣는다.

```bash
cd /opt/audeniq
(
set -e
C="docker compose -p audeniq-production -f compose.production.yaml --env-file production.env"
# 1) 운영 도구 전용 DB 역할 (주소에 그대로 쓸 수 있게 영문·숫자 비밀번호)
PW=$(openssl rand -hex 24)
$C exec -T postgres psql -U audeniq_owner -d audeniq_prod -v ON_ERROR_STOP=1 -v pw="$PW" <<'SQL'
SELECT format('CREATE ROLE audeniq_staff_ops LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L', :'pw')
 WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_staff_ops') \gexec
SELECT format('ALTER ROLE audeniq_staff_ops PASSWORD %L', :'pw') \gexec
SQL
# 2) production.env에 주소 (있으면 바꾸고 없으면 붙인다)
URL="postgres://audeniq_staff_ops:$PW@postgres/audeniq_prod"
if grep -q '^AUDENIQ_STAFF_OPS_DATABASE_URL=' production.env; then
  sed -i "s#^AUDENIQ_STAFF_OPS_DATABASE_URL=.*#AUDENIQ_STAFF_OPS_DATABASE_URL=$URL#" production.env
else
  echo "AUDENIQ_STAFF_OPS_DATABASE_URL=$URL" >> production.env
fi
# 3) 권한 다시 적용 (역할이 있어야 운영 도구 권한 블록이 적용된다) → API를 새 설정으로 다시 띄운다
$C run --rm grants
$C up -d --no-deps api
echo "운영 도구 켰어요."
)
```

- 역할이 이미 있으면 비밀번호만 새로 바꾼다. 다시 실행해도 된다.
- `production.env`는 비밀 파일이다. 권한은 그대로 `chmod 600`.

**최고 관리자(레벨 4) 주기** — CLI에서만 줄 수 있다. 관리자 화면에서 정산금 추가·직원 권한·식별코드 범위·파트너 라이브를 쓸 사람:

```bash
cd /opt/audeniq && set -a && . ./production.env && set +a
docker exec -e DATABASE_URL="postgres://audeniq_owner:$POSTGRES_PASSWORD@postgres/audeniq_prod" audeniq-production-api-1 \
  audeniq-admin --operator 내이름 staff grant 관리자이메일@audeniq.com OWNER
```

그 계정으로 관리자 화면에 다시 로그인하면 **운영 도구** 메뉴(정산·지급 처리, 보호 아티스트, AI·개발자 앱, 파트너·식별코드, 직원 권한)가 열린다.
돈·직원 권한 같은 중요한 작업은 인증 앱 코드를 한 번 더 묻는다. 그러려면 운영 전에 `STAFF_MFA_REQUIRED=true`.

확인:

```bash
cd /opt/audeniq
docker compose -p audeniq-production -f compose.production.yaml --env-file production.env exec -T postgres \
  psql -U audeniq_owner -d audeniq_prod -Atc "
  SELECT rolname FROM pg_roles WHERE rolname='audeniq_staff_ops';
  SELECT has_function_privilege('audeniq_staff_ops','finance.staff_credit_balance(uuid,uuid,numeric,text,text)','EXECUTE');"
docker exec audeniq-production-api-1 printenv AUDENIQ_STAFF_OPS_DATABASE_URL | sed 's#:[^:@]*@#:***@#'
```

첫 줄 `audeniq_staff_ops`, 둘째 줄 `t`, 셋째 줄에 주소(비밀번호 가림)가 나오면 켜진 것이다.
관리자 화면에 "이 서버에서는 운영 도구가 아직 꺼져 있어요"가 보이면 주소가 API에 안 들어간 것이다 — 2)·3)을 다시 확인한다.
끄려면 `production.env`에서 `AUDENIQ_STAFF_OPS_DATABASE_URL=`을 비우고 `$C up -d --no-deps api`.

## 5. 병합 전 브랜치를 서버에서 빌드·배포 (예: Studio 서빙 구조 시험)

CI 이미지가 없는 커밋(main에 병합 전)은 서버에서 `build-on-server.sh`로 빌드해 배포한다. 백엔드 이미지와
Studio 화면(`studio/releases/<커밋>`)을 **같은 커밋**으로 함께 올린다. 처음 한 번 준비(저장소 clone, 엔진 토큰,
GHCR 로그인)는 `build-on-server.sh` 머리말을 따른다. 서버의 옛 스크립트는 main만 받으므로 브랜치의 새 스크립트로 실행한다:

```bash
B=claude/studio-serving-restructure-0gmhx6
git -C /opt/audeniq-src fetch --quiet origin "$B"
git -C /opt/audeniq-src show FETCH_HEAD:deploy/build-on-server.sh > /tmp/build-on-server.sh
bash /tmp/build-on-server.sh "$B"
```

- 이미지 빌드(release, 수십 분) → 같은 커밋의 배포 파일 복사(`studio-release.sh` 포함) → Studio 화면 빌드
  (`node:22-bookworm-slim` 컨테이너, 저장소는 건드리지 않음) → `studio-release.sh install` → `deploy.sh`.
- 화면 빌드를 빼려면 `SKIP_STUDIO=1`, 이미지만 만들려면 `BUILD_ONLY=1`.
- 지금의 Cloudflare Worker(`audeniq-studio`)는 `/api`만 넘기므로 이 배포로 사용자에게 바뀌는 건 없다.
  화면이 Worker 대신 백엔드에서 나가는 건 프록시 Worker로 도메인을 옮긴 뒤다 ([STUDIO_SERVING.md](STUDIO_SERVING.md) 1단계 전환 순서).

확인 (api 컨테이너 안에서 바로 — 서비스 비밀 없이 화면은 200, API는 403이어야 한다):

```bash
cd /opt/audeniq
./studio-release.sh status
C=audeniq-production-api-1
docker logs "$C" 2>&1 | grep -m1 'studio release loaded'
for p in / /releases/1 /assets/없는파일.js /api/me; do
  printf '%-22s ' "$p"; docker run --rm --network "container:$C" curlimages/curl:8.11.1 -s -o /dev/null -w '%{http_code}\n' "http://127.0.0.1:8080$p"
done   # 200, 200, 404, 403
```

되돌리기: 백엔드는 `./deploy.sh --rollback`, 화면만은 `./studio-release.sh rollback`.
