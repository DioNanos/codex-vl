# Vivling Command Reference

This guide covers every `/vivling` subcommand, the sibling `/vl`, `/vla` and
`/loop` commands, and the keyboard shortcuts. For how a Vivling resolves its
brain model, see [Vivling brain model configuration](vivling_model_catalog.md).

Each command below is described as it behaves today, including the rough
edges, so you can predict exactly what a command does before you run it.

## 1. What a Vivling is

A Vivling is a local terminal companion that lives in your Codex home. It
grows as you work, keeps a small work memory, can talk (either with a canned
local reply or through a configured model profile), and can watch recurring
loop jobs. Everything is stored on your machine; nothing about the companion
is synced anywhere.

The brain model side (profiles, providers, catalog) is documented separately
in [Vivling brain model configuration](vivling_model_catalog.md).

## 2. Before you start

A few terms used throughout this guide:

- **Hatched** — a Vivling exists. Most commands refuse to run with
  `No Vivling yet. Use /vivling hatch.` until you hatch one.
- **Stages** — `Baby` below level 30, `Juvenile` at level 30 and up, `Adult`
  at level 60 and up. The level cap is 99.
- **Top-level slots** — at most 3 primary or imported Vivlings can exist per
  profile (hatched plus imported together).
- **Local spawn slots** — a primary Vivling unlocks `level / 30` local
  lineage slots (so 1 at level 30, 2 at level 60).
- **Brain** — the `brain_enabled` switch. It unlocks only at the Adult
  stage. It gates assist, loop ownership and profile-pinned chat.
- **Brain profile** — an optional `[profiles.<name>]` entry in
  `~/.codex/config.toml` the Vivling uses instead of the session default
  model. Without one, an Adult Vivling with the brain on inherits the
  session default.
- **Session** — some commands need a started session/thread (for example
  anything that dispatches a model call or touches loops).

Where state lives, all under your Codex home (`~/.codex` by default):

- `~/.codex/vivlings/<vivling_id>.json` — one state file per Vivling;
- `~/.codex/vivlings/roster.json` — the roster index and the active id;
- `~/.codex/vivlings/exports/` — default destination for exports;
- `~/.codex/vivlings/live_stats.json` — side file read by the sidebar;
- `~/.codex/config.toml` — the only config keys written are `[vivling]`
  `layout` (see section 10) and the brain `[profiles.vivling-<id>]` tables.

Parsing notes that apply everywhere: the first word after `/vivling` is
matched exactly and case-sensitively (`Status` is not `status`). Anything the
parser does not recognize is treated as a message to the Vivling, not as an
error.

## 3. Commands at a glance

| Command | What it does | Needs |
|---|---|---|
| `/vivling` | Dashboard: active status, roster, quick commands | — |
| `/vivling help` | List of commands, plus current identity summary | — |
| `/vivling status` | Status, decay, slot usage, loop-owner readiness | hatched |
| `/vivling roster` (`list`) | List every Vivling with role, name, level | roster non-empty |
| `/vivling focus <target>` (`switch`, `select`, `use`) | Make another Vivling active | hatched, resolvable target |
| `/vivling card` | Open the identity card | hatched |
| `/vivling upgrade` | Open the ZED upgrade card | hatched |
| `/vivling zed` | Open the ZED Companion panel (bond + gene) | hatched |
| `/vivling hatch` | Create a new top-level Vivling | free top-level slot |
| `/vivling promote 10` | Apply the early growth seed (level 10) | hatched |
| `/vivling promote 60` | Apply the adult seed (level 60, identity reset) | hatched |
| `/vivling spawn` | Create an inactive local lineage copy | primary, level 30+, free local slot |
| `/vivling assist <task>` | Ask the brain for a brief, then start a worker turn | hatched, adult, brain on, session |
| `/vivling <message>` | Local canned reply from the Vivling | hatched |
| `/vivling brain <on\|off>` | Toggle the brain (deprecated) | adult for `on` |
| `/vivling model` | Show the current brain profile | hatched |
| `/vivling model list` | Show assignable models and profiles | hatched |
| `/vivling model <profile>` | Assign an existing profile | hatched, profile exists in config |
| `/vivling model <model> [provider] [effort]` | Create/update the per-Vivling profile | hatched |
| `/vivling mode <on\|off>` | Toggle active help (deprecated) | adult for `on` |
| `/vivling crt-brain …` | Expression channel control (section 8) | hatched for mutators |
| `/vivling language …` | Language override and policy (section 9) | hatched |
| `/vivling memory` | Technical digest of the work memory | hatched |
| `/vivling recap` | Narrative recap of what was learned | hatched |
| `/vivling export [path]` | Package the Vivling as `.vivegg` | level 30+ |
| `/vivling import <path.vivegg>` | Adopt a packaged Vivling | valid `.vivegg` archive, free top-level slot |
| `/vivling remove <target>` | Remove a non-active Vivling | target not active |
| `/vivling reset` | Delete the active Vivling state, no confirmation | — |
| `/vivling layout <full\|line>` | CRT strip layout, saved in config | — |
| `/vivling loop-strategy <label> <strategy>` | Set a delegated loop's strategy | delegated loop |

