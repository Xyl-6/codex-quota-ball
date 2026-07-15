# Task 3 Report: Codex app-server JSON-RPC client

## Status

Implemented the stdio JSON-RPC client in `src/codex.rs`, exported it from
`src/lib.rs`, and added the deterministic fake server plus four protocol tests.

The client:

- launches `CommandSpec::codex()` as `codex app-server --listen stdio://`;
- sends `initialize`, waits for response ID 1, then sends `initialized`;
- sends only `account/rateLimits/read` for quota reads;
- ignores unrelated notifications/responses and matches request IDs;
- uses a caller-provided `Duration` with a channel-backed receive timeout;
- maps missing CLI, signed-out, timeout, process, protocol, and server failures;
- parses quota data through `crate::quota::parse_quota_response`;
- does not read credentials or log raw server responses; and
- kills and waits for the child process on drop.

## TDD Evidence

### RED

Command:

```text
cargo test --test codex_client_tests -- --test-threads=1
```

Exit code: `101`.

The test target compiled and ran four tests. The first failure was the intended
runtime skeleton failure:

```text
thread 'categorizes_signed_out_response' panicked at src/codex.rs:13:9:
not implemented: command construction
```

Result: `0 passed; 4 failed` (the later cases observed the expected poisoned
test mutex after the first runtime panic).

### Initial GREEN attempt and resolved brief correction

After replacing the skeleton with the brief's implementation, the focused run
showed that `"not logged in"` mapped to `ClientError::Server`. Root-cause
inspection found that the proposed predicate checked `"login"`, which is not a
substring of `"logged"`. The smallest semantics-preserving correction adds
`lower.contains("logged")` to the authentication classification.

### GREEN

Command:

```text
cargo test --test codex_client_tests -- --test-threads=1
```

Exit code: `0`.

```text
running 4 tests
test categorizes_signed_out_response ... ok
test initializes_ignores_notifications_and_reads_quota ... ok
test rejects_malformed_output_without_panicking ... ok
test times_out_and_reports_child_exit ... ok

test result: ok. 4 passed; 0 failed
```

## Final Verification

- `cargo fmt --check`: exit `0`.
- `git diff --check`: exit `0`.
- `cargo test -- --test-threads=1`: exit `0`; 14 integration tests passed
  (4 protocol, 4 config, 6 quota), with 0 failures; unit/doc targets also passed.

## Self-review

- Scope is limited to the four requested implementation/test files; Task 1/2
  behavior is unchanged.
- Concurrency uses only `std::thread` and `std::sync::mpsc`.
- Production code contains no `unwrap`, `expect`, panic, raw-response logging,
  or credential access.
- Malformed JSON and incompatible quota payloads become `Protocol` errors.
- The only correction to the brief's sample code is the signed-out substring
  fix described above.

## Concerns

None blocking. Authentication mapping is necessarily message-based because the
fixture and brief identify signed-out state through the server error message.

## Review-fix evidence (2026-07-15)

### Scope and root causes

- `Command::output()` waited forever for `codex --version`; the version probe now
  uses the supplied timeout, kills and waits for the probe on timeout, and falls
  back to `unknown version` without dropping version context from protocol errors.
- The timeout fixture previously left Bash owning the app-server process while a
  child `sleep` held stdout open. The tracked executable fixture is now launched
  directly and uses `exec sleep 2`, so `CodexClient` kills and reaps the blocker it
  owns and the stdout reader reaches EOF.
- A disconnected stdout channel discarded an already-available child status.
  The disconnect path now calls `try_wait` and includes `exit status: 7` when the
  fixture exits with code 7.
- `Instant::now() + Duration::MAX` panicked. Deadline construction now uses
  `checked_add` and returns `ClientError::Timeout` when the duration is not
  representable.

### RED

Focused suite before production changes:

```text
cargo test --test codex_client_tests -- --test-threads=1
```

Exit code: `101`. The bounded-probe regression failed after the old probe blocked
for about two seconds:

```text
test bounds_version_probe_and_reports_unknown_version ... FAILED
assertion failed: started.elapsed() < Duration::from_secs(1)
test result: FAILED. 0 passed; 7 failed; finished in 2.01s
```

The remaining tests in that run observed the expected poisoned environment lock,
so the other regressions were run individually:

```text
cargo test --test codex_client_tests reports_child_exit_status_when_stdout_closes -- --test-threads=1
```

Exit code: `101`:

