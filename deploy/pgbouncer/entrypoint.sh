#!/bin/sh
# Renders pgbouncer.ini + userlist into the tmpfs at start: the image stays
# read-only and passwords never land in an image layer.
set -eu
: "${PGBOUNCER_DB:?}" "${POSTGRES_HOST:=postgres}" "${POSTGRES_PORT:=5432}"
: "${API_DB_PASSWORD:?}" "${WORKER_DB_PASSWORD:?}"
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
; sqlx prepares every statement; PgBouncer tracks them per server connection.
max_prepared_statements = ${PGBOUNCER_MAX_PREPARED:-200}
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
