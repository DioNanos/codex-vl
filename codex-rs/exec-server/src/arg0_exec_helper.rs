#[cfg(unix)]
use std::process::Command;

pub const CODEX_ARG0_EXEC_HELPER_ARG1: &str = "--codex-run-as-arg0-exec-helper";

#[cfg(unix)]
pub fn main() -> ! {
    use std::os::unix::process::CommandExt;

    let mut args = std::env::args_os();
    let _program = args.next();
    let _helper_mode = args.next();
    let Some(arg0) = args.next() else {
        eprintln!("missing arg0 for exec helper");
        std::process::exit(1);
    };
    let Some(program) = args.next() else {
        eprintln!("missing program for exec helper");
        std::process::exit(1);
    };

    let mut command = Command::new(&program);
    command.arg0(arg0).args(args);
    // The helper replaces this process with an external program that inherits
    // the full environment: drop the launcher-private install metadata before
    // the exec. No-op outside Android.
    codex_utils_process::finalize_external_child(&mut command);
    let error = command.exec();
    eprintln!("failed to exec {program:?}: {error}");
    std::process::exit(1);
}

#[cfg(not(unix))]
pub fn main() -> ! {
    eprintln!("arg0 exec helper is only supported on Unix");
    std::process::exit(1);
}