```text
assertion failed: matches!(client.read_quota(), Err(ClientError::Process(message)) if
    message.contains("exit status: 7"))
test result: FAILED. 0 passed; 1 failed
```

```text
cargo test --test codex_client_tests duration_max_returns_an_error_without_panicking -- --test-threads=1
```

Exit code: `101`:

```text
panicked at library/std/src/time.rs:429:33:
overflow when adding duration to instant
test result: FAILED. 0 passed; 1 failed
```

### GREEN

```text
cargo test --test codex_client_tests -- --test-threads=1
```

Exit code: `0`:

```text
running 7 tests
test bounds_version_probe_and_reports_unknown_version ... ok
test categorizes_signed_out_response ... ok
test duration_max_returns_an_error_without_panicking ... ok
test initializes_ignores_notifications_and_reads_quota ... ok
test rejects_malformed_output_without_panicking ... ok
test reports_child_exit_status_when_stdout_closes ... ok
test times_out_and_reaps_the_blocking_process ... ok
test result: ok. 7 passed; 0 failed; finished in 0.17s
```

### Final verification

- `cargo test -- --test-threads=1`: exit `0`; 17 integration tests passed
  (7 protocol, 4 config, 6 quota), with 0 failures; unit/doc targets also passed.
- `cargo fmt --check`: exit `0`.
- `git diff --check`: exit `0`.
- `tests/fixtures/fake_codex.sh` is tracked as executable mode `100755` in the
  review-fix commit.

## Important-finding follow-up (2026-07-15)

### Root causes and scope

- The bounded version probe polled only the direct child, then synchronously
  drained stdout after that child exited. A descendant retaining the inherited
  stdout pipe therefore kept `read_to_string` blocked beyond the deadline. The
  probe now drains on a detached thread and receives its result only for the
  time remaining on the original probe deadline; it never joins that thread.
- JSON serialization, newline writing, and flushing happened on the request
  thread before response timeout accounting. A dedicated writer thread now owns
  stdin. Each request sends bytes plus a one-shot acknowledgment channel, and
  both write acknowledgment and response receipt share one request deadline.
- Timeout, malformed protocol, incompatible response, stdin failure, and stdout
  failure store a terminal client error. Later `read_quota` calls return that
  error before allocating an ID or touching either stream. `Drop` still kills
  and reaps the direct child, which also releases a writer blocked on its stdin
  when no descendant retains that pipe.
- The fixed small JSON-RPC messages cannot deterministically fill an OS pipe in
  the existing black-box fixture, so write-ack timeout coverage would require a
  production test hook or a protocol-changing large request. Neither was added.

### RED

Version descendant regression before production changes:

```text
cargo test --test codex_client_tests bounds_version_probe_when_descendant_holds_stdout -- --test-threads=1
```

Exit code: `101`.

```text
test bounds_version_probe_when_descendant_holds_stdout ... FAILED
assertion failed: started.elapsed() < Duration::from_secs(1)
test result: FAILED. 0 passed; 1 failed; finished in 2.01s
```

Terminal-reuse regression before production changes:

```text
cargo test --test codex_client_tests timeout_makes_client_terminal_and_reaps_the_blocking_process -- --test-threads=1
```

Exit code: `101`.

```text
test timeout_makes_client_terminal_and_reaps_the_blocking_process ... FAILED
assertion failed: reuse_started.elapsed() < Duration::from_millis(25)
test result: FAILED. 0 passed; 1 failed; finished in 0.11s
```

### GREEN

```text
cargo test --test codex_client_tests -- --test-threads=1
```

Exit code: `0`.

```text
running 8 tests
test bounds_version_probe_and_reports_unknown_version ... ok
test bounds_version_probe_when_descendant_holds_stdout ... ok
test categorizes_signed_out_response ... ok
test duration_max_returns_an_error_without_panicking ... ok
test initializes_ignores_notifications_and_reads_quota ... ok
test rejects_malformed_output_without_panicking ... ok
test reports_child_exit_status_when_stdout_closes ... ok
test timeout_makes_client_terminal_and_reaps_the_blocking_process ... ok
test result: ok. 8 passed; 0 failed; finished in 0.23s
```

### Final verification

- `cargo fmt --check`: exit `0`.
- `cargo test --test codex_client_tests -- --test-threads=1`: exit `0`;
  8 passed, 0 failed.
- `cargo test -- --test-threads=1`: exit `0`; 18 integration tests passed
  (8 protocol, 4 config, 6 quota), with 0 failures; unit/doc targets also
  passed.
- `git diff --check`: exit `0`.
