# codex-app-server-daemon

> `codex-app-server-daemon` is experimental and its lifecycle contract may
> change while the remote-management flow is still being developed.

`codex-app-server-daemon` backs the machine-readable `codex app-server`
lifecycle commands used by remote clients such as the desktop and mobile apps.
It is intended for Codex instances launched over SSH, including fresh developer
machines that should expose app-server with `remote_control` enabled.

## Platform support

The current daemon implementation is Unix-only. It uses pidfile-backed
daemonization plus Unix process and file-locking primitives, and does not yet
support Windows lifecycle management.

## Commands

```sh
codex app-server daemon start
codex app-server daemon restart
codex app-server daemon enable-remote-control
codex app-server daemon disable-remote-control
codex app-server daemon stop
codex app-server daemon version
codex app-server daemon bootstrap --remote-control
```

On success, every command writes exactly one JSON object to stdout. Consumers
should parse that JSON rather than relying on human-readable text. Lifecycle
responses report the resolved backend, socket path, local CLI version, and
running app-server version when applicable.

## Bootstrap flow

For a new remote machine:

```sh
npm install -g @mmmbuto/codex-vl@latest
codex-vl app-server daemon bootstrap --remote-control
```

In the codex-vl fork, npm/bun-managed installs bootstrap the daemon with the
currently running fork binary. The fork does not fetch or run the upstream
standalone installer. `bootstrap` records the daemon settings under
`CODEX_HOME/app-server-daemon/`, starts app-server as a pidfile-backed detached
process, and reports standalone auto-update as disabled for npm/bun installs.

## Installation and update cases

In this fork, the daemon supports npm/bun-managed launches and legacy
standalone layouts. npm/bun launches resolve to the currently running fork
binary. Standalone layouts still resolve through the managed binary path under
`CODEX_HOME`, but the fork does not fetch upstream standalone installers.

| Situation | What starts | Does this daemon fetch new binaries? | Does a running app-server eventually move to a newer binary on its own? |
| --- | --- | --- | --- |
| npm or bun shim launches the daemon | The pidfile backend uses the currently running `codex-vl` binary | No | No. Update with npm/bun, then restart the daemon to use the newer binary. |
| Standalone layout exists, but only `start` is used | `start` uses `CODEX_HOME/packages/standalone/current/codex` | No | No. The managed path is used when starting or restarting, but no updater is installed. |
| Standalone layout exists, then `bootstrap` is used | The pidfile backend uses `CODEX_HOME/packages/standalone/current/codex` | No. The fork disables standalone auto-update until a fork-owned installer exists. | No automatic binary replacement. Update through a fork-owned channel, then restart the daemon. |
| Some other tool updates the managed binary path | The next fresh start or restart uses the updated file at that path | No | A currently running app-server remains on the old executable image until an explicit `restart`. |

### npm/bun installs

For installs launched through the `codex-vl` npm/bun shim:

- lifecycle commands use the currently running fork binary
- `bootstrap` is supported
- `bootstrap` reports standalone auto-update as disabled
- after `npm install -g @mmmbuto/codex-vl@latest` or `@next`, restart the daemon
  to use the newly installed binary

### Standalone installs

For legacy standalone layouts:

- lifecycle commands always use the standalone managed binary path
- `bootstrap` is supported
- standalone auto-update is disabled in this fork until a fork-owned installer
  exists
- the updater loop is not reboot-persistent; it must be started again by
  rerunning `bootstrap` after a reboot

### Out-of-band updates

This daemon does not watch arbitrary executable files for replacement. If some
other tool updates the managed binary path:

- without `bootstrap`, a currently running app-server remains on the old
  executable image until an explicit `restart`
- with `bootstrap`, this fork does not run an installer or replace the binary
  automatically; restart explicitly after updating the fork package

## Lifecycle semantics

`start` is idempotent and returns after app-server is ready to answer the normal
JSON-RPC initialize handshake on the Unix control socket.

`restart` stops any managed daemon and starts it again.

`enable-remote-control` and `disable-remote-control` persist the launch setting
for future starts. If a managed app-server is already running, they restart it
so the new setting takes effect immediately.

Top-level `codex remote-control` bootstraps with `--remote-control` when the
updater loop is not running. Otherwise it enables remote control and starts the
daemon normally.

`stop` sends a graceful termination request first, then sends a second
termination signal after the grace window if the process is still alive.

All mutating lifecycle commands are serialized per `CODEX_HOME`, so a concurrent
`start`, `restart`, `enable-remote-control`, `disable-remote-control`, `stop`,
or `bootstrap` does not race another in-flight lifecycle operation.

## State

The daemon stores its local state under `CODEX_HOME/app-server-daemon/`:

- `settings.json` for persisted launch settings
- `app-server.pid` for the app-server process record
- `app-server-updater.pid` for the pid-backed standalone updater loop
- `daemon.lock` for daemon-wide lifecycle serialization
