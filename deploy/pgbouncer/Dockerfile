# PgBouncer in front of PostgreSQL (transaction pooling). Built from Debian's
# own package (no third-party image): trixie ships PgBouncer 1.24, which
# supports protocol-level prepared statements in transaction mode (>= 1.21),
# needed because sqlx prepares every query.
# BASE is overridable only to test the image where Debian mirrors are
# unreachable; production builds use the default.
ARG BASE=debian:13-slim
FROM ${BASE}
RUN apt-get update \
    && apt-get install -y --no-install-recommends pgbouncer ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && pgbouncer --version
COPY entrypoint.sh /usr/local/bin/pgbouncer-entrypoint
RUN chmod 0755 /usr/local/bin/pgbouncer-entrypoint
USER postgres
EXPOSE 6432
ENTRYPOINT ["/usr/local/bin/pgbouncer-entrypoint"]
