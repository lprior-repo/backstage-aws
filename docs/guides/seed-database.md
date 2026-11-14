# Seed database

`seed` is the shared template cloned only when a namespace is first created. Replacing it never changes existing preview or production databases.

## Prepare a custom-format dump

```sh
pg_dump --format=custom --no-owner --no-acl --file=backstage.dump "$SOURCE_DATABASE_URL"
```

`db dump` produces the same custom format. The Fargate restore task uses PostgreSQL 16 tooling; verify source/destination versions and extensions. Dumps may contain sensitive data: sanitize, encrypt and restrict access; never commit or attach them publicly.

## Replace the shared seed

Run from the generated project root with the intended account settings loaded and short-lived AWS credentials:

```sh
set -a; . environments/preview.env; set +a
backstage-aws seed --file ./backstage.dump --approve
```

For production, load `environments/production.env` and independently confirm the account. The CLI uploads the file as `seed.dump` to the seed bucket and restores it into `seed`; this replaces the seed contents.

## Export a namespace

`db dump` selects the environment namespace by default or `pr-<number>` with the PR selector. It runs `pg_dump` inside the VPC and stores `backups/<namespace>/<timestamp>.dump` in the seed bucket. Optional `--file` downloads to a new owner-readable local file; an existing path is not overwritten.

```sh
backstage-aws db dump --pull-request 42 --file ./pr-42.dump --approve
```

The bucket is encrypted and versioned; noncurrent objects expire after 30 days. Exports are on-demand, not continuous. Secure local copies as sensitive data.

## Limits

`seed --file` only refreshes shared `seed`; it neither restores nor overwrites an existing namespace. There is no CLI import-to-namespace command. Namespace recovery requires an operator-managed `pg_restore` path inside the VPC to a verified target, with traffic stopped and data validated before any cutover. See [Back up and restore](backup-and-restore.md).
