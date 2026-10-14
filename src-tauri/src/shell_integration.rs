//! Starts the user's interactive shell with a session-only prompt hook.
//! No startup files are edited and no commands are typed into the PTY.

pub fn probe_command(token: &str) -> String {
    let script = format!(
        "# __SSH_WORKSPACE_PROBE__\n__ssh_workspace_probe_token={}\nprintf '\\n__SSH_WORKSPACE_SHELL__%s;%s\\n' \"$__ssh_workspace_probe_token\" \"${{SHELL-}}\"",
        quote(token),
    );
    format!("/bin/sh -c {}", quote(&script))
}

pub fn supported_shell(output: &str, token: &str) -> Option<String> {
    let prefix = format!("__SSH_WORKSPACE_SHELL__{token};");
    output.lines().find_map(|line| {
        let path = line.strip_prefix(&prefix)?;
        (path.starts_with('/')
            && !path.contains(['\0', '\r', '\n'])
            && matches!(path.rsplit('/').next(), Some("bash" | "zsh" | "fish")))
        .then(|| path.to_string())
    })
}

pub fn launch_command(token: &str, shell: &str) -> String {
    let bash = bash_rc(token);
    let zsh = zsh_hook(token);
    let fish = fish_init(token);
    let script = format!(
        r#"# __SSH_WORKSPACE_BOOTSTRAP__
__ssh_workspace_shell={shell}
case "${{__ssh_workspace_shell##*/}}" in
  bash)
    __ssh_workspace_rc=$(mktemp "${{TMPDIR:-/tmp}}/ssh-workspace.XXXXXXXX") || exec "$__ssh_workspace_shell" -il
    if ! cat > "$__ssh_workspace_rc" <<'__SSH_WORKSPACE_BASH_RC__'
{bash}
__SSH_WORKSPACE_BASH_RC__
    then
      command rm -f -- "$__ssh_workspace_rc"
      exec "$__ssh_workspace_shell" -il
    fi
    exec "$__ssh_workspace_shell" --noprofile --rcfile "$__ssh_workspace_rc" -i
    ;;
  zsh)
    __ssh_workspace_dir=$(mktemp -d "${{TMPDIR:-/tmp}}/ssh-workspace.XXXXXXXX") || exec "$__ssh_workspace_shell" -il
    __ssh_workspace_zsh_fallback() {{
      command rm -f -- "$__ssh_workspace_dir/.zshenv" "$__ssh_workspace_dir/.zprofile" "$__ssh_workspace_dir/.zshrc" "$__ssh_workspace_dir/.zlogin"
      command rmdir -- "$__ssh_workspace_dir"
      unset SSH_WORKSPACE_ORIGINAL_ZDOTDIR SSH_WORKSPACE_TEMP_ZDOTDIR
      exec "$__ssh_workspace_shell" -il
    }}
    export SSH_WORKSPACE_ORIGINAL_ZDOTDIR="${{ZDOTDIR-$HOME}}"
    export SSH_WORKSPACE_TEMP_ZDOTDIR="$__ssh_workspace_dir"
    if ! cat > "$__ssh_workspace_dir/.zshenv" <<'__SSH_WORKSPACE_ZSH_ENV__'
if [[ -r "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zshenv" ]]; then
  source "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zshenv"
fi
[[ -n $ZDOTDIR && $ZDOTDIR != $SSH_WORKSPACE_TEMP_ZDOTDIR ]] && SSH_WORKSPACE_ORIGINAL_ZDOTDIR=$ZDOTDIR
ZDOTDIR=$SSH_WORKSPACE_TEMP_ZDOTDIR
__SSH_WORKSPACE_ZSH_ENV__
    then __ssh_workspace_zsh_fallback; fi
    if ! cat > "$__ssh_workspace_dir/.zprofile" <<'__SSH_WORKSPACE_ZSH_PROFILE__'
[[ -r "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zprofile" ]] && source "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zprofile"
ZDOTDIR=$SSH_WORKSPACE_TEMP_ZDOTDIR
__SSH_WORKSPACE_ZSH_PROFILE__
    then __ssh_workspace_zsh_fallback; fi
    if ! cat > "$__ssh_workspace_dir/.zshrc" <<'__SSH_WORKSPACE_ZSH_RC__'
[[ -r "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zshrc" ]] && source "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zshrc"
ZDOTDIR=$SSH_WORKSPACE_TEMP_ZDOTDIR
__SSH_WORKSPACE_ZSH_RC__
    then __ssh_workspace_zsh_fallback; fi
    if ! cat > "$__ssh_workspace_dir/.zlogin" <<'__SSH_WORKSPACE_ZSH_LOGIN__'