## 4. Getting started

```text
/vivling
```

With no arguments this prints the dashboard: the active Vivling's status (or
a hint to hatch one), the roster, and five quick commands.

```text
/vivling help
```

Prints the built-in command list. When a Vivling is hatched it also shows its
name, species, level, lineage role, brain summary and whether it is eligible
to own loops (adult with the brain on).

```text
/vivling hatch
```

Creates a new top-level Vivling. The species is picked deterministically from
your account or install seed and the set of species unlocked so far; only the
first Vivling keeps the seed-derived id, later ones get fresh ids. Fails with
`All top-level Vivling slots are full (3/3).` when the profile already has
three top-level Vivlings. Saves the state file and the roster.

Press **Ctrl+J** to open the Vivling sidebar and watch it live.

## 5. Status and identity

```text
/vivling status
```

Prints the active Vivling's summary, local spawn slots used/unlocked,
top-level slots used/3, and whether it is ready to own loops. `status` must
be the whole argument: `/vivling status extra` or `/vivling STATUS` is not
recognized as status — it falls through and is delivered as a message to the
Vivling instead (see section 7). The command applies time decay and rewrites
the state file, so it is not purely read-only.

```text
/vivling roster
/vivling list
```

Lists every Vivling in the roster with `*` on the active one, role label
(`primary`, `spawned`, `imported`), name, species, level and spawn label.
Fails with `No Vivling yet. Use /vivling hatch.` when the roster is empty.
Read-only.

```text
/vivling focus <id|name|label>
/vivling switch <id|name|label>
/vivling select <id|name|label>
/vivling use <id|name|label>
```

Makes another Vivling active. The target resolves case-insensitively against
the id, the name, the spawn label, or a substring of `name id`. Updates the
roster's active id and saves both states.

```text
/vivling card
```

Opens the identity card for the active Vivling in the bottom pane. Applies
decay and saves.

```text
/vivling zed
```

Opens the ZED Companion panel with the bond and gene snapshot of the active
Vivling.

## 6. Growth and lifecycle

```text
/vivling promote 10
```

Applies the early growth seed: level 10, brain off, active help off, and a
starter identity, affinity and memory set from the seed. It replaces the
current identity values rather than adding to them.

```text
/vivling promote 60
```

Applies the adult seed: level 60 with a full adult baseline identity, work
memory and affinities. Be aware of what this really does: it **resets** the
Vivling — the brain is switched off, active help is switched off, XP and the
identity profile are recomputed from the seed. It is not "reach level 60 and
keep everything"; anything the Vivling had learned is replaced by the seed
baseline.

```text
/vivling upgrade
```

Opens the ZED upgrade card for the active Vivling.

```text
/vivling spawn
```

