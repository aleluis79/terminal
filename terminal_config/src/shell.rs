//! Shell profile resolution (RF-64, RF-65).
//!
//! Answers three questions without touching the GUI, the GPU or a PTY:
//! which shells the user can pick ([`Config::shell_profiles`]), which one is
//! the default ([`Config::default_shell_command`]) and whether a program is
//! actually installed ([`program_exists`]).

use std::path::Path;

use crate::config::{Config, ShellProfile};

/// A shell ready to be launched (RF-64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    /// Display name of the profile (e.g., `"wsl"`).
    pub name: String,
    /// Program to run (e.g., `"wsl.exe"`).
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
}

impl ShellCommand {
    /// Build a command from its parts.
    pub fn new(name: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args,
        }
    }

    /// The shell of the system: `$SHELL` (Unix) or `%COMSPEC%` (Windows).
    pub fn platform_default() -> Self {
        Self {
            name: "shell".to_string(),
            program: platform_shell_program(),
            args: Vec::new(),
        }
    }
}

impl Config {
    /// Shells the user can pick (RF-64, RF-65, CL-26).
    ///
    /// Configured profiles win; entries without `name` or `program` are
    /// discarded. When no valid profile is configured, the platform defaults
    /// are returned instead.
    #[must_use]
    pub fn shell_profiles(&self) -> Vec<ShellProfile> {
        let configured: Vec<ShellProfile> = self
            .shell
            .profiles
            .iter()
            .filter(|profile| !profile.name.trim().is_empty() && !profile.program.trim().is_empty())
            .cloned()
            .collect();

        if configured.is_empty() {
            platform_profiles()
        } else {
            configured
        }
    }

    /// Command used for new sessions (RF-62).
    ///
    /// `shell.program` wins when set; otherwise the first profile is used and,
    /// failing that, the shell of the system.
    #[must_use]
    pub fn default_shell_command(&self) -> ShellCommand {
        if let Some(program) = self
            .shell
            .program
            .as_deref()
            .map(str::trim)
            .filter(|program| !program.is_empty())
        {
            return ShellCommand::new("shell", program, self.shell.args.clone());
        }

        self.profile_command(0)
            .unwrap_or_else(ShellCommand::platform_default)
    }

    /// Command for profile `index`, or `None` when it does not exist (RF-66, CL-27).
    #[must_use]
    pub fn profile_command(&self, index: usize) -> Option<ShellCommand> {
        self.shell_profiles().get(index).map(command_from_profile)
    }

    /// Command for every pickable profile, in order (RF-64).
    ///
    /// Index `n` here is what `Ctrl+Shift+(n+1)` opens (RF-66).
    #[must_use]
    pub fn shell_commands(&self) -> Vec<ShellCommand> {
        self.shell_profiles()
            .iter()
            .map(command_from_profile)
            .collect()
    }
}

/// Convert a profile into a launchable command.
fn command_from_profile(profile: &ShellProfile) -> ShellCommand {
    ShellCommand::new(
        profile.name.clone(),
        profile.program.clone(),
        profile.args.clone(),
    )
}

/// Whether `program` is installed (RF-65, CL-29).
///
/// A bare name is looked up in `PATH` (using `PATHEXT` on Windows); anything
/// containing a path separator is checked directly on disk.
#[must_use]
pub fn program_exists(program: &str) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }

    let path = Path::new(program);
    if path.is_absolute() || program.contains('/') || program.contains('\\') {
        return path.is_file();
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    for dir in std::env::split_paths(&paths) {
        if dir.join(program).is_file() {
            return true;
        }

        #[cfg(windows)]
        {
            for ext in path_extensions() {
                if dir.join(format!("{program}{ext}")).is_file() {
                    return true;
                }
            }
        }
    }

    false
}

/// Default profiles of the platform (RF-65).
#[cfg(windows)]
fn platform_profiles() -> Vec<ShellProfile> {
    let candidates = [
        ("cmd", platform_shell_program()),
        ("powershell", "powershell.exe".to_string()),
        ("wsl", "wsl.exe".to_string()),
    ];

    let profiles: Vec<ShellProfile> = candidates
        .into_iter()
        .filter(|(_, program)| program_exists(program))
        .map(|(name, program)| ShellProfile {
            name: name.to_string(),
            program,
            args: Vec::new(),
        })
        .collect();

    if profiles.is_empty() {
        vec![profile_of_platform_shell()]
    } else {
        profiles
    }
}

/// Default profile on Unix: a single profile running `$SHELL` (RF-65).
#[cfg(unix)]
fn platform_profiles() -> Vec<ShellProfile> {
    vec![profile_of_platform_shell()]
}

