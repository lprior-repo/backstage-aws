# Output and errors

| Stream | Contract |
|---|---|
| stdout | One newline-terminated JSON envelope per parsed command, success or failure. `--help`/`--version` are plain text. Parse errors use `command:"unknown"`. |
| stderr | Step JSONL and child-process output. Step events: `event:"step"`, `name`, `status` (`started`, `ok`, `failed`), `elapsed_ms`. |
| `$GITHUB_STEP_SUMMARY` | After a successful `env up`, append its step-timing Markdown table when set. |

```json
{"ok":true,"command":"env up","result":{}}
{"ok":false,"command":"env up","error":{"code":"APPROVAL_REQUIRED","message":"...","retryable":false}}
```

Success keys: `ok`, `command`, `result`; failure keys: `ok`, `command`, `error` (`code`, `message`, `retryable`). Result fields vary by command. Examples are protocol shapes, not live AWS/GitHub results.

Blocked `setup` adds `error.details`: ordered `checks`, one `next` action, `resume` argv and non-secret `environment`. Status `verified` is observed evidence; `action_required`, `blocked` and `not_checked` mean the stage is incomplete. Reruns rediscover state; no completion journal.

## Exit and error codes

| Exit | Code | Meaning | Retryable |
|---:|---|---|---:|
| 0 | — | Success | — |
| 2 | `INVALID_INPUT` | CLI syntax or invalid/missing setting | false |
| 3 | `APPROVAL_REQUIRED`, `PRECONDITION_FAILED`, `SETUP_BLOCKED` | Approval missing; blocked by account, identity, prerequisite or permission | false |
| 4 | `TOOL_FAILED`, `AWS_FAILED`, `GITHUB_FAILED`, `STATE_FAILED`, `DB_TASK_FAILED` | Tool, service/API, GitHub, local state/I/O, or database task failure | false |
| 5 | `REMOTE_TIMEOUT` | Remote operation continues after wait limit; error names operation and remote ID | true |

Exit 5 is not completion: inspect remote state before retrying. Local child-process timeout is exit 4. Preserve error JSON and stderr when reporting.

Remote AWS or GitHub mutations without `--approve` or `BACKSTAGE_AWS_APPROVE=true` stop before changing remote state and return exit 3 / `APPROVAL_REQUIRED`; commands never prompt. Local project changes do not require approval.
