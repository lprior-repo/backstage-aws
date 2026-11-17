#!/bin/sh
set -eu

: "${APP_ROLE:?APP_ROLE must be set}"
: "${APP_PASSWORD:?APP_PASSWORD must be supplied by Secrets Manager}"
: "${LIFECYCLE_ROLE:?LIFECYCLE_ROLE must be set}"
: "${LIFECYCLE_PASSWORD:?LIFECYCLE_PASSWORD must be supplied by Secrets Manager}"

# Create application role and seed database (unchanged).
psql --no-psqlrc --no-password --set=ON_ERROR_STOP=1 --set=app_role="$APP_ROLE" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN', :'app_role')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = :'app_role') \gexec
SELECT format('GRANT %I TO CURRENT_USER', :'app_role') \gexec
SELECT format('CREATE DATABASE %I OWNER %I', 'seed', :'app_role')
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'seed') \gexec
SQL
# psql encrypts before ALTER ROLE, so SQL errors cannot log plaintext.
printf '%s\n%s\n' "$APP_PASSWORD" "$APP_PASSWORD" |
  psql --no-psqlrc --no-password --quiet --set=ON_ERROR_STOP=1 \
    --set=app_role="$APP_ROLE" --command '\password :app_role'

# Create the dedicated lifecycle DB login: CREATEDB allows clone/drop of
# per-environment databases, but it is not a superuser and cannot create
# roles. It inherits app role membership so cloned databases have the
# correct owner, but the ADMIN OPTION is explicitly absent so lifecycle
# cannot alter the app role.
psql --no-psqlrc --no-password --set=ON_ERROR_STOP=1 \
  --set=app_role="$APP_ROLE" --set=lifecycle_role="$LIFECYCLE_ROLE" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN CREATEDB', :'lifecycle_role')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = :'lifecycle_role') \gexec
-- Lifecycle must inherit the app role: CREATE DATABASE ... TEMPLATE seed needs ownership
-- of the template, and DROP DATABASE needs ownership of cloned databases; both are checked
-- through inherited membership. PG16 copies the role's INHERIT setting into each new
-- membership, so revoke and re-grant to replace any edge created while NOINHERIT.
ALTER ROLE :lifecycle_role INHERIT;
REVOKE :app_role FROM :lifecycle_role;
GRANT :app_role TO :lifecycle_role;
-- RDS admins are not superusers, and Postgres rejects ALTER ROLE naming SUPERUSER,
-- REPLICATION or BYPASSRLS even when the value is unchanged. The CREATE above already
-- yields those attributes off, so verify them read-only and fail closed instead.
SELECT format('DO $$ BEGIN RAISE EXCEPTION %L; END $$', 'lifecycle role has elevated attributes')
FROM pg_roles
WHERE rolname = :'lifecycle_role' AND (rolsuper OR rolreplication OR rolbypassrls) \gexec
SQL
# Set the lifecycle login password (encrypted by psql before sending).
printf '%s\n%s\n' "$LIFECYCLE_PASSWORD" "$LIFECYCLE_PASSWORD" |
  psql --no-psqlrc --no-password --quiet --set=ON_ERROR_STOP=1 \
    --set=lifecycle_role="$LIFECYCLE_ROLE" --command '\password :lifecycle_role'
