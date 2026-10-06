# Continuous integration

This fork runs a reduced CI surface on GitHub Actions. What runs, what is
disabled, and why:

## What runs

- **blocking-ci** (pull requests and main): codespell, repo-checks,
  cargo-deny, the cargo gates in rust-ci (format, cargo shear, and the
  argument-comment lint package), and the blob size policy. Bazel
  does not run. The Python SDK installation job still runs and is not
  part of `CI required`.
- **postmerge-ci** (pushes to main): calls `rust-ci-full.yml` and
  `v8-canary.yml`. On this fork the cargo checks in rust-ci-full stay
  required. The Linux v8-canary legs run on GitHub-hosted runners.

## What is disabled, and why

Several inherited legs cannot run on this fork's GitHub account, so they
are disabled instead of showing permanent red. GitHub applies
`matrix: include:` entries after `matrix: exclude:` entries, so an
`exclude:` cannot remove an `include:` entry; the unrunnable matrix legs
are therefore commented out of the `include:` lists, while fully
inherited jobs are turned off with `if: false` at the job level. Every
disabled block carries a comment with the reason and how to re-enable it.
The cargo gates stay on.

Per workflow:

- **`bazel.yml`, test, clippy, and verify-release-build**: turned off
  with `if: false` on every platform. Bazel is upstream's build system
  and uses a remote cache with credentials this fork does not have. This
  fork publishes with cargo. Remove the `if` to restore a job. The macOS
  matrix legs stay commented out because they need paid runners, and the
  Windows legs stay commented out because they ask for the private runner
  group `<repo>-runners`. `CI required` does not list the Bazel workflow,
  so a skipped Bazel job cannot fail that gate.
- **Three inherited Windows Bazel jobs in `bazel.yml`**: turned off with
  `if: false` at the job level. They ask for `<repo>-runners`, which does
  not exist on this fork. Remove the `if` to restore a job.
- **`rust-ci.yml`, Bazel argument-comment lint**: the prebuilt job is
  turned off with `if: false`, for the same Bazel reason as above.
  Remove the `if` to restore it. `CI results` does not require that job.
  The cargo package job and the cargo general and shear checks stay
  strict. The macOS and Windows legs stay commented out.
- **`rust-ci-full.yml`** (called by postmerge-ci): the prebuilt
  argument-comment lint is turned off because it runs through Bazel.
  `lint_build` and every test leg are turned off with `if: false`: the
  Linux and Windows legs ask for `<repo>-runners`, and the macOS leg
  needs paid runners. The results gate accepts `skipped` for those jobs
  and still requires success from general, cargo shear, and the cargo
  argument-comment lint package. Remove the `if` to restore a job.
- **`v8-canary.yml`** (called by postmerge-ci): the Linux legs (x64 and
  arm64, release and ptrcomp-sandbox variants) run on GitHub-hosted
  runners. The four macOS legs are commented out of the `matrix: include:`
  list. The `build-windows-source` job keeps its upstream conditional
  gate, so it runs only when the metadata job asks for it.
- **`rust-release-argument-comment-lint.yml`**: this workflow builds the
  lint library with cargo, not Bazel, so it stays. The two Linux legs
  (x64 and arm64) run. The macOS leg and the Windows leg are commented
  out of the `matrix: include:` list.
- **`sdk.yml`**: the Bazel `sdks` job is turned off with `if: false`.
  It builds the CLI and the code-mode host with Bazel, which is off on
  this fork. Remove the `if` to restore it, and add `sdk` back to the
  `CI required` needs list in `blocking-ci.yml`. The Python installation
  job still runs and is not a merge gate.

## Known postmerge limitation

`postmerge-ci` grants `rust-ci-full` contents read and actions write, and
`v8-canary` contents read and actions read, so the callers can start.
`v8-canary` uses GitHub-hosted Linux runners. The `rust-ci-full` jobs that
need the private runner group, and the Bazel prebuilt lint, are disabled
as listed above.
