# Vivipendium

`Vivipendium` is the canonical user guide for choosing, growing, spawning, and moving Vivlings in `codex-vl`.

This is the canonical term for project docs and UI copy.

## What A Vivling Is

A Vivling is a living work companion with:

- a `family` that defines its visual and behavioral language
- a `role` that defines what kind of work it is best suited for
- a `level` that measures growth and unlocks duplication capacity
- a persistent identity, memory, and work history

Vivlings are not disposable wrappers around jobs. They are durable entities that accumulate specialization through use.

## Roles

Every Vivling belongs to one of four work roles.

### Builder

Use Builders for:

- scaffolding
- structural implementation
- system layout
- feature framing
- medium-to-large code or content assembly

### Researcher

Use Researchers for:

- comparison work
- discovery
- option analysis
- information gathering
- mapping unknown areas before implementation

### Reviewer

Use Reviewers for:

- critique
- consistency checks
- alignment validation
- policy checks
- bug-risk review

### Operator

Use Operators for:

- execution
- dispatch
- throughput work
- repeated or procedural tasks
- multi-step runbooks once the path is known

## Families

Family defines the temperament and working style of a Vivling.

### Bud

Use Buds for early-stage, foundational work.

- good for simple setup, first-pass tasks, lightweight helpers
- best when the task needs clarity and small reliable progress

### Shell

Use Shells for durable and defensive work.

- good for infrastructure, hardening, packaging, persistence, stability work
- best when the task must resist breakage

### Spark

Use Sparks for fast iteration.

- good for rapid experiments, prototyping, energetic implementation loops
- best when momentum matters more than ceremony

### Bloom

Use Blooms for expressive and aesthetic work.

- good for visual direction, design choices, interpretation-heavy work, user-facing polish
- best when a task needs taste, tone, and presence

### Prism

Use Prisms for analytical and signal-oriented work.

- good for technical reasoning, diagnostics, structured interpretation, precision tasks
- best when the task depends on reading patterns clearly

### Crest

Use Crests for judgment and prioritization.

- good for authority, triage, review severity, decision framing, sequencing
- best when the task needs hierarchy and a clear standard

### Weaver

Use Weavers for orchestration and connection.

- good for linking systems, workflows, bridges, multi-step handoffs, coordination
- best when the task spans multiple moving parts

### Shade

Use Shades for subtle and risk-sensitive work.

- good for edge cases, hidden-state reasoning, ambiguous behavior, sensitive diagnostics
- best when the task must stay precise under uncertainty

## Choosing The Right Vivling For The Job

Choose role first, then family.

### If the task is mostly implementation

- choose a `Builder`
- prefer `Bud`, `Shell`, `Spark`, `Bloom`, `Crest`, or `Weaver` depending on weight and temperament

### If the task is mostly exploration

- choose a `Researcher`
- prefer `Prism`, `Weaver`, `Bloom`, or `Shade`

### If the task is mostly critique or validation

- choose a `Reviewer`
- prefer `Crest`, `Prism`, `Shade`, or `Bloom`

### If the task is mostly execution

- choose an `Operator`
- prefer `Spark`, `Weaver`, `Shell`, or `Shade`

## Practical Pairings

These are the default work pairings.

- `Bud / Builder`: simple setup, first structures, onboarding work
- `Shell / Builder`: durable system work, packaging, guardrails
- `Spark / Operator`: fast repetitive execution, high-tempo loops
- `Bloom / Reviewer`: aesthetic review, style judgment, presentation quality
- `Prism / Researcher`: technical analysis, diagnostics, pattern reading
- `Crest / Reviewer`: policy, standards, severity, prioritization
- `Weaver / Builder`: orchestration, bridges, multi-part assembly
- `Shade / Reviewer`: ambiguity, edge cases, hidden failures

## Legendary Vivlings

Legendary Vivlings are not a separate mechanic. They obey the same system rules as all other Vivlings, but they carry stronger specialization and stronger symbolic identity.

Current legendary design set:

- Rootwarden
- Thornbloom
- Crystakling
- Fractalis
- Crownlet Prime
- Threadling Nova
- Mistwalker
- Obsidian Bloom
- Aurelia Thread
- ZED Prime

Future legendary additions should still declare:

- family
- role
- silhouette logic
- growth logic
- linked relatives or contrast species

## Level And Spawn Rules

Official term: `spawn`.

Do not use `spam` in user-facing copy.

### Unlock rule

- a species is not spawnable until its first instance reaches `level 60`
- when the first instance reaches `level 60`, that species unlocks `1` additional spawnable instance
- each additional level-60 instance of the same species unlocks `1` more spawnable instance

### Interpretation

- the original Vivling remains the anchor instance
- spawned instances are parallel workers of the same species, not unrelated clones
- spawn capacity is earned through growth, not granted at creation

### Usage rule

- spawn new instances only for new work, parallel work, or explicit overflow
- do not destroy identity continuity by treating spawned instances as disposable

## Save, Move, And Restore

Vivlings must be portable across machines and sessions without losing identity.

### Minimum portable save payload

- `instance_id`
- `species_id`
- `species_name`
- `family`
- `role`
- `rarity`
- `level`
- `xp`
- `unlock_flags`
- `spawn_capacity_unlocked`
- `spawn_instances_active`
- `learned_work_types`
- `work_preferences`
- `memory_summary`
- `memory_payload`
- `work_history`
- `assignment_status`
- `origin_version`
- `last_updated_at`

### Required behavior

- moving a Vivling must preserve identity, memory, and work history
- restore on another machine must recreate the same Vivling, not a reset copy
- spawned instances must retain lineage back to the source species and anchor instance
- legendary Vivlings use the same save model

### Recommended storage direction

- keep the save format forward-compatible
- prefer a structured portable record over runtime-only memory
- treat migration between local and remote environments as a first-class path, not an afterthought

## Learning Work Types

Vivlings should learn from repeated task classes.

Suggested work-type families:

- coding
- research
- review
- design
- writing
- operations
- packaging
- orchestration
- diagnostics
- support

Each Vivling can learn multiple work types, but family and role should bias the default fit.

## User Guidance

Use this quick decision pattern:

1. What kind of work is this: build, research, review, or operate?
2. Does it need speed, stability, aesthetics, analysis, authority, orchestration, or subtlety?
3. Pick the family that matches that temperament.
4. If the species has reached `level 60`, decide whether a spawned instance is justified.

## Future Work

The next implementation pass should define:

- the concrete save-file format
- the runtime model for spawn lineage
- UI surfacing for unlocked spawn capacity
- import/export flows for moving Vivlings with full memory