[[ -r "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zlogin" ]] && source "$SSH_WORKSPACE_ORIGINAL_ZDOTDIR/.zlogin"
ZDOTDIR=$SSH_WORKSPACE_ORIGINAL_ZDOTDIR
command rm -f -- "$SSH_WORKSPACE_TEMP_ZDOTDIR/.zshenv" "$SSH_WORKSPACE_TEMP_ZDOTDIR/.zprofile" "$SSH_WORKSPACE_TEMP_ZDOTDIR/.zshrc" "$SSH_WORKSPACE_TEMP_ZDOTDIR/.zlogin"
command rmdir -- "$SSH_WORKSPACE_TEMP_ZDOTDIR"
unset SSH_WORKSPACE_ORIGINAL_ZDOTDIR SSH_WORKSPACE_TEMP_ZDOTDIR
{zsh}
__SSH_WORKSPACE_ZSH_LOGIN__
    then __ssh_workspace_zsh_fallback; fi
    ZDOTDIR="$__ssh_workspace_dir" exec "$__ssh_workspace_shell" -il
    ;;
  fish)
    exec "$__ssh_workspace_shell" --login --interactive --init-command {fish}
    ;;
esac"#,
        shell = quote(shell),
        fish = quote(&fish),
    );
    format!("/bin/sh -c {}", quote(&script))
}

