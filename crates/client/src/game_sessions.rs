//! Actual-command wrapper. Only the desktop process executes the game; the
//! daemon derives scope identity from the socket peer and owns the profile hold.
pub use crate::game_supervisor::exit_code;
use crate::launch_issues::{best_effort, History, Issue, Reason, Stage};
use omen_core::config::OmenConfig;
use omen_core::games::{GameSession, SessionPhase};
use omen_core::{OmenError, Result};
use omen_ipc::{IpcClient, Op};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus};

fn execute(arguments: &[OsString]) -> Result<ExitStatus> {
    let (program, args) = arguments.split_first().ok_or_else(|| {
        OmenError::InvalidArgument(
            "game-run requires -- followed by the actual game command".into(),
        )
    })?;
    Err(OmenError::LaunchFailed(format!(
        "could not execute the wrapped command: {}",
        Command::new(program)
            .args(args)
            .env_remove(crate::launch_issues::REQUEST_ENV)
            .exec()
    )))
}

/// Check if `mangohud` is executable in PATH.
pub fn mangohud_available() -> bool {
    omen_hardware::games::PreparedLaunch::mangohud_executable().is_some()
}

/// Wrap arguments with `mangohud` if overlay is enabled, available, and not already present.
pub fn prepare_arguments(arguments: &[OsString], enable_mangohud: bool) -> Vec<OsString> {
    let already = arguments.first().is_some_and(|first| {
        std::path::Path::new(first).file_name() == Some(std::ffi::OsStr::new("mangohud"))
    });
    if enable_mangohud && !arguments.is_empty() && !already && mangohud_available() {
        let mut wrapped = Vec::with_capacity(arguments.len() + 1);
        wrapped.push(OsString::from("mangohud"));
        wrapped.extend_from_slice(arguments);
        wrapped
    } else {
        arguments.to_vec()
    }
}

pub fn run(
    id: &str,
    arguments: &[OsString],
    socket: Option<&str>,
    worker_token: Option<&str>,
) -> Result<ExitStatus> {
    run_with_overlay(id, arguments, socket, worker_token, false)
}

pub fn run_with_overlay(
    id: &str,
    arguments: &[OsString],
    socket: Option<&str>,
    worker_token: Option<&str>,
    mangohud: bool,
) -> Result<ExitStatus> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(OmenError::PermissionDenied(
            "games must run as the desktop user, never root".into(),
        ));
    }
    if arguments.is_empty() || !omen_core::games::valid_game_id(id) {
        return Err(OmenError::InvalidArgument(
            "game-run needs a valid game ID and actual command arguments".into(),
        ));
    }
    let history = History::for_config(&OmenConfig::path());
    let inherited = std::env::var(crate::launch_issues::REQUEST_ENV)
        .ok()
        .filter(|v| crate::launch_issues::valid_id(v))
        .or_else(|| {
            worker_token
                .filter(|v| crate::launch_issues::valid_id(v))
                .map(str::to_owned)
        });
    let early_report = |stage, error: &OmenError| {
        // Diagnostic availability never blocks an unbound command.
        let request = inherited
            .clone()
            .or_else(|| crate::games::new_transaction_token().ok());
        if let Some(request) = request {
            best_effort(
                &history,
                Issue::new(
                    request,
                    id.into(),
                    None,
                    stage,
                    Reason::from_error(stage, error),
                ),
            );
        }
    };
    let config = OmenConfig::load().inspect_err(|e| early_report(Stage::Configuration, e))?;
    let enable_hud = mangohud || config.games.mangohud_overlay;
    let effective_args = prepare_arguments(arguments, enable_hud);
    if !config.games.managed_game_ids.contains(id) {
        return execute(&effective_args).inspect_err(|e| early_report(Stage::Execution, e));
    }
    let request = inherited
        .map(Ok)
        .unwrap_or_else(crate::games::new_transaction_token)?;
    let scope = worker_token
        .filter(|v| crate::launch_issues::valid_id(v))
        .map(str::to_owned);
    let report = |stage, error: &OmenError| {
        best_effort(
            &history,
            Issue::new(
                request.clone(),
                id.into(),
                scope.clone(),
                stage,
                Reason::from_error(stage, error),
            ),
        );
    };
    let profile = config
        .games
        .per_game_profiles
        .get(id)
        .copied()
        .ok_or_else(|| {
            OmenError::InvalidArgument("automatic game profile binding is missing".into())
        })
        .inspect_err(|e| report(Stage::Configuration, e))?;
    if let Some(token) = worker_token {
        let daemon = socket
            .map(IpcClient::new)
            .unwrap_or_else(IpcClient::default_path);
        let session: GameSession = daemon
            .call_decode(Op::StartManagedGame {
                token: token.into(),
                game_id: id.into(),
                profile,
            })
            .inspect_err(|e| report(Stage::Registration, e))?;
        if !session.profile_held || session.phase != SessionPhase::Tracking {
            let error = OmenError::SafetyLockout("the managed profile was released before command execution; retry with a new session".into());
            report(Stage::Registration, &error);
            return Err(error);
        }
        let outcome = execute(&effective_args).inspect_err(|e| report(Stage::Execution, e));
        // Failed exec still has a live worker: relinquish its hold promptly.
        let _ = daemon.call(Op::ReleaseGameSession {
            token: token.into(),
        });
        return outcome;
    }
    let token = crate::games::new_transaction_token()?;
    let executable = std::env::current_exe()?;
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--description=OMEN managed game scope",
            "--collect",
            "--expand-environment=no",
        ])
        .arg(format!("--unit=omen-game-{token}.scope"))
        .arg("--")
        .arg(executable);
    if let Some(socket) = socket {
        command.args(["--socket", socket]);
    }
    command
        .args(["game-worker", id, "--token", &token, "--"])
        .args(&effective_args);
    command.env(crate::launch_issues::REQUEST_ENV, &request);
    let signals = crate::game_supervisor::Signals::new()
        .map_err(OmenError::from)
        .inspect_err(|e| report(Stage::Scope, e))?;
    let mut child = signals
        .spawn(&mut command)
        .map_err(OmenError::from)
        .inspect_err(|e| report(Stage::Scope, e))?;
    let status = signals
        .wait(&mut child)
        .map_err(OmenError::from)
        .inspect_err(|e| report(Stage::Submission, e))?;
    if let Some(reason) = Reason::from_status(status) {
        best_effort(
            &history,
            Issue::new(request, id.into(), Some(token), Stage::Submission, reason),
        );
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mangohud_leaves_arguments_unchanged() {
        let args = vec![OsString::from("/bin/true"), OsString::from("--help")];
        let prep = prepare_arguments(&args, false);
        assert_eq!(prep, args);
    }

    #[test]
    fn already_mangohud_does_not_duplicate() {
        let args = vec![OsString::from("mangohud"), OsString::from("game")];
        let prep = prepare_arguments(&args, true);
        assert_eq!(prep, args);
    }

    #[test]
    fn path_qualified_mangohud_does_not_duplicate() {
        let args = vec![OsString::from("/usr/bin/mangohud"), OsString::from("game")];
        let prep = prepare_arguments(&args, true);
        assert_eq!(prep, args);
    }
}
