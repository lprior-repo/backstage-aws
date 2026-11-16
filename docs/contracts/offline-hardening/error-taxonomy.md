# Error taxonomy

Command execution uses the machine protocol implemented by `error.rs` and the CLI: one JSON stdout result, progress/child logs on stderr, and no secret values. Help/version retain Clap's text output. Failures are not successful partial results.

| Failure | Stable code | Exit | `retryable()` |
|---|---|---:|---|
| Invalid local input or CLI contract | `INVALID_INPUT` | 2 | no |
| Remote mutation not approved | `APPROVAL_REQUIRED` | 3 | no |
| Invalid transition or unmet precondition | `PRECONDITION_FAILED` | 3 | no |
| Setup requires a human action | `SETUP_BLOCKED` | 3 | no |
| Child tool, process, filesystem, or stream failure | `TOOL_FAILED` | 4 | no |
| AWS request failure | `AWS_FAILED` | 4 | no |
| GitHub API failure | `GITHUB_FAILED` | 4 | no |
| Invalid persisted checkpoint or metadata | `STATE_FAILED` | 4 | no |
| Observed failed database task | `DB_TASK_FAILED` | 4 | no |
| Remote operation deadline; operation may continue remotely | `REMOTE_TIMEOUT` | 5 | yes |

These mappings are the current `Error::code`, `Error::exit_code`, and `Error::retryable` behavior in `crates/backstage_aws/src/error.rs`. Local filesystem and stream errors are `TOOL_FAILED`, not invalid user input. Process failures use the same `TOOL_FAILED` class; there are no separate process error variants. Callers that deliberately inspect a child's nonzero status, such as release validation, do so explicitly.

Malformed persisted schema, UUID, version ID, or key is a state failure. CAS conflict is a concurrency observation: reload, then resume the current valid operation or return a semantic conflict; never silently overwrite. Recovery context may include operation/attempt identity and task ARN, never passwords or secret response bodies. A timeout is uncertain execution, not evidence that a new attempt is safe.