fn encode_function() -> &'static str {
    r#"__ssh_workspace_encode() {
  REPLY=$1
  REPLY=${REPLY//%/%25}
  REPLY=${REPLY//;/%3B}
  REPLY=${REPLY//$'\a'/%07}
  REPLY=${REPLY//$'\e'/%1B}
  REPLY=${REPLY//$'\n'/%0A}
  REPLY=${REPLY//$'\r'/%0D}
}"#
}

fn prompt_function(token: &str) -> String {
    format!(
        r#"__ssh_workspace_token={token}
{encode}
__ssh_workspace_prompt() {{
  local __ssh_workspace_status=$?
  local __ssh_workspace_cwd __ssh_workspace_home REPLY
  __ssh_workspace_encode "$PWD"
  __ssh_workspace_cwd=$REPLY
  __ssh_workspace_encode "${{HOME-}}"
  __ssh_workspace_home=$REPLY
  printf '\033]777;ssh-workspace;%s;%s;%s\007' "$__ssh_workspace_token" "$__ssh_workspace_cwd" "$__ssh_workspace_home"
  return "$__ssh_workspace_status"
}}"#,
        token = quote(token),
        encode = encode_function(),
    )
}

fn bash_rc(token: &str) -> String {
    // --rcfile is intentionally non-login so Bash reads this temporary file.
    // Source the same profile chain as a normal SSH login before installing our
    // hook. Profiles which source .bashrc keep doing so themselves.
    // BASH_ENV only applies to non-interactive Bash, and an inherited
    // PROMPT_COMMAND can be overwritten by profiles. This reliable hook retains
    // the non-login flag: login_shell checks and .bash_logout remain a known
    // difference rather than replacing a user's EXIT trap to emulate logout.
    format!(
        r#"command rm -f -- "${{BASH_SOURCE[0]}}"
if [[ -r /etc/profile ]]; then source /etc/profile; fi
if [[ -r "$HOME/.bash_profile" ]]; then
  source "$HOME/.bash_profile"
elif [[ -r "$HOME/.bash_login" ]]; then
  source "$HOME/.bash_login"
elif [[ -r "$HOME/.profile" ]]; then
  source "$HOME/.profile"
fi
{prompt}
if (( BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1) )); then
  PROMPT_COMMAND=("${{PROMPT_COMMAND[@]}}" __ssh_workspace_prompt)
else
  PROMPT_COMMAND="${{PROMPT_COMMAND:+$PROMPT_COMMAND; }}__ssh_workspace_prompt"
fi"#,
        prompt = prompt_function(token),
    )
}

fn zsh_hook(token: &str) -> String {
    format!(
        "{}\nprecmd_functions=(\"${{precmd_functions[@]}}\" __ssh_workspace_prompt)",
        prompt_function(token)
    )
}

fn fish_init(token: &str) -> String {
    format!(
        r#"function __ssh_workspace_prompt --on-event fish_prompt
  set -l __ssh_workspace_status $status
  printf '\033]777;ssh-workspace;%s;%s;%s\007' {token} (string escape --style=url -- "$PWD") (string escape --style=url -- "$HOME")
  return $__ssh_workspace_status
end"#,
        token = quote(token),
    )
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    #[test]
    fn launcher_uses_temp_files_and_never_edits_user_profiles() {
        let command = launch_command("session-123", "/bin/bash");
        assert!(command.contains("__SSH_WORKSPACE_BOOTSTRAP__"));
        assert!(command.contains("--rcfile"));
        assert!(command.contains("--init-command"));
        assert!(command.contains("PROMPT_COMMAND"));
        assert!(!command.contains("> \"$HOME/"));
        assert!(!command.contains(">>"));
    }

    #[test]
    fn scalar_and_array_prompt_commands_are_preserved() {
        let rc = bash_rc("session-123");
        assert!(rc.contains("\"${PROMPT_COMMAND[@]}\""));
        assert!(rc.contains("${PROMPT_COMMAND:+$PROMPT_COMMAND; }"));
        assert!(rc.contains("return \"$__ssh_workspace_status\""));
    }

    #[test]
    fn probe_accepts_known_shells_only_and_ignores_banners_and_wrong_tokens() {
        let banner = "Welcome to the SSH server\n__SSH_WORKSPACE_SHELL__other;/bin/bash\n";
        assert_eq!(supported_shell(banner, "own"), None);
        for shell in ["/bin/bash", "/usr/bin/zsh", "/usr/local/bin/fish"] {
            assert_eq!(
                supported_shell(
                    &format!("{banner}__SSH_WORKSPACE_SHELL__own;{shell}\n"),
                    "own"
                ),
                Some(shell.into())
            );
        }
        for shell in [
            "/bin/sh",
            "/bin/csh",
            "/bin/tcsh",
            "bash",
            "",
            "/bin/bash\0",
        ] {
            assert_eq!(
                supported_shell(
                    &format!("{banner}__SSH_WORKSPACE_SHELL__own;{shell}\n"),
                    "own"
                ),
                None
            );
        }
    }

    #[test]
    fn real_bash_preserves_profiles_hooks_status_and_reports_actual_cwd() {
        let bash = if cfg!(windows) {
            std::path::Path::new("C:/Program Files/Git/bin/bash.exe")
        } else {
            std::path::Path::new("/bin/bash")
        };
        if !bash.is_file() {
            // Developers without Bash still run all parser/stream unit tests.
            return;
        }
        let probe = Command::new(bash)
            .args(["--noprofile", "--norc", "-c", &probe_command("real-probe")])
            .env("SHELL", "/bin/bash")
            .output()
            .unwrap();
        assert!(probe.status.success());
        assert_eq!(
            supported_shell(&String::from_utf8_lossy(&probe.stdout), "real-probe"),
            Some("/bin/bash".into())
        );
        for hook in [
            r#"PROMPT_COMMAND='printf "[existing:%s]" "$?"'"#,
            r#"PROMPT_COMMAND=('printf "[existing:%s]" "$?"')"#,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut home = temp.path().to_string_lossy().replace('\\', "/");
            if cfg!(windows) {
                home = format!("/{}{}", home[..1].to_lowercase(), &home[2..]);
            }
            let special = format!("{home}/中文 空格%;");
            std::fs::create_dir(temp.path().join("中文 空格%;")).unwrap();
            std::fs::write(
                temp.path().join(".bash_profile"),
                format!("PS1='TEST> '\nprintf '[profile]'\n{hook}\nalias goto_tmp='cd /tmp'\n"),
            )
            .unwrap();
            let commands = format!(
                "false\nprintf '[status:%s]\\n' \"$?\"\ngoto_tmp\ncd -- {}\ncd /no/such/ssh-workspace-directory\npwd\nexit\n",
                quote(&special)
            );
            let mut child = Command::new(bash)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    &launch_command("real-bash-test", "/bin/bash"),
                ])
                .env("HOME", &home)
                .env("SHELL", "/bin/bash")
                .env("TERM", "xterm-256color")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(commands.as_bytes())
                .unwrap();
            let result = child.wait_with_output().unwrap();
            assert!(result.status.success());
            let output = String::from_utf8_lossy(&result.stdout);
            assert_eq!(output.matches("[profile]").count(), 1, "{output}");
            assert!(output.contains("[existing:1]"), "{output}");
            assert!(output.contains("[status:1]"), "{output}");
            assert!(output.contains(";real-bash-test;/tmp;"), "{output}");
            let escaped = special.replace('%', "%25").replace(';', "%3B");
            assert!(
                output
                    .matches(&format!(";real-bash-test;{escaped};{home}\x07"))
                    .count()
                    >= 3,
                "{output}"
            );
        }
    }
}
