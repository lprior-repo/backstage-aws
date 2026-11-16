#!/bin/sh
set -eu

: "${APP_ROLE:?APP_ROLE must be set}"
: "${SEED_OPERATION_ID:?SEED_OPERATION_ID must be set}"
: "${DUMP_PATH:?DUMP_PATH must be set}"
: "${SEED_OBJECT_KEY:?SEED_OBJECT_KEY must be set}"
[ "$SEED_OBJECT_KEY" = "operations/seed/$SEED_OPERATION_ID.dump" ] || {
  echo "seed input does not match its operation" >&2; exit 2;
}
case "$SEED_OPERATION_ID" in
  *[!0123456789abcdef-]*|'') echo "invalid seed operation id" >&2; exit 2 ;;
esac
[ "${#SEED_OPERATION_ID}" -eq 36 ] || { echo "invalid seed operation id" >&2; exit 2; }

compact_id=$(printf '%s' "$SEED_OPERATION_ID" | tr -d '-')
stage_db="seed_stage_${compact_id}"
backup_db="seed_previous_${compact_id}"
seed_receipt="backstage-aws-seed-v1:$SEED_OPERATION_ID:$SEED_OBJECT_KEY"
psql_admin() {
  psql --set=ON_ERROR_STOP=1 --set=app_role="$APP_ROLE" \
    --set=stage_db="$stage_db" --set=backup_db="$backup_db" \
    --set=seed_receipt="$seed_receipt" "$@"
}
database_exists() {
  psql --tuples-only --no-align --set=ON_ERROR_STOP=1 --set=lookup_db="$1" <<'SQL'
SELECT EXISTS (SELECT FROM pg_database WHERE datname = :'lookup_db');
SQL
}

stage_exists=$(database_exists "$stage_db")
backup_exists=$(database_exists "$backup_db")
seed_exists=$(database_exists seed)
if [ "$backup_exists" = t ] && [ "$seed_exists" = t ] && [ "$stage_exists" = f ]; then
  exit 0
fi
if [ "$backup_exists" = t ] && [ "$seed_exists" = f ] && [ "$stage_exists" = f ]; then
  echo "seed cutover is incomplete; previous seed remains at $backup_db" >&2
  exit 1
fi
if [ "$backup_exists" = t ] && [ "$seed_exists" = t ] && [ "$stage_exists" = t ]; then
  echo "seed cutover has conflicting databases; refusing to replace either" >&2
  exit 1
fi

if [ "$backup_exists" = f ] || [ "$seed_exists" = t ]; then
  if [ "$stage_exists" = t ]; then
    psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I IS_TEMPLATE false', :'stage_db') \gexec
SELECT format('DROP DATABASE IF EXISTS %I WITH (FORCE)', :'stage_db') \gexec
SQL
  fi
  psql_admin <<'SQL'
SELECT format('CREATE DATABASE %I OWNER %I', :'stage_db', :'app_role') \gexec
SQL
  pg_restore --single-transaction --no-owner --role="$APP_ROLE" \
    --host="$PGHOST" --port="$PGPORT" --username="$PGUSER" \
    --dbname="$stage_db" "$DUMP_PATH"
  # Database comments follow the database OID through rename, including when
  # the first seed has no previous database. Only the admin owns this receipt.
  psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I OWNER TO %I', :'stage_db', current_user) \gexec
SELECT format('COMMENT ON DATABASE %I IS %L', :'stage_db', :'seed_receipt') \gexec
SELECT format('ALTER DATABASE %I IS_TEMPLATE true', :'stage_db') \gexec
SQL

  # Keep one historical seed; the current live seed is untouched until restore
  # has completed and the new database is ready for cutover.
  psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I IS_TEMPLATE false', datname)
FROM pg_database
WHERE datname LIKE 'seed_previous_%' AND datname <> :'backup_db' \gexec
SELECT format('DROP DATABASE IF EXISTS %I WITH (FORCE)', datname)
FROM pg_database
WHERE datname LIKE 'seed_previous_%' AND datname <> :'backup_db' \gexec
SQL
  if [ "$seed_exists" = t ]; then
    if [ "$(database_exists seed)" != t ]; then
      echo "live seed database disappeared before cutover; refusing to continue" >&2
      exit 1
    fi
    psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I RENAME TO %I', 'seed', :'backup_db') \gexec
SQL
  fi
fi

if [ "$(database_exists "$stage_db")" != t ]; then
  echo "staging database is missing; refusing to report a successful restore" >&2
  exit 1
fi
if psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I RENAME TO %I', :'stage_db', 'seed') \gexec
SQL
then
  exit 0
fi
if [ "$seed_exists" = t ]; then
  psql_admin <<'SQL'
SELECT format('ALTER DATABASE %I RENAME TO %I', :'backup_db', 'seed') \gexec
SQL
fi
exit 1
