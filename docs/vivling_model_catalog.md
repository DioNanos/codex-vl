# Vivling Brain Model Configuration

This guide explains how a Vivling resolves its model, provider, and model catalog metadata without relying on shell wrappers.

The important design rule is:

- the main `codex-vl` session model stays separate
- each Vivling stores only a profile reference
- the Vivling brain loads that profile through the normal `~/.codex/config.toml` machinery

That keeps the feature merge-safe and makes custom providers work the same way they already work for the main Codex runtime.

## How Vivling Model Resolution Works

When you run Vivling brain commands such as:

```text
/vivling model <profile>
/vivling model <model> [provider] [effort]
/vivling brain on
/vivling assist <task>
/vl <message>
```

the Vivling does not use shell wrappers.

Instead it loads a normal Codex profile from `~/.codex/config.toml`.

The resolved profile provides:

- `model`
- `model_provider`
- `model_reasoning_effort`
- `model_catalog_json`
- the matching `model_providers.<provider>` entry

That means:

- model metadata comes from the normal model catalog
- endpoint and auth come from the normal provider config
- the Vivling brain stays separate from the main session model
- `/vivling assist <task>` uses a task-oriented prompt
- `/vl <message>` uses a companion-chat prompt

## What Must Exist

For a Vivling custom model to work cleanly, these pieces must already exist:

1. a valid `model_catalog_json`
2. a provider entry under `[model_providers.<id>]`
3. a Vivling profile that points to that provider

The Vivling command can create or update a profile entry, but it does not invent the provider definition for you.

## Recommended Setup for Custom Models

For custom models, the current recommended path is:

- use the model catalog for model slugs and context metadata
- use a normal provider entry in `config.toml`
- treat an Ollama-compatible endpoint as the simplest custom-provider path

This is especially useful when your local testing shows that an Ollama-compatible endpoint behaves better than shell wrappers for recent Codex builds.

The examples below use a local Ollama-compatible provider name. You can use a different provider id, base URL, or model slug as long as the profile, provider, and catalog agree.

## Example `config.toml`

This example is intentionally generic and does not include real secrets.

```toml
model = "gpt-5.4"
model_provider = "openai"
model_reasoning_effort = "medium"
model_catalog_json = "/Users/you/.config/codex/model_catalog.json"

[model_providers.ollama]
name = "Ollama"
base_url = "http://127.0.0.1:11434/v1"
wire_api = "responses"
api_key = "ollama"

[profiles.vivling-default]
model = "deepseek-v4-flash:cloud"
model_provider = "ollama"
model_reasoning_effort = "medium"
```

Notes:

- `model_catalog_json` points to the shared model catalog file
- `model_provider = "ollama"` selects the provider entry defined above
- the Vivling profile can point to any model slug that exists in the catalog
- `api_key = "ollama"` is only a placeholder for a local compatible endpoint; use whatever your local runtime expects

## Example `model_catalog.json`

The catalog only needs to describe the model slug and its metadata. The provider config stays in `config.toml`.

Minimal example:

```json
{
  "models": [
    {
      "slug": "deepseek-v4-flash:cloud",
      "display_name": "DeepSeek V4 Flash Cloud",
      "description": "Example custom model through Ollama-compatible runtime",
      "supported_reasoning_levels": ["medium", "high"],
      "shell_type": "default",
      "visibility": "none",
      "supported_in_api": true,
      "priority": 50,
      "base_instructions": "",
      "supports_reasoning_summaries": false,
      "default_reasoning_summary": "auto",
      "support_verbosity": false,
      "web_search_tool_type": "text",
      "truncation_policy": { "mode": "tokens", "limit": 200000 },
      "supports_parallel_tool_calls": true,
      "supports_image_detail_original": false,
      "context_window": 200000,
      "max_context_window": 200000,
      "effective_context_window_percent": 90,
      "experimental_supported_tools": [],
      "input_modalities": ["text"],
      "supports_search_tool": false
    }
  ]
}
```

## Vivling Commands

Once the provider, profile, and catalog are in place:

```text
/vivling model vivling-default
/vivling brain on
/vl hello
```

Expected behavior:

- if the active Vivling is adult, brain-enabled, and has a profile, `/vl` dispatches to the configured model
- if the brain is not ready, `/vl` falls back to the local lightweight reply path
- `/vivling assist <task>` remains the explicit task-oriented brain command

For explicit task-oriented use:

```text
/vivling assist review the current blocker
```

Or create/update a Vivling profile from inside the TUI:

```text
/vivling model deepseek-v4-flash:cloud ollama medium
/vivling brain on
/vl hello
```

This updates the Vivling profile, but it still assumes the `ollama` provider already exists in `config.toml`.

## Wrappers vs Profiles

Wrappers are still useful for launching your own CLI sessions quickly.

But for the Vivling brain, the preferred path is:

- profiles
- provider definitions
- model catalog

not shell wrappers.

That keeps the Vivling internal, reproducible, and merge-safe.

## Troubleshooting

If `/vl` still uses the local fallback:

- check that the active Vivling is adult, normally level 60 or higher
- run `/vivling model` to confirm a brain profile exists
- run `/vivling brain on`
- confirm the profile name exists in `~/.codex/config.toml`
- confirm the model slug exists in `model_catalog_json`
- confirm the provider endpoint is reachable

If the model starts but tool calls fail, check the provider compatibility first. Some OpenAI-compatible or Anthropic-compatible endpoints are stricter about tool names, thinking fields, or request schemas.
