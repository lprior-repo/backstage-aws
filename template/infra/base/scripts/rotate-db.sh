#!/bin/sh
set -eu

: "${APP_ROLE:?APP_ROLE must be set}"
: "${APP_PASSWORD_FILE:?APP_PASSWORD_FILE must be mounted from the pinned secret version}"
password=$(cat "$APP_PASSWORD_FILE")
[ "${#password}" -eq 40 ] || { echo "invalid rotation secret value" >&2; exit 2; }
case "$password" in *[!A-Za-z0-9]*|'') echo "invalid rotation secret value" >&2; exit 2 ;; esac
# psql encrypts client-side; no superuser-only logging changes are required.
printf '%s\n%s\n' "$password" "$password" |
  psql --no-psqlrc --no-password --quiet --set=ON_ERROR_STOP=1 \
    --set=app_role="$APP_ROLE" --command '\password :app_role'
unset password