/// Profile wrapping the shell of the system.
fn profile_of_platform_shell() -> ShellProfile {
    ShellProfile {
        name: "shell".to_string(),
        program: platform_shell_program(),
        args: Vec::new(),
    }
}

/// The shell the system would use: `$SHELL` on Unix, `%COMSPEC%` on Windows.
#[cfg(unix)]
fn platform_shell_program() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

/// The shell the system would use: `$SHELL` on Unix, `%COMSPEC%` on Windows.
#[cfg(windows)]
fn platform_shell_program() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
}

/// Executable extensions to try when looking a bare name up in `PATH`.
#[cfg(windows)]
fn path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn profile(name: &str, program: &str) -> ShellProfile {
        ShellProfile {
            name: name.to_string(),
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    #[test]
    fn test_configured_profiles_win() {
        let mut config = Config::default();
        config.shell.profiles = vec![profile("cmd", "cmd.exe"), profile("wsl", "wsl.exe")];

        let profiles = config.shell_profiles();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "cmd");
        assert_eq!(profiles[1].program, "wsl.exe");
    }

    #[test]
    fn test_empty_profiles_fall_back_to_platform_defaults() {
        let config = Config::default();
        let profiles = config.shell_profiles();
        assert_ne!(profiles, Vec::new());
        assert_ne!(profiles[0].program, "");
    }

    #[test]
    fn test_invalid_profiles_are_discarded() {
        // CL-26: a profile without a program (or without a name) is dropped.
        let mut config = Config::default();
        config.shell.profiles = vec![
            profile("", "cmd.exe"),
            profile("broken", "  "),
            profile("ok", "cmd.exe"),
        ];

        let profiles = config.shell_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "ok");
    }

    #[test]
    fn test_all_invalid_profiles_fall_back() {
        let mut config = Config::default();
        config.shell.profiles = vec![profile("", "")];
        assert_ne!(config.shell_profiles(), Vec::new());
    }

    #[test]
    fn test_program_override_wins_over_profiles() {
        // RF-62: `shell.program` is the default shell, profiles are just pickable.
        let mut config = Config::default();
        config.shell.program = Some("wsl.exe".to_string());
        config.shell.args = vec!["-d".to_string(), "Ubuntu".to_string()];
        config.shell.profiles = vec![profile("cmd", "cmd.exe")];

        let command = config.default_shell_command();
        assert_eq!(command.program, "wsl.exe");
        assert_eq!(command.args, vec!["-d", "Ubuntu"]);
    }

    #[test]
    fn test_default_command_is_first_profile() {
        let mut config = Config::default();
        config.shell.profiles = vec![profile("powershell", "powershell.exe")];

        let command = config.default_shell_command();
        assert_eq!(command.program, "powershell.exe");
        assert_eq!(command.name, "powershell");
    }

    #[test]
    fn test_profile_command_index() {
        let mut config = Config::default();
        config.shell.profiles = vec![profile("cmd", "cmd.exe"), profile("wsl", "wsl.exe")];

        assert_eq!(
            config.profile_command(1).map(|c| c.program),
            Some("wsl.exe".to_string())
        );
        // CL-27: a shortcut for a profile that does not exist yields nothing.
        assert_eq!(config.profile_command(5), None);
    }

    #[test]
    fn test_shell_commands_match_profiles() {
        let mut config = Config::default();
        config.shell.profiles = vec![
            profile("cmd", "cmd.exe"),
            profile("powershell", "powershell.exe"),
        ];

        let commands = config.shell_commands();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "cmd");
        assert_eq!(commands[1].program, "powershell.exe");
        // The default shell is the first command of the list.
        assert_eq!(config.default_shell_command(), commands[0]);
    }

    #[test]
    fn test_platform_default_is_the_system_shell() {
        let command = ShellCommand::platform_default();
        assert_eq!(command.program, platform_shell_program());
        assert_ne!(command.program, "");
    }

    #[test]
    fn test_program_exists_rejects_unknown_and_empty() {
        assert!(!program_exists(""));
        assert!(!program_exists("   "));
        assert!(!program_exists("this-program-does-not-exist-xyz-123"));
    }

    #[test]
    fn test_program_exists_accepts_explicit_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("my-shell");
        std::fs::write(&file, b"").unwrap();

        assert!(program_exists(&file.to_string_lossy()));
    }

    #[test]
    #[cfg(windows)]
    fn test_program_exists_finds_cmd_on_windows() {
        // `cmd` has no extension in the name; PATHEXT must resolve it.
        assert!(program_exists("cmd.exe"));
        assert!(program_exists("cmd"));
    }

    #[test]
    #[cfg(unix)]
    fn test_program_exists_finds_sh_on_unix() {
        assert!(program_exists("sh"));
    }
}
