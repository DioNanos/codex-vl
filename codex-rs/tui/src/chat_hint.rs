//! Shared resolution of the chat key-hints that depend on the terminal.
//!
//! Two actions belong to the same key *family*, one per direction:
//! `edit_queued_message` moves forward through the queued messages / questions,
//! `prompt_stack_back` moves back toward the composer. The terminal decides the
//! family — Alt+Up/Down everywhere, Shift+Left/Right where Alt+Up is swallowed
//! (Apple Terminal, Warp, the VS Code terminal, tmux) — and the action picks the
//! direction.
//!
//! Upstream #47618 made Shift-arrows the *first* (i.e. displayed) binding, so
//! reading the keymap primary alone would show a different family from the one
//! the fork actually uses. This module is the single place that decides, so the
//! composer hint and the async-questions footer cannot drift apart.

use codex_terminal_detection::Multiplexer;
use codex_terminal_detection::TerminalInfo;
use codex_terminal_detection::TerminalName;
use crossterm::event::KeyCode;

use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::key_hint::ShortcutHint;
use crate::keymap::KeymapContext;
use crate::keymap::RuntimeKeymap;

/// (forward, backward) bindings for this terminal.
///
/// The match is exhaustive so that adding a new `TerminalName` variant forces an
/// explicit decision about which family that terminal should use.
pub(crate) fn family_for_terminal(terminal_info: TerminalInfo) -> (KeyBinding, KeyBinding) {
    if matches!(
        terminal_info.multiplexer.as_ref(),
        Some(Multiplexer::Tmux { .. })
    ) {
        return (
            key_hint::shift(KeyCode::Left),
            key_hint::shift(KeyCode::Right),
        );
    }

    match terminal_info.name {
        TerminalName::AppleTerminal | TerminalName::WarpTerminal | TerminalName::VsCode => (
            key_hint::shift(KeyCode::Left),
            key_hint::shift(KeyCode::Right),
        ),
        TerminalName::Ghostty
        | TerminalName::Iterm2
        | TerminalName::WezTerm
        | TerminalName::Kitty
        | TerminalName::Alacritty
        | TerminalName::Konsole
        | TerminalName::GnomeTerminal
        | TerminalName::Vte
        | TerminalName::WindowsTerminal
        | TerminalName::Dumb
        | TerminalName::Unknown => (key_hint::alt(KeyCode::Up), key_hint::alt(KeyCode::Down)),
    }
}

/// The hint to show for a chat action on this terminal.
///
/// Only `edit_queued_message` and `prompt_stack_back` are terminal-sensitive;
/// every other action keeps the configured primary. A configured chord always
/// wins (it is an explicit choice), and a family binding the keymap no longer
/// holds falls back to the configured primary.
pub(crate) fn hint_for(
    keymap: &RuntimeKeymap,
    action: &'static str,
    terminal_info: TerminalInfo,
) -> Option<ShortcutHint> {
    let (forward, backward) = family_for_terminal(terminal_info);
    let (binding, bindings) = match action {
        "edit_queued_message" => (forward, keymap.chat.edit_queued_message.as_slice()),
        "prompt_stack_back" => (backward, keymap.chat.prompt_stack_back.as_slice()),
        _ => return keymap.primary_hint(KeymapContext::Chat, action),
    };

    let configured = keymap.primary_hint(KeymapContext::Chat, action);
    if matches!(configured, Some(ShortcutHint::Chord { .. })) {
        return configured;
    }

    bindings
        .contains(&binding)
        .then_some(ShortcutHint::Single(binding))
        .or(configured)
}

#[cfg(test)]
#[path = "chat_hint_tests.rs"]
mod tests;
