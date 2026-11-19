use super::{ExitMode, Mode, Program};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::io::Write;
fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
        } else {
            if c == '\"' {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
            } else {
                out.push_str(&"\\".repeat(slashes));
            }
            out.push(c);
            slashes = 0;
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}
fn arguments(args: &[String]) -> String {
    args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}
fn encoded(launcher: &str, script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "{} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        launcher.rsplit(['\\', '/']).next().unwrap(),
        STANDARD.encode(utf16)
    )
}
pub(super) fn encode(launcher: &str, exit_mode: ExitMode, mode: Mode, p: &Program) -> String {
    let config = json!({"file":p.file,"line":arguments(&p.argv),"remove":p.remove_env,"terminal":mode==Mode::Terminal,
        "checks":p.checks.iter().map(|c|json!({"line":arguments(&c.argv),"expected":c.expected_stdout})).collect::<Vec<_>>()});
    let script =
        include_str!("windows.ps1").replace("__CONFIG__", &config.to_string().replace('\'', "''"));
    // Reuse the installed compressor to fit cmd's outer command-line limit.
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gzip.write_all(script.as_bytes()).unwrap();
    let payload = STANDARD.encode(gzip.finish().unwrap());
    let loader=format!("$ProgressPreference='SilentlyContinue';$b=[Convert]::FromBase64String('{payload}');$m=[IO.MemoryStream]::new($b,0,$b.Length);$z=[IO.Compression.GZipStream]::new($m,[IO.Compression.CompressionMode]::Decompress);Invoke-Expression ([IO.StreamReader]::new($z)).ReadToEnd()");
    let command = encoded(launcher, &loader);
    match exit_mode {
        ExitMode::Native => command,
        // PowerShell's native pipeline rewrites bytes and reduces nonzero exit codes to 1.
        ExitMode::Powershell => {
            let arguments = command.split_once(' ').unwrap().1;
            format!("$s=[Diagnostics.ProcessStartInfo]::new();$s.FileName='{}';$s.Arguments='{arguments}';$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$p.WaitForExit();exit $p.ExitCode",launcher.replace('\'',"''"))
        }
    }
}
pub(super) fn probe(keys: &[String]) -> String {
    let names = keys
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",");
    let script=format!("$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);[Console]::WriteLine('SODECK_ENV_1');[Console]::WriteLine('windows');[Console]::WriteLine([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((Get-Process -Id $PID).Path)));foreach($n in @({names})){{[Console]::WriteLine([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string][Environment]::GetEnvironmentVariable($n))))}};exit 37");
    encoded("powershell.exe", &script)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn docker_inspect_fits_the_windows_outer_command_budget() {
        let format = r#"{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Config.Image}},"imageId":{{json .Image}},"status":{{json .State.Status}},"exitCode":{{json .State.ExitCode}},"health":{{with index .State "Health"}}{{json .Status}}{{else}}null{{end}},"labels":{{json .Config.Labels}},"startedAt":{{json .State.StartedAt}}}"#;
        let host = "npipe:////./pipe/dockerDesktopLinuxEngine";
        let p = Program {
            file: "docker.exe".into(),
            argv: [
                "--host",
                host,
                "container",
                "inspect",
                "--format",
                format,
                &"a".repeat(64),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            remove_env: vec!["DOCKER_HOST".into(), "DOCKER_CONTEXT".into()],
            checks: vec![super::super::Check {
                argv: ["--host", host, "info", "--format", "{{.ID}}"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                expected_stdout: "4452f872-bafb-4190-b289-74bdfcf9a518".into(),
            }],
        };
        let command = super::super::encode(
            &super::super::Environment::Windows {
                launcher: "powershell.exe".into(),
                exit_mode: ExitMode::Native,
            },
            Mode::NonInteractive,
            &p,
        )
        .unwrap();
        assert!(command.len() <= 8190);
    }
    #[test]
    fn windows_arguments_preserve_quotes_and_trailing_slashes() {
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("a\\\"b\\"), "\"a\\\\\\\"b\\\\\"");
    }
    #[cfg(windows)]
    #[test]
    fn native_cmd_program_preserves_arguments_bytes_and_exit() {
        let args = vec![
            "".into(),
            "空 格".into(),
            "a\\\"b\\".into(),
            "$();&<>|".into(),
            "SDP1 1\nx".into(),
            "a'b".into(),
        ];
        let source="process.stdout.write(JSON.stringify(process.argv.slice(1)));process.stderr.write(Buffer.from([0,255,13,10]));process.exit(37)";
        let mut argv = vec!["-e".into(), source.into(), "--".into()];
        argv.extend(args.clone());
        let p = Program {
            file: "node.exe".into(),
            argv,
            remove_env: vec![],
            checks: vec![],
        };
        let cmd = super::super::encode(
            &super::super::Environment::Windows {
                launcher: "powershell.exe".into(),
                exit_mode: ExitMode::Native,
            },
            Mode::NonInteractive,
            &p,
        )
        .unwrap();
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/S", "/C", &cmd])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut d = super::super::Decoder::new(true, 4096);
        let chunks = d
            .push(true, &output.stderr)
            .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(&output.stderr)));
        assert_eq!(d.started, Some(true));
        assert_eq!(chunks[1].1, [0, 255, 13, 10]);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&output.stdout).unwrap(),
            args
        );
        let cmd = super::super::encode(
            &super::super::Environment::Windows {
                launcher: "powershell.exe".into(),
                exit_mode: ExitMode::Powershell,
            },
            Mode::NonInteractive,
            &p,
        )
        .unwrap();
        let utf16: Vec<u8> = cmd.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &STANDARD.encode(utf16),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut d = super::super::Decoder::new(true, 4096);
        let chunks = d.push(true, &output.stderr).unwrap();
        assert_eq!(chunks[1].1, [0, 255, 13, 10]);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&output.stdout).unwrap(),
            args
        );
    }
    #[cfg(windows)]
    #[test]
    fn native_probe_distinguishes_shell_exit_and_byte_semantics() {
        let keys = vec!["DOCKER_HOST".into()];
        let command = probe(&keys);
        let out = std::process::Command::new("cmd.exe")
            .args(["/D", "/S", "/C", &command])
            .output()
            .unwrap();
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            super::super::parse_environment(&out.stdout, &keys, out.status.code().unwrap() as u32)
                .unwrap()["environment"]["exitMode"],
            "native"
        );
        let utf16: Vec<u8> = command.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let out = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &STANDARD.encode(utf16),
            ])
            .output()
            .unwrap();
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            super::super::parse_environment(&out.stdout, &keys, out.status.code().unwrap() as u32)
                .unwrap()["environment"]["exitMode"],
            "powershell"
        );
    }
    #[cfg(windows)]
    #[test]
    fn native_checks_fail_without_starting_the_main_program() {
        let p = Program {
            file: "node.exe".into(),
            argv: vec!["-e".into(), "process.stdout.write('should not run')".into()],
            remove_env: vec![],
            checks: vec![super::super::Check {
                argv: vec!["-e".into(), "process.stdout.write('wrong\\n')".into()],
                expected_stdout: "right".into(),
            }],
        };
        let cmd = super::super::encode(
            &super::super::Environment::Windows {
                launcher: "powershell.exe".into(),
                exit_mode: ExitMode::Native,
            },
            Mode::NonInteractive,
            &p,
        )
        .unwrap();
        let out = std::process::Command::new("cmd.exe")
            .args(["/D", "/S", "/C", &cmd])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        let mut d = super::super::Decoder::new(true, 4096);
        d.push(true, &out.stderr)
            .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(d.started, Some(false));
        assert_eq!(d.failure.unwrap()["index"], 1);
    }
}