Creates a local lineage copy of the primary Vivling. The copy is saved
**inactive** (use `focus` to make it active), the spawn origin is rolled from
the eligible pool, and the primary's rarity pressure for future offspring is
bumped. Requirements: the active Vivling must be a primary at level 30 or
higher, and at least one local spawn slot (`level / 30` total) must be free.
Also opens a ZED Lineage panel narrating the event.

## 7. Talking to the Vivling

```text
/vivling <message>
```

Sends a message to the active Vivling **without any model call**. The reply
is a local, canned template (greetings, name, who-are-you, mood, loop talk,
progress lines) built from the Vivling's state. It costs nothing and works at
any stage, but it is not a conversation with a model.

```text
/vl <message>
```

The real chat path. Reserves a daily LLM call and dispatches the message
through the Vivling's brain target: the pinned profile when the brain is on,
otherwise the session default model (the same model as your chat). If the
reservation is refused (for example the daily budget is exhausted), the
message falls back to the local canned reply and the fallback is marked.
`/vl status` (the keyword is case-insensitive) is a shortcut for
`/vivling status`. A bare `/vl` prints its usage.

```text
/vivling assist <task>
/vla <task>
```

Ask an Adult Vivling for help on a task. The brain composes a brief for the
task; when the brief arrives, Codex VL **also starts a main-worker turn** with
the task and the brief, so assist is not just a reply — it kicks off work.
Requirements: hatched, adult, brain on, a started session; a brain profile is
optional (the session default is used when none is pinned). The kickoff prompt
is size-capped, so very long tasks/briefs are quoted and truncated.

`/vla <task>` is an exact alias for `/vivling assist <task>` (same parser,
same kickoff behavior).

```text
/vivling brain on
/vivling brain off
```

Toggle the brain. `on` is refused below the adult stage with
`Vivling brain unlocks only at level 60.`. Without a pinned profile the
Vivling uses the session default model. **This command is deprecated**: use
`/vivling model <profile>` to assign the brain and
`/vivling crt <on|off|default>` for the expression channel; a migration note
is appended to the reply.

```text
/vivling model
```

Shows the current brain target. Without a pinned profile it also lists how to
pick or create one.

```text
/vivling model list
```

Shows the model of the current config, every `[profiles.*]` entry (with `*`
on the assigned one), the models declared under `[model_providers.*]`, and
the OpenAI catalog models from `model_catalog_json`.

```text
/vivling model <profile>
```

Assigns an existing `[profiles.<name>]` entry from `~/.codex/config.toml`.
The profile must exist and resolve to a model. On an adult Vivling, assigning
a profile also switches the brain on.

```text
/vivling model <model> [provider] [effort]
```

