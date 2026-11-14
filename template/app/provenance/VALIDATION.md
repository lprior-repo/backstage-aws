> This record describes the application baseline before the runtime authentication and database configuration changes. It is not validation evidence for the current source.

# Application validation record

The baseline was checked on 2026-11-13 in Linux x64 with Node 24.22.1 and the checked-in Yarn 4.13.0 release. Direct Backstage package versions were pinned from the official 1.56.5 release manifest recorded in this directory.

## Historical checks

The baseline dependency installation, TypeScript compilation, application unit tests, production frontend/backend bundle, and production-dependency installation completed successfully. The baseline lockfile included the package integrity checksums and was unchanged by the later immutable-install check.

The baseline production bundle was built without runtime credentials. Its runtime configuration used PostgreSQL, GitHub OAuth, and session secrets. Docker and AWS were unavailable for that validation; no live sign-in, database connection, or load balancer health check was exercised.

## Current change status

The historical results above do not establish that the current configuration, provider selection, permission tests, or container image build pass. The current changes have not been typechecked, tested, or built as part of this record. No AWS resources or GitHub changes were made.
