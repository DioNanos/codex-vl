use super::family_for_terminal;
use super::hint_for;
use crate::key_hint;
use crate::keymap::RuntimeKeymap;
use codex_terminal_detection::Multiplexer;
use codex_terminal_detection::TerminalInfo;
use codex_terminal_detection::TerminalName;
use crossterm::event::KeyCode;

fn terminal(name: TerminalName) -> TerminalInfo {
    TerminalInfo {
        name,
        term_program: None,
        version: None,
        term: None,
        multiplexer: None,
    }
}

fn under_tmux() -> TerminalInfo {
    TerminalInfo {
        name: TerminalName::Unknown,
        term_program: None,
        version: None,
        term: None,
        multiplexer: Some(Multiplexer::Tmux { version: None }),
    }
}

/// `prompt_stack_back` must follow the terminal family, not the keymap order:
/// upstream #47618 put Shift first, but Alt remains this fork's default family.
#[test]
fn prompt_stack_back_follows_the_terminal_family() {
    let keymap = RuntimeKeymap::defaults();
    let label = |name| {
        hint_for(&keymap, "prompt_stack_back", terminal(name))
            .unwrap()
            .display_label()
    };

    // Terminals that swallow Alt+Up -> Shift family (Shift+Right is the back one).
    assert_eq!(
        label(TerminalName::AppleTerminal),
        key_hint::shift(KeyCode::Right).display_label()
    );
    assert_eq!(
        label(TerminalName::WarpTerminal),
        key_hint::shift(KeyCode::Right).display_label()
    );
    assert_eq!(
        label(TerminalName::VsCode),
        key_hint::shift(KeyCode::Right).display_label()
    );

    // Everything else -> Alt family (Alt+Down is the back one).
    assert_eq!(
        label(TerminalName::Unknown),
        key_hint::alt(KeyCode::Down).display_label()
    );
    assert_eq!(
        label(TerminalName::Ghostty),
        key_hint::alt(KeyCode::Down).display_label()
    );
    assert_eq!(
        label(TerminalName::Iterm2),
        key_hint::alt(KeyCode::Down).display_label()
    );
}

/// A multiplexer overrides the terminal name: tmux does not pass Alt+Up reliably.
#[test]
fn prompt_stack_back_under_tmux_uses_the_shift_family() {
    let keymap = RuntimeKeymap::defaults();
    let hint = hint_for(&keymap, "prompt_stack_back", under_tmux()).unwrap();
    assert_eq!(
        hint.display_label(),
        key_hint::shift(KeyCode::Right).display_label()
    );
}

/// Forward and back are two directions of the SAME family: whichever family the
/// terminal picks, the two hints must belong to it together.
#[test]
fn forward_and_back_stay_in_the_same_family() {
    let keymap = RuntimeKeymap::defaults();
    for name in [
        TerminalName::Unknown,
        TerminalName::Ghostty,
        TerminalName::AppleTerminal,
        TerminalName::VsCode,
    ] {
        let (forward, backward) = family_for_terminal(terminal(name));
        assert_eq!(
            hint_for(&keymap, "edit_queued_message", terminal(name))
                .unwrap()
                .display_label(),
            forward.display_label(),
        );
        assert_eq!(
            hint_for(&keymap, "prompt_stack_back", terminal(name))
                .unwrap()
                .display_label(),
            backward.display_label(),
        );
    }
}
