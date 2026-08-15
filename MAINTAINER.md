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

| Channel | Where |
|---|---|
| Issues, PRs, ideas | [DioNanos/codex-vl](https://github.com/DioNanos/codex-vl) |
| Security disclosures | [`SECURITY.md`](./SECURITY.md) — `security@mmmbuto.com` |
| General contact | `dev@mmmbuto.com` |

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

*Per aspera ad astra.*
