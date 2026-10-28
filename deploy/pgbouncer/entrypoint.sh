#!/bin/sh
# Renders pgbouncer.ini + userlist into the tmpfs at start: the image stays
# read-only and passwords never land in an image layer.
set -eu
: "${PGBOUNCER_DB:?}" "${POSTGRES_HOST:=postgres}" "${POSTGRES_PORT:=5432}"
: "${API_DB_PASSWORD:?}" "${WORKER_DB_PASSWORD:?}"
# Probes use the same environment as userlist generation. Never print psql's
# raw stderr (it can contain names/values from server configuration).
check_path() {
  path=$1
  case "$path" in
    direct) host=$POSTGRES_HOST; port=$POSTGRES_PORT ;;
    pooled) host=127.0.0.1; port=6432 ;;
  esac
  for role in audeniq_api audeniq_worker; do
    case "$role" in
      audeniq_api) password=$API_DB_PASSWORD ;;
      audeniq_worker) password=$WORKER_DB_PASSWORD ;;
    esac
    if detail=$(LC_ALL=C PGPASSWORD="$password" PGCONNECT_TIMEOUT=2 \
      timeout 4 \
      psql -X -w -h "$host" -p "$port" -U "$role" -d "$PGBOUNCER_DB" \
      -v ON_ERROR_STOP=1 -Atqc 'SELECT 1' 2>&1); then
      [ "$detail" = 1 ] || { echo "db_check path=$path role=$role kind=configuration"; return 21; }
    else
      case "$detail" in
        *"password authentication failed"*|*"SASL authentication failed"*|*"no pg_hba.conf entry"*|*"auth failed"*|*"not permitted to log in"*) kind=auth; code=20 ;;
        *"does not exist"*|*"no such database"*|*"no such user"*) kind=configuration; code=21 ;;
        *"server_login_retry"*) kind=backend_login_retry; code=1 ;;
        *) kind=unreachable_or_timeout; code=1 ;;
      esac
      echo "db_check path=$path role=$role kind=$kind"
      return "$code"
    fi
  done
  echo "db_check path=$path kind=ready"
}
case "${1:-}" in
  --check-direct) check_path direct; exit $? ;;
  --check-pooled) check_path pooled; exit $? ;;
  --healthcheck) check_path direct; check_path pooled; exit $? ;;
esac
dir=/run/pgbouncer
mkdir -p "$dir"
umask 077
quote() { printf '"%s"' "$(printf '%s' "$1" | sed 's/"/""/g')"; }
{
  printf '%s %s\n' "$(quote audeniq_api)" "$(quote "$API_DB_PASSWORD")"
  printf '%s %s\n' "$(quote audeniq_worker)" "$(quote "$WORKER_DB_PASSWORD")"
} > "$dir/userlist.txt"
cat > "$dir/pgbouncer.ini" <<INI
[databases]
${PGBOUNCER_DB} = host=${POSTGRES_HOST} port=${POSTGRES_PORT} dbname=${PGBOUNCER_DB}

[pgbouncer]
listen_addr = 0.0.0.0
listen_port = 6432
unix_socket_dir =
auth_type = scram-sha-256
auth_file = ${dir}/userlist.txt
pool_mode = transaction
; Retry backend login after transient outages; this does not repair bad auth.
server_login_retry = 2
server_connect_timeout = 3
; Read-only console inspection, no admin commands.
stats_users = audeniq_api,audeniq_worker
; sqlx prepares every statement; PgBouncer tracks them per server connection.
max_prepared_statements = ${PGBOUNCER_MAX_PREPARED:-1000}
max_client_conn = ${PGBOUNCER_MAX_CLIENT_CONN:-200}
default_pool_size = ${PGBOUNCER_POOL_SIZE:-10}
min_pool_size = 0
reserve_pool_size = 2
reserve_pool_timeout = 3
max_db_connections = ${PGBOUNCER_MAX_DB_CONN:-30}
server_idle_timeout = 300
server_lifetime = 3600
query_wait_timeout = 30
; sqlx sends extra_float_digits at startup; PgBouncer does not track it.
ignore_startup_parameters = extra_float_digits
log_connections = 0
log_disconnections = 0
stats_period = 300
logfile =
pidfile =
INI
exec pgbouncer "$dir/pgbouncer.ini"
