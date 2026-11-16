#!/bin/sh
# No DDL or restore: an unavailable ECS record must never authorize a cutover.
set -eu
: "${SEED_OPERATION_ID:?SEED_OPERATION_ID must be set}"
: "${SEED_OBJECT_KEY:?SEED_OBJECT_KEY must be set}"
case "$SEED_OPERATION_ID" in
  *[!0123456789abcdef-]*|'') echo "invalid seed operation id" >&2; exit 2 ;;
esac
[ "${#SEED_OPERATION_ID}" -eq 36 ] || exit 2
[ "$SEED_OBJECT_KEY" = "operations/seed/$SEED_OPERATION_ID.dump" ] || exit 2
compact_id=$(printf '%s' "$SEED_OPERATION_ID" | tr -d '-')
receipt="backstage-aws-seed-v1:$SEED_OPERATION_ID:$SEED_OBJECT_KEY"
# Force read-only transactions even if the SQL is changed accidentally.
export PGOPTIONS="${PGOPTIONS:+$PGOPTIONS }-c default_transaction_read_only=on"
proven=$(psql --no-psqlrc --no-password --tuples-only --no-align --set=ON_ERROR_STOP=1 \
  --set=stage_db="seed_stage_$compact_id" --set=backup_db="seed_previous_$compact_id" \
  --set=receipt="$receipt" <<'SQL'
SELECT EXISTS (
  SELECT FROM pg_database live
  WHERE live.datname = 'seed'
    AND NOT EXISTS (SELECT FROM pg_database WHERE datname = :'stage_db')
    AND (
      shobj_description(live.oid, 'pg_database') = :'receipt'
      OR (
        shobj_description(live.oid, 'pg_database') IS NULL
        AND EXISTS (SELECT FROM pg_database WHERE datname = :'backup_db')
      )
    )
);
SQL
)
if [ "$proven" = t ]; then
  exit 0
fi
echo "seed outcome is unknown: no matching live receipt or unambiguous legacy completion; checkpoint retained; inspect seed/stage/previous databases and db-admin logs" >&2
exit 3
