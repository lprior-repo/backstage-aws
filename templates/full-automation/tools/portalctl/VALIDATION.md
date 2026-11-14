> Historical baseline evidence from the original source archive. It does not establish validation of the changed automation bundle. See docs/VALIDATION.md at the repository root for current results.

# portalctl validation

Validated locally on **2026-11-13**, Linux x86_64. This record covers the optional
Rust CLI, not the Backstage application, Terraform, or a deployed AWS environment.

## Toolchain and dependencies

The pinned `1.100.0` stable toolchain was installed from the official Rust
distribution at `https://static.rust-lang.org/` with the minimal profile plus
Clippy and rustfmt. See the
[Rust 1.100.0 release announcement](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/).

| Tool | Observed version |
| --- | --- |
| rustc | `1.100.0 (0ef1bc386 2026-11-09)` |
| Cargo | `1.100.0 (3acc79c0c 2026-11-05)` |
| Clippy | `0.1.100 (0ef1bc3860 2026-11-09)` |
| rustfmt | `1.11.0-stable (0ef1bc3860 2026-11-09)` |

`cargo tree --edges normal` lists only `portalctl v0.1.0`: the CLI has **zero
external crate dependencies** and uses the standard library. Its AWS operations
invoke the separately installed AWS CLI through argument arrays, without a shell.

## Executed gates

Run these commands from `tools/portalctl` with the pinned Rust toolchain on PATH:

```sh
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --bins --examples --all-features -- \
  -D warnings -D unsafe_code \
  -D clippy::unwrap_used -D clippy::expect_used \
  -D clippy::panic -D clippy::panic_in_result_fn \
  -D clippy::todo -D clippy::unimplemented -D clippy::dbg_macro \
  -D clippy::indexing_slicing -D clippy::string_slice \
  -D clippy::get_unwrap -D clippy::arithmetic_side_effects \
  -D clippy::as_conversions -D clippy::let_underscore_must_use \
  -D clippy::await_holding_lock
cargo test --all-features
cargo build --release --locked
```

All five commands passed. The test run passed **6 integration tests**, with no
failures or ignored tests. Tests cover help without credentials; rejection of
invalid region/ARN/request/argument counts before spawning an external command;
oversized and non-UTF-8 arguments; stable retry identity and fixed request data;
and propagation of child-process failure. Controlled local `echo`/`false`
executables stand in for the AWS CLI, so these tests make no AWS calls. Three
tests use Unix-specific facilities and are conditionally compiled.

The production Clippy gate intentionally selects binaries and examples. Test
assertions and fixture setup use ordinary test-only `assert!`/`expect`; those are
not presented as passing the production prohibition on panics. A production
source scan found no `unsafe`, `unwrap`, `expect`, explicit panic/assertion,
`todo`, `unimplemented`, `unreachable`, or `dbg` invocation.

The release executable was **392,832 bytes** on the validated Linux x86_64 host.
This is a build observation, not a performance benchmark or a portability claim.
The deliverable includes source so operators can build for their own platform.

## Reviewed bounds and limitations

- The CLI retains at most four arguments, each at most 512 encoded bytes, and
  rejects a fifth argument. Rust/OS argument materialization can allocate before
  these application checks; this is not a whole-process allocation proof.
- Request IDs are 1–64 ASCII letters, digits, underscores, or hyphens. Region,
  account ID, state-machine/execution ARN shape, and name lengths are checked
  before invoking AWS. This validates syntax, not account ownership or IAM rights.
- Child processing uses at most 1,200 polls with a 100 ms interval, then kills and
  reaps the direct child. The nominal limit is 120 seconds; OS scheduling and
  cleanup can extend elapsed time. This is not a hard real-time deadline or a
  process-tree sandbox. An AWS credential helper may spawn its own descendants.
- AWS CLI connect/read timeouts are 10/60 seconds, with three configured standard
  retry attempts. A timeout or interrupted response does not prove that AWS
  rejected the request. Inspect execution state and reuse the same request ID
  when reconciling the same intended rollout.
- No AWS credentials, live service calls, deployment, or full `doctor` run were
  used for this validation. `doctor` checks tool responsiveness, not authentication.
- No async runtime, concurrency framework, third-party Rust crate, or unsafe Rust
  is present. The source forbids unsafe code.

Only rustc, Cargo, Clippy, rustfmt, and the standard library component were
installed. Advanced gates such as Miri, Kani, cargo-audit/cargo-deny, fuzzing,
coverage tools, sanitizers, assembly inspection, and profiling/benchmarks were
**not installed or not run**. No formal verification, full Power of Ten or
NASA/JPL compliance, security certification, or performance certification is
claimed.
