# Database readiness diagnosis (read-only)

Run in `/opt/audeniq`; do not paste secrets, connection URLs, environment dumps,
userlist contents or password hashes into a ticket. These commands inspect state
or run SELECT/SHOW only. They do not reconcile passwords or restart services.

1. Select the **same** Compose files and secret environment files as deploy.sh.
   Do not source production.env as shell code. For the standard TLS install:

   ```bash
   cd /opt/audeniq
   C=(docker compose -p audeniq-production -f compose.production.yaml --env-file production.env)
   # If SECRETS_ENV_FILE is configured, append its existing tmpfs path:
   # C+=(--env-file /existing/tmpfs/secret-file)
   [ ! -f compose.override.yaml ] || C+=(-f compose.override.yaml)
   # If TLS_BIND is configured:
   C+=(-f compose.tls.yaml)
   "${C[@]}" ps -a
   "${C[@]}" logs --since 10m --tail 100 postgres pgbouncer api migrate grants
   ```

   Find the **first** PgBouncer backend failure preceding server_login_retry.
   Refused/timeout/DNS suggests reachability; password/SCRAM/HBA/unknown role or
   database suggests configuration. Review logs locally; redact before sharing.
   EOF alone does not identify who closed the connection.

2. Test actual TCP authentication to PostgreSQL with both existing runtime secrets
   (pg_isready alone does not verify passwords). This works with the old images:

   ```bash
   "${C[@]}" exec -T postgres sh -eu -c '
     for role in audeniq_api audeniq_worker; do
       case "$role" in
         audeniq_api) PGPASSWORD=$API_DB_PASSWORD ;;
         audeniq_worker) PGPASSWORD=$WORKER_DB_PASSWORD ;;
       esac
       export PGPASSWORD PGCONNECT_TIMEOUT=3
       printf "direct role=%s\n" "$role"
       psql -X -w -h postgres -U "$role" -d audeniq_prod -v ON_ERROR_STOP=1 -Atqc "SELECT 1"
       printf "pooled role=%s\n" "$role"
       psql -X -w -h pgbouncer -p 6432 -U "$role" -d audeniq_prod -v ON_ERROR_STOP=1 -Atqc "SELECT 1"
     done'
   ```

   A direct failure isolates PostgreSQL/auth/network; direct success and pooled
   failure isolates the PgBouncer path. PostgreSQL container env can itself be
   stale: success here does not prove API/PgBouncer containers have the same env.
   Compare the resolved `API_DB_PASSWORD` / `WORKER_DB_PASSWORD` from the existing
   production.env + SECRETS_ENV_FILE mechanism with each container's effective
   environment **privately, without printing them**. Changing env on an existing
   volume never updates the PostgreSQL role password: bootstrap.sql is new-volume
   only; grants.sql does not rotate it. Check role existence/login/expiry and HBA
   locally (no password/hash queries):

   ```bash
   "${C[@]}" exec -T postgres psql -X -U audeniq_owner -d audeniq_prod -c \
     "SELECT rolname,rolcanlogin,rolvaliduntil FROM pg_roles WHERE rolname IN ('audeniq_api','audeniq_worker');"
   "${C[@]}" exec -T postgres psql -X -U audeniq_owner -d audeniq_prod -c \
     'SELECT line_number,type,database,user_name,address,auth_method,error FROM pg_hba_file_rules;'
   ```

3. With the reviewed PgBouncer image installed, test using **PgBouncer's own env**:

   ```bash
   "${C[@]}" exec -T pgbouncer /usr/local/bin/pgbouncer-entrypoint --check-direct
   "${C[@]}" exec -T pgbouncer /usr/local/bin/pgbouncer-entrypoint --check-pooled
   ```

   Both roles must pass. Exit 20 = explicit auth/HBA, 21 = role/database/config,
   1 = unavailable/timeout or cached backend login retry. Verify userlist matches
   that env without displaying it (quote escaping mirrors entrypoint.sh):

   ```bash
   "${C[@]}" exec -T pgbouncer sh -eu -c '
     quote() { printf "\"%s\"" "$(printf "%s" "$1" | sed '\''s/"/""/g'\'')"; }
     { printf "%s %s\n" "$(quote audeniq_api)" "$(quote "$API_DB_PASSWORD")"
       printf "%s %s\n" "$(quote audeniq_worker)" "$(quote "$WORKER_DB_PASSWORD")"
     } | cmp -s - /run/pgbouncer/userlist.txt
   ' && echo 'userlist matches container env'
   ```

   A match does not prove the live PostgreSQL role password matches: the direct
   TCP login is that check. No auth_query or forced backend user is configured.

4. Inspect pool state (new config grants read-only stats access to runtime roles):

   ```bash
   "${C[@]}" exec -T pgbouncer sh -eu -c '
     export PGPASSWORD="$API_DB_PASSWORD" PGCONNECT_TIMEOUT=3
     for query in "SHOW POOLS" "SHOW STATS" "SHOW SERVERS" "SHOW HELP"; do
       psql -X -w -h 127.0.0.1 -p 6432 -U audeniq_api -d pgbouncer -c "$query"
     done'
   ```

   Check waiting clients and missing server connections. Use `SHOW ERRORS` only
   if the installed version lists it in SHOW HELP; PgBouncer versions without it
   require logs. Old config has no stats_users; lack of console permission is
   not proof that application login fails. Do not enable admin access to diagnose.

5. Confirm API independently of CloudFront (audits found cached /ready responses):

   ```bash
   "${C[@]}" exec -T api audeniq-api healthcheck
   "${C[@]}" logs --since 5m api | grep -E 'database_readiness_failed|DATABASE_UNAVAILABLE'
   ```

   After the reviewed changes, logs distinguish auth, backend_login_retry,
   unreachable, timeout, configuration and query failures without raw DB errors.
   Both SELECT paths succeeding while /ready fails points to API env/pool or its
   adapter_profiles query/grants/schema; inspect migrate/grants exit states.
   Verify local /ready returns 200 repeatedly after recovery without restarting
   API. A controlled restart/recovery test belongs in staging, outside this
   read-only checklist: restart PostgreSQL/PgBouncer separately, require /ready
   503 during the outage and eventual 200 on the **same API container** afterward;
   /health must remain 200. Also verify wrong role credentials fail the deploy
   gate immediately and a transient startup delay succeeds within its budget.
