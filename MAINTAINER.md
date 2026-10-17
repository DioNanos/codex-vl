# Maintainer

Codex VL is maintained by **Davide A. Guglielmi** (GitHub: [DioNanos](https://github.com/DioNanos)).

Codex VL is a fork of [OpenAI Codex](https://github.com/openai/codex) that adds
the Vivling companion system, the `/loop` workflow layer, and side-by-side npm
packaging under `@mmmbuto/codex-vl`. The fork is upstream-first: rebases against
`openai/codex` are routine and the VL layer is intentionally narrow.

## Scope of maintenance

In scope:

- the VL workflow layer (`/vivling`, `/vl`, `/loop`, `/goal`)
- the `@mmmbuto/codex-vl` npm packages (Linux x86_64-musl, Android arm64-musl,
  macOS arm64 source-build)
- integration points kept maintainable for routine upstream merges
- documentation in [`docs/`](./docs) and the dev journal at
  [dev.mmmbuto.com](https://dev.mmmbuto.com)

Out of scope here:

- changes that belong upstream — please file those on
  [openai/codex](https://github.com/openai/codex) directly
- features unrelated to the VL workflow layer

## Reporting

| Channel              | Where                                                     |
| -------------------- | --------------------------------------------------------- |
| Issues, PRs, ideas   | [DioNanos/codex-vl](https://github.com/DioNanos/codex-vl) |
| Security disclosures | [`SECURITY.md`](./SECURITY.md) — `security@mmmbuto.com`   |
| General contact      | `dev@mmmbuto.com`                                         |

When reporting a bug, please include: Codex VL version (`codex-vl --version`),
platform target (Linux musl / Android arm64 / macOS arm64), and a minimal
reproduction.

## Release flow

1. Work lands on `develop`.
2. Validated npm packages are published to the `latest` channel under
   `@mmmbuto/codex-vl`.
3. Tested commits are promoted to clean GitHub `main`.
4. GitHub Releases are cut from `main`.

`main` push to GitHub is gated and only happens after explicit approval.

### CI overlay on `main`

GitHub `main` carries a small set of CI changes that `develop` does not:
upstream jobs that need infrastructure this project does not have (Bazel,
private runner groups, paid macOS runners) are switched off with an explanatory
comment. They live in `scripts/fork-overlay/` and are reapplied on every
promotion:

1. Prepare the clean tree for `main`.
2. Make sure PyYAML is available, for example
   `python3 -m venv ~/.venvs/fork-overlay && ~/.venvs/fork-overlay/bin/pip install pyyaml==6.0.2` and
   `export PYTHON=~/.venvs/fork-overlay/bin/python` (CI uses the same version through
   `uv`). Then run `scripts/fork-overlay/fork-ci-overlay.sh` from the repository
   root. It checks the dependency before writing anything, applies the patch,
   does nothing if it is already applied, and fails if the patch no longer
   fits.
3. The script ends with `check_fork_ci_overlay.py`, which must report
   `INTACT`. It also runs the result steps in an isolated shell: they must
   pass when the disabled jobs are `skipped` and fail when one of them fails. Do not push to `main` otherwise.
4. After an upstream merge changes the workflows, regenerate the patch with
   `git diff <develop> <main> -- .github/workflows` in a dedicated change.

## Identity

- Profile: [github.com/DioNanos](https://github.com/DioNanos)
- Project hub: [mmmbuto.com](https://mmmbuto.com)
- Maintainer page and dev journal: [dev.mmmbuto.com](https://dev.mmmbuto.com)
- Deep-dive on the Vivling layer: [dev.mmmbuto.com/vivling](https://dev.mmmbuto.com/vivling)

## License

Codex VL is distributed under the Apache License 2.0 inherited from
[OpenAI Codex](https://github.com/openai/codex). Original Codex VL additions
(Vivling, `/loop`, `/vl`, `/goal`) are released under the same license.
See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE).

---

_Per aspera ad astra._
