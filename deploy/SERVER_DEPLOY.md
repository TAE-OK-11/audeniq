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
