# Managing Backstage plugins

The project-local `plugin` commands work on `<project>/app/` and do not need AWS settings or Terraform. Start with the read-only catalog; see the [CLI reference](../reference/cli.md#project-local-commands) for all flags and the [local development guide](local-development.md) to run the app.

## Catalog plugins

List the supported catalog IDs and installed packages:

```sh
backstage-aws plugin list
```

Add one or more IDs from this catalog:

| ID | Package(s) | Notes |
|---|---|---|
| `signals` | Frontend `@backstage/plugin-signals`; backend `@backstage/plugin-signals-backend` | Adds the signal API provider and backend service. |
| `notifications` | Frontend `@backstage/plugin-notifications`; backend `@backstage/plugin-notifications-backend` | Uses the signals API; automatically adds both `signals` packages. |
| `api-docs` | Frontend `@backstage/plugin-api-docs` | API documentation UI. |
| `catalog-graph` | Frontend `@backstage/plugin-catalog-graph` | Catalog graph UI. |
| `catalog-github` | Backend `@backstage/plugin-catalog-backend-module-github` | Provide the GitHub organization with `--param org=ORG`. |
| `scaffolder-github` | Backend `@backstage/plugin-scaffolder-backend-module-github` | GitHub actions for scaffolder. |

Example: add notifications; the catalog installer also adds its signals dependency. `--install` refreshes the Yarn lockfile:

```sh
backstage-aws plugin add notifications --install
```

The notifications frontend uses `plugin.signal.service`. If its provider is absent, the app can report `No API factory for plugin.signal.service`. The `notifications` catalog entry automatically adds the signals frontend plugin and backend service that it needs.

For a GitHub catalog module:

```sh
backstage-aws plugin add catalog-github --param org=ACME --install
```

Package versions are selected from the Backstage release pinned in the app's `backstage.json` and provenance manifest. A package absent from that release must use an explicit exact `PKG@VERSION`.

## Add another package

For a package not in the built-in catalog, specify which app package owns it and give an explicit exact version if the package is not in the pinned release:

```sh
backstage-aws plugin add \
  --frontend @acme/backstage-plugin-portal@1.2.3 \
  --backend @acme/plugin-portal-backend@1.2.3 \
  --install
```

Use `--frontend` for a frontend dependency, `--backend` for a backend dependency, or both. The CLI edits the local dependency manifests and any requested configuration/registration. `--set key=value` supplies a configuration entry; `--param key=value` supplies a catalog plugin parameter such as `org`. Run without `--install` to edit app files without refreshing `yarn.lock`; `--install` runs the package install and requires access to the package registry.

Preview planned edits without writing them:

```sh
backstage-aws plugin add catalog-github --param org=ACME --dry-run
```

## Remove and repeat

`plugin remove` accepts the same catalog IDs and package options as `plugin add`. It removes edits managed by `plugin add`; it does not remove unrelated packages. Review first with `plugin list`, then remove, optionally asking it to refresh the lockfile with `--install`:

```sh
backstage-aws plugin remove catalog-github --param org=ACME --dry-run
backstage-aws plugin remove catalog-github --param org=ACME --install
```

Both add and remove are safe to repeat. A refused change leaves the app files untouched. They edit local files only; they do not deploy the app.

## Human and agent use

Humans can run commands directly from the generated project root:

```sh
backstage-aws plugin list
backstage-aws plugin add notifications --dry-run
backstage-aws plugin add notifications --install
```

For agents and scripts, capture stdout separately from stderr, check the exit status, parse stdout as one JSON object, and stop on a nonzero exit code or an `ok: false` result. Do not treat progress lines as results. Exit codes: `0` success, `2` invalid input, `3` blocked precondition or approval required, `4` tool/API failure, `5` remote work continues after a timeout. Full contract: [CLI output and errors](../reference/cli.md#output-and-exit-codes).
