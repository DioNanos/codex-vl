# Codex-VL AI-Guided Test Suite

This suite is intentionally AI-guided. Do not turn it into a blind automation
runner. The validating AI should drive the same checks a human operator would
expect, inspect outputs, decide PASS/FAIL, and write a sanitized report.

## Non-Negotiable Manual Validation Rule

The AI doing the validation is part of the test. It must perform the checks
manually, one command or interaction at a time, inspect the actual output, and
record its own PASS/FAIL judgement for each surface.

Do not use shell runner scripts, bulk `sh` pipelines, generated TSV summaries,
or aggregate script exit codes as the source of truth for `test-report/`
validation. A helper command may be used only as raw evidence when the AI
independently inspects and explains the output it produced.

Do not run compile-time validation here. In particular, do not run `cargo test`,
Rust build commands, release builds, or any other command that turns a
post-install test report into a rebuild. Build and merge validation belongs to
the build phase; `test-report/` validates the installed package and the AI's
manual operating surface.

All reports produced from this guide must be safe to publish. Do not include
private hostnames, absolute local paths, tokens, usernames, internal service
addresses, raw environment dumps, or full command transcripts containing local
secrets. Use placeholders such as `<repo>`, `<tmp-workspace>`, and
`<platform-device>`.

## Required Order

1. Confirm repo and release state.
2. Check package and command surface.
3. Check tool surface visible to the AI.
4. Run installed runtime smoke tests manually.
5. Run manual TUI checks.
6. Verify read, write, and patch behavior.
7. Write the sanitized report.

Stop and report immediately if a core build, package install, or version check
fails. Non-core environment failures, such as DNS being unavailable during the
network smoke, should be recorded as blockers or environmental failures, not
hidden.

## 1. Repo And Release State

From the repo root:

```sh
git fetch --all --prune
git status --short --branch
git log --oneline -5
npm view @mmmbuto/codex-vl dist-tags --json
```

Expected evidence:

- current branch and tracking remote
- HEAD commit under test
- dirty/untracked state separated from release files
- package version and dist-tags

Report PASS only when the branch and package version match the intended release
candidate.

## 2. Package And Command Surface

Run these from the installed package, not only from the source tree:

```sh
codex-vl --version
codex-vl-exec --version
codex-vl --help
codex-vl exec --help
codex-vl review --help
codex-vl login --help
codex-vl logout --help
codex-vl resume --help
codex-vl fork --help
codex-vl mcp --help
codex-vl sandbox --help
codex-vl app-server generate-json-schema --help
codex-vl completion bash >/tmp/codex-vl-completion.bash
codex-vl login status
codex-vl mcp list
codex-vl features list
codex-vl debug prompt-input --help
```

Expected evidence:

- `codex-vl` and `codex-vl-exec` report the same release version
- help routing works for major subcommands
- login status is readable without exposing credentials
- configured MCP servers are listed with secrets redacted
- feature list includes the fork-relevant features

## 3. AI Tool Surface

The validating AI must inventory the tools it can actually use in-session.
Record only categories and tool names, not secrets or local configuration.

Minimum expected tool categories:

- shell command execution
- stdin/session continuation
- patch application
- loop management when available
- memory or MCP discovery when available

If a tool is expected but unavailable, record the fallback and why it was used.
Do not read private MCP state files directly unless the MCP tool is unavailable
or the task is explicitly an MCP debug audit.

## 4. Installed Runtime Smoke

Use the installed package. Run each check manually and inspect its output before
marking PASS/FAIL. Do not delegate this section to `qa/runtime-suite/*.sh` or
any other runner.

Use a temporary workspace. Do not run these in the repo root.

```sh
tmp="$(mktemp -d)"
printf 'seed\n' > "$tmp/seed.txt"
cd "$tmp"

codex-vl exec --skip-git-repo-check --ephemeral 'Reply with exactly: OK'
codex-vl-exec --sandbox workspace-write --skip-git-repo-check --json \
  'Print current directory and list files. Do not modify files.'
codex-vl-exec --sandbox workspace-write --skip-git-repo-check --json \
  'Create hello.txt with content hello-codex-vl, then read seed.txt and hello.txt back. Reply with only the two file contents.'
codex-vl-exec --sandbox workspace-write --skip-git-repo-check --json \
  'Run one network check with curl -I https://www.google.com and report the first HTTP status line only.'
```

Expected evidence:

- exact `OK` response from non-interactive exec
- model can read current directory and list files
- model can write a file in workspace-write mode
- model can read both existing and newly written files
- network smoke either returns an HTTP status line or a clear environment-level
  failure such as DNS unavailable

## 5. Manual TUI Checks

Launch an interactive TUI in a disposable or normal test thread:

```sh
codex-vl --dangerously-bypass-approvals-and-sandbox
```

Check:

- first launch renders version, model, cwd, permission mode, and Vivling
- `/loop` shows usage
- `/loop ls` lists configured loops or says none are configured
- `/goal` shows current goal or no-goal state
- `/goal <objective>` sets a goal and updates the status line
- interrupting a goal pauses it cleanly
- `/goal clear` clears it cleanly
- `/vivling` and `/vl` surfaces render without crashing
- resume/fork help is present; interactive resume/fork should be tested before
  release promotion when practical

Do not leave loop jobs or active goals behind after the test.

## 6. Read, Write, And Patch Behavior

Cover three different surfaces:

- Shell/read surface: use normal commands such as `pwd`, `ls`, and `cat` inside
  a temporary workspace.
- Runtime/write surface: ask `codex-vl-exec` to create a temporary file and read
  it back.
- Patch surface: use the agent's patch tool to add or update the report itself,
  then inspect `git diff --check` and `git status --short`.

PASS means file reads are correct, writes are scoped to temporary or intended
report files, and the patch creates reviewable git changes without whitespace
errors.

## 7. Report Format

Create one report per release candidate under `test-report/manual/` or
`test-report/automated/`. For AI-guided suites, prefer:

```text
test-report/manual/YYYY-MM-DD_<version>_ai_guided_surface.md
```

The report must include:

- version and commit under test
- sanitized environment summary
- explicit confirmation that validation was AI-manual, not runner-driven
- command surface results
- tool surface results
- runtime smoke results
- manual TUI results
- read/write/patch results
- failures, blockers, and residual risk
- final decision: PASS, PASS WITH ENVIRONMENTAL LIMITATION, or FAIL

Do not include:

- absolute local paths
- private remotes or hostnames
- tokens or account identifiers
- raw process lists with unrelated user sessions
- unredacted MCP environment variables

## Release Gate

A release candidate is acceptable only when:

- installed package version is correct
- core command surface works
- validation was performed manually by the AI and not accepted from a runner
- runtime read/write smoke passes
- manual TUI first launch, `/loop`, `/goal`, and Vivling checks are sane
- every failure is either fixed or explicitly classified as environmental
- report and guide changes are sanitized and ready for public GitHub mirroring
