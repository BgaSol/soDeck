use super::{Mode, Program};
pub(super) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn call(args: &[String]) -> String {
    format!(
        "\"$file\" {}",
        args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
    )
}
fn emit(started: bool, index: usize) -> String {
    // Only generated control bytes are interpreted by printf; arguments remain quoted data.
    let frame = super::startup(started, index);
    let value = String::from_utf8(frame).unwrap();
    format!("printf '%s' {} >&2; ", quote(&value))
}
pub(super) fn encode(mode: Mode, p: &Program) -> String {
    let mut script = String::new();
    if !p.remove_env.is_empty() {
        script.push_str(&format!("unset {}; ", p.remove_env.join(" ")));
    }
    script.push_str(&format!(
        "file=$(command -v -- {}) || file=; if [ ! -f \"$file\" ] || [ ! -x \"$file\" ]; then ",
        quote(&p.file)
    ));
    if mode == Mode::NonInteractive {
        let body =
            r#"{"v":1,"started":false,"error":{"message":"Program executable unavailable"}}"#;
        script.push_str(&format!(
            "printf '%s' {} >&2; ",
            quote(&format!("SDP1 {}\n{body}", body.len()))
        ));
    } else {
        script.push_str("printf '%s\\n' 'Program executable unavailable' >&2; ");
    }
    script.push_str("exit 127; fi; ");
    for (i, c) in p.checks.iter().enumerate() {
        let expected = quote(&c.expected_stdout);
        let check = call(&c.argv);
        script.push_str(&format!("actual=$({{ {check} 2>/dev/null; code=$?; printf '\\037%s' \"$code\"; }} | dd bs=1 count=4097 2>/dev/null); marker=$(printf '\\0370'); if [ \"$actual\" != {expected}\"$marker\" ] && [ \"$actual\" != {expected}\"\n$marker\" ] && [ \"$actual\" != {expected}\"$(printf '\\r')\n$marker\" ]; then "));
        if mode == Mode::NonInteractive {
            script.push_str(&emit(false, i + 1));
        } else {
            script.push_str("printf '%s\\n' 'Program identity check failed' >&2; ");
        }
        script.push_str("exit 1; fi; ");
    }
    if mode == Mode::NonInteractive {
        script.push_str(&emit(true, 0));
    }
    script.push_str(&format!("exec {}", call(&p.argv)));
    format!("sh -c {}", quote(&script))
}
pub(super) fn probe(keys: &[String]) -> String {
    let mut script =
        "test -n \"$(uname -s 2>/dev/null)\" || exit 1; printf '%s\\n' 'SODECK_ENV_1' 'posix'; "
            .to_string();
    for key in keys {
        script.push_str(&format!(
            "printf '%s' \"${{{key}-}}\" | base64 | tr -d '\\n'; printf '\\n'; "
        ));
    }
    format!("sh -c {}", quote(&script))
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn native_program_keeps_bytes_arguments_and_check_failures() {
        let args = vec!["".into(), "空 格'\\".into(), "$();&\n".into()];
        let mut argv=vec!["-e".into(),"process.stdout.write(JSON.stringify(process.argv.slice(1)));process.stderr.write(Buffer.from([0,255,13,10]));process.exit(37)".into(),"--".into()];
        argv.extend(args.clone());
        let mut p = Program {
            file: "node".into(),
            argv,
            remove_env: vec![],
            checks: vec![super::super::Check {
                argv: vec!["-e".into(), "process.stdout.write('identity\\n')".into()],
                expected_stdout: "identity".into(),
            }],
        };
        let output = std::process::Command::new("sh")
            .args(["-c", &encode(Mode::NonInteractive, &p)])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut d = super::super::Decoder::new(true, 4096);
        assert_eq!(d.push(true, &output.stderr).unwrap()[1].1, [0, 255, 13, 10]);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&output.stdout).unwrap(),
            args
        );
        p.checks[0].expected_stdout = "changed".into();
        let output = std::process::Command::new("sh")
            .args(["-c", &encode(Mode::NonInteractive, &p)])
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
        let mut d = super::super::Decoder::new(true, 4096);
        d.push(true, &output.stderr).unwrap();
        assert_eq!(d.failure.unwrap()["index"], 1);
        p.file = "sodeck-nonexistent-program".into();
        p.checks.clear();
        let output = std::process::Command::new("sh")
            .args(["-c", &encode(Mode::NonInteractive, &p)])
            .output()
            .unwrap();
        let mut d = super::super::Decoder::new(true, 4096);
        d.push(true, &output.stderr).unwrap();
        assert_eq!(d.started, Some(false));
    }
}