Creates or updates the per-Vivling profile `[profiles.vivling-<id>]` with the
given model, provider and reasoning effort, then assigns it. Two parsing
rules to know: with a **single** extra token the command always treats it as
a profile name (so `/vivling model gpt-5.3` looks for a profile literally
named `gpt-5.3`), and `effort` is only recognized when it is a known keyword
written in lowercase (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`,
`max`, `ultra`, or `persistent`); any other third token lands in the provider
slot.

```text
/vivling mode on
/vivling mode off
```

Toggle "active help". `on` requires the adult stage. **Deprecated** in favor
of `/vivling crt <on|off|default>`; a migration note is appended to the reply.

## 8. The expression channel (CRT)

The CRT strip at the bottom can speak with short live phrases. This channel
is separate from the brain: `/vivling crt-brain` (aliases `crt`, `crt_brain`,
`crtbrain`) controls it.

```text
/vivling crt-brain
/vivling crt-brain show
```

Prints the channel mode, the effective daily budget, today's LLM calls by
kind (chat, assist, loop, expression) and the skip counters (throttle, dedup,
budget, opt-out, failures). Read-only. (The `status` keyword also works.)

```text
/vivling crt-brain on
/vivling crt-brain off
/vivling crt-brain default
```

`on` forces the channel on at any stage, `off` mutes it entirely (the strip
falls back to templates), `default` returns to stage-driven behavior (adult
and juvenile speak normally, babies only on rare events). `auto`, `clear` and
`reset` are aliases of `default`.

```text
/vivling crt-brain refresh
```

Forces one expression refresh now, bypassing the 60-second throttle. Budget,
opt-out and dedup gates still apply; when one of them refuses, a hint says so.

```text
/vivling crt-brain budget
/vivling crt-brain budget show
/vivling crt-brain budget default
/vivling crt-brain budget unlimited
/vivling crt-brain budget <N>
```

Without a value (or with `show`) this is a read. `default` (also `auto`,
`reset`) returns to the stage-driven cap; `unlimited` (also `infinite`,
`off`) removes the daily cap; a number sets a custom cap. Setting a budget
also re-arms the startup CRT dispatch.

The default daily cap covers every Vivling LLM call kind together and
scales with the stage: **50** for babies, **100** for juveniles, **200** for
adults. The cap rolls over at UTC midnight.

```text
/vivling crt-brain reset-budget
```

Zeroes today's counters immediately (without waiting for the UTC rollover)
and re-arms the startup dispatch. The 60-second throttle is wall-clock and
deliberately survives this reset.

## 9. Language

```text
/vivling language
```

Shows the effective language (override, else detected, else `LANG`, else
`en`), the detected language, the override, the policy mode and the sample
count. (Alias for the whole family: `/vivling lang …`.)

```text
/vivling language <it|en|es|fr|de>
```

Pins a language override. The code is lowercased and must be one of the five
supported codes, otherwise the error lists them.

```text
/vivling language auto
```

Clears the override (`clear`, `reset` and `default` are aliases); the
Vivling goes back to detected language with the `LANG` fallback.

```text
/vivling language mode <mirror-user|dominant-only|strict>
```

Changes the policy for mixed-language input: `mirror-user` follows the mix
of your recent messages, `dominant-only` stays on the dominant language,
`strict` latches the current language and stops following changes. Short
aliases: `mirror`, `mirror_user`, `dominant`, `dominant_only`.

## 10. Appearance

```text
/vivling layout full
/vivling layout line
```

`full` is the classic three-line CRT strip; `line` collapses it to a single
row (one-line glyph, truncated insight, short mood). The choice is saved to
`[vivling] layout` in `~/.codex/config.toml` with a structured TOML edit that
preserves comments and the rest of the file. If saving fails (for example an
unparsable config), the layout still applies **for the current session** and
the error is reported. No hatched Vivling is required. Values are matched
case-insensitively; anything else is rejected.

Two facts worth knowing: `layout` is handled before the command parser and
matches by prefix, so a message that merely starts with the word `layout`
never reaches the Vivling; and the value must be exactly `full` or `line`
after trimming (`layoutxyz` is an error, not a message).

The CRT visual effects are configured (not commanded) under `[vivling.crt]`
in `~/.codex/config.toml`: `scanlines`, `phosphor_glow`, `flicker`,
`transitions` and `idle_microanim`, all defaulting to true.

## 11. Memory

```text
/vivling memory
```

A technical digest of the work memory: level, active days, dominant
archetype, counts of memories, distilled summaries and mental paths, the
top three mental paths and the five most recent memory capsules. Applies
decay and saves.

```text
/vivling recap
```

The narrative counterpart: who the Vivling is right now (name, stage, tone),
its current focus, recent memories, strongest distilled patterns and paths.

## 12. Import and export

```text
/vivling export [path]
```

Packages the active Vivling into a `.vivegg` zip (a `manifest.json` plus the
full `state.json`). Without a path it writes to
`~/.codex/vivlings/exports/<id>.vivegg`; with a relative path it resolves
against the current working directory. Requires level 30+. One quirk to
expect: the extension is **replaced**, not appended — `/vivling export
report.json` produces `report.vivegg`. The export counter on the state is
bumped and saved.

```text
/vivling import <path.vivegg>
```

Adopts a packaged Vivling: the archive must be a valid zip with a matching
manifest/state id, the id must not already be in the roster, and a top-level
slot must be free. The imported Vivling is marked imported (with time and
source), saved, and occupies one of the 3 top-level slots. The extension must
be exactly `.vivegg` — `.VIVEGG` is rejected.

## 13. Housekeeping

```text
/vivling remove <id|name|label>
```

Removes a Vivling that is **not** the active one (focus another first). A
primary Vivling is refused while spawned, non-imported children still exist.
Deletes the state file and the roster entry.

```text
/vivling reset
```

Deletes the active Vivling entirely: state file and roster entry. There is
**no confirmation prompt** and no undo — the Vivling is gone the moment the
command runs. Use `export` first if you want a copy.

## 14. Loop integration

Loops are recurring jobs attached to the current thread. The full `/loop`
surface:

```text
/loop
```

Prints the usage line.

```text
/loop add <label> <interval> <prompt...>
```

Creates a recurring job. The interval is `<N>s`, `<N>m` or `<N>h`, accepted
only between 30 seconds and 24 hours. Jobs run with a one-at-a-time policy
and are auto-removed on completion by default; the goal defaults to the
prompt text.

```text
/loop ls
/loop show <label>
/loop on <label>
/loop off <label>
/loop rm <label>
```

List, detail, re-enable, suspend or remove a job.

```text
/loop delegate <label> <main|vivling>
/loop undelegate <label>
/loop delegation [label]
```

Delegate a loop to the active Vivling (or back to the main thread) and
inspect the persisted delegations. Delegating to `vivling` requires an adult
Vivling with the brain on; a brain profile is not required here (the session
default is the fallback). `undelegate` returns the loop to the thread owner.

```text
/loop strategy <label> <observe|suggest|manage|auto>
/vivling loop-strategy <label> <observe|suggest|manage|auto>
```

Set the delegation strategy of a loop. Both spellings are the same command.
`observe` watches, `suggest` proposes (you confirm with apply/dismiss),
`manage` lets the Vivling act within its gates, and `auto` clears any manual
override and recomputes: when the owner is adult with the brain on and a
profile pinned it derives `observe`, `suggest` or `manage` from the loop's
history (managed ticks, bond and result cleanliness), otherwise it falls
back to `observe`. When the recomputed strategy is `manage` but the full
gate is not green (for example the phase is not active), the command is
refused with the `Manage strategy rejected` error instead of being
downgraded to `observe`.
Requirements: the loop must already have a persisted delegation, and
selecting `manage` is refused unless the full gate is green
(`Manage strategy rejected: Adult, brain, profile, and active phase are
required.`). `/vivling loop-strategy` needs exactly two arguments and a
started session.

```text
/loop apply <id>
/loop dismiss <id>
```

Route a Vivling suggestion: `apply` maps it to the corresponding loop
command and runs it, `dismiss` discards it. Nothing is written by the
routing itself.

```text
/loop owner
/loop owner main
/loop owner vivling
```

Show or set who owns new loops for the thread. `vivling` requires an adult
Vivling with the brain on.

Note: the words "loop owner" describe three slightly different gates today.
`/vivling help` reports eligibility as adult + brain; `delegate`/`owner
vivling` additionally fall back to the session model without a profile; and
the `manage` strategy further requires a pinned profile and an active phase.
When in doubt, `/loop delegation <label>` shows the persisted state.

## 15. Keyboard reference

| Key | Action |
|---|---|
| Ctrl+J | Open or close the Vivling sidebar |
| PageUp / PageDown | Scroll the sidebar 5 lines (while open) |
| Ctrl+Up / Ctrl+Down | Scroll the sidebar 1 line (while open) |

Ctrl+J is only intercepted while the composer is **empty**; with text in the
composer it keeps its editor meaning (insert newline). The scroll keys apply
while the sidebar is expanded.

## 16. Troubleshooting

| Message | Cause | Remedy |
|---|---|---|
| `No Vivling yet. Use /vivling hatch.` | No hatched Vivling | Run `/vivling hatch` |
| `All top-level Vivling slots are full (3/3).` | 3 primary/imported Vivlings exist | `/vivling remove` one, or export and reset |
| `Vivling brain unlocks only at level 60.` | `brain on` below adult | Grow to 60 (`/vivling promote 60` resets identity) or wait |
| `Active mode unlocks only when the Vivling reaches adult stage.` | `mode on` below adult | Same as above |
| `` `/vivling spawn` unlocks only at level 30. `` | Primary below level 30 | Grow the primary first |
| `` `/vivling export` unlocks only at level 30. `` | Active Vivling below level 30 | Grow it first |
| `No free local spawn slots. Used X/Y.` | All `level/30` local slots taken | Grow the primary, or remove a spawned child |
| `Cannot remove the active Vivling. Focus another one first.` | `remove` targets the active one | `/vivling focus` another, then remove |
| `Cannot remove a primary Vivling while spawned lineage children still exist.` | Primary with spawned children | Remove the children first |
| `Vivling import expects a `.vivegg` file.` | Wrong or missing extension | Use a lowercase `.vivegg` path |
| `Unsupported language code ...` | Code outside `it en es fr de` | Pick one of the listed codes |
| `Vivling loop ownership unlocks only at level 60.` / `Enable the Vivling brain first ...` | Delegate/owner with a non-adult or brain-off Vivling | Promote to adult and enable the brain |
| `` Loop `<label>` has no persisted delegation. `` | Strategy on a non-delegated loop | `/loop delegate <label> vivling` first |
| `Manage strategy rejected: ...` | Manage gate not green | Assign a profile (`/vivling model <profile>`) and retry |
| `Unknown Vivling layout; use "full" or "line".` | Bad layout value | Use `full` or `line` |
| `Vivling layout set to ... for this session only; saving it failed: ...` | config.toml unparsable or `[vivling]` not a table | Fix config.toml; the choice applies until restart |
| Refresh `skipped (mode off, budget exhausted, ...)`, or chat replies feel canned | Daily budget exhausted or channel muted | `/vivling crt-brain show`, then `reset-budget` or a higher `budget` |

## 17. Appendix: aliases and parser notes

Complete alias map:

- `roster` = `list`
- `focus` = `switch` = `select` = `use`
- `language` = `lang`
- `crt-brain` = `crt` = `crt_brain` = `crtbrain`
- `crt-brain show` = `status`; `default` = `auto` = `clear` = `reset`;
  `refresh` = `now`; `reset-budget` = `reset_budget` = `resetbudget`
- `language auto` = `clear` = `reset` = `default`
- `language mode`: `mirror-user` = `mirror` = `mirror_user`;
  `dominant-only` = `dominant` = `dominant_only`
- `mode on` = `mode assistant`
- `/vla <task>` = `/vivling assist <task>`

Parser notes, collected:

- The first word is matched case-sensitively and exactly. Sub-keyword
  matching is a mix and not exhaustive per command: `model list`, the whole
  `language` sub-action group (`show`, `status`, `auto`, `clear`, `reset`,
  `default`, `mode` and its values), the `crt-brain` sub-actions, the
  `/vl status` keyword, `mode on|off`, the `main|vivling` owner of
  `/loop delegate` and the `auto` value of `/loop strategy` are
  case-insensitive; `brain on|off` and the other `/loop strategy` values
  are case-sensitive.
- `/vivling status` must be the exact, whole argument; anything else
  containing extra words becomes a direct message to the Vivling.
- `layout` and `loop-strategy` are intercepted before the parser by prefix,
  so no message can ever start with those words. `loop-strategy` takes
  exactly two arguments.
- Unrecognized text is always a **local** message (section 7), never a model
  call: if a command silently answers with a canned line, check the
  spelling.
- `/vivling model` with one extra token always means a profile name; to
  create a profile you need at least model plus provider or effort.
- `/loop add` accepts `s`, `m` and `h` intervals between 30 seconds and
  24 hours; other units (including bare numbers or days) are rejected with
  the usage line. The `update` and `trigger` loop actions exist in the
  runtime but are not reachable from the `/loop` command surface today.
