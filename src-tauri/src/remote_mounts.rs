//! Optional Linux mount metadata. Queries never use the interactive PTY.
use crate::{
    blocking, connection,
    model::{AppError, AppResult},
    ssh,
    ssh_transport::Cancellation,
    State,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{State as TauriState, WebviewWindow};

const LIMIT: usize = 2 * 1024 * 1024;
const QUERY_TIMEOUT: Duration = Duration::from_secs(6);
static QUERIES: AtomicUsize = AtomicUsize::new(0);
struct QuerySlot;
impl QuerySlot {
    fn acquire(check: &impl Fn() -> AppResult<()>) -> AppResult<Self> {
        let deadline = Instant::now() + QUERY_TIMEOUT;
        loop {
            check()?;
            let current = QUERIES.load(Ordering::SeqCst);
            if current < 2
                && QUERIES
                    .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                return Ok(Self);
            }
            if Instant::now() >= deadline {
                return Err(AppError::new("busy", "挂载查询正在进行，请稍后重试"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for QuerySlot {
    fn drop(&mut self) {
        QUERIES.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMount {
    pub(crate) key: String,
    pub(crate) target: String,
    pub(crate) source: String,
    pub(crate) fs_root: String,
    pub(crate) fs_type: String,
    pub(crate) read_only: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountTable {
    pub(crate) version: String,
    pub(crate) status: &'static str,
    pub(crate) mounts: Vec<RemoteMount>,
    pub(crate) issues: Vec<MountIssue>,
    pub(crate) message: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountIssue {
    pub(crate) kind: &'static str,
    pub(crate) target: String,
    pub(crate) layers: usize,
}
#[derive(Clone, Debug)]
struct ParsedMount {
    id: u64,
    parent_id: u64,
    mount: RemoteMount,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountUsage {
    pub(crate) mount_key: String,
    pub(crate) status: &'static str,
    pub(crate) total_bytes: Option<String>,
    pub(crate) used_bytes: Option<String>,
    pub(crate) available_bytes: Option<String>,
    pub(crate) used_percent: Option<u32>,
    pub(crate) updated_at: Option<u64>,
    pub(crate) error: Option<String>,
}
impl MountUsage {
    fn unavailable(key: &str, status: &'static str, message: &str) -> Self {
        Self {
            mount_key: key.into(),
            status,
            total_bytes: None,
            used_bytes: None,
            available_bytes: None,
            used_percent: None,
            updated_at: None,
            error: Some(message.into()),
        }
    }
}

fn unescape(value: &str) -> AppResult<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let escaped = match bytes.get(i + 1..i + 4) {
                Some(b"040") => b' ',
                Some(b"011") => b'\t',
                Some(b"012") => b'\n',
                Some(b"134") => b'\\',
                _ => return Err(AppError::new("mountFormat", "挂载路径转义无效")),
            };
            output.push(escaped);
            i += 4;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(output).map_err(|_| AppError::new("mountFormat", "挂载路径不是有效的 UTF-8"))
}
fn parse_table(text: &str) -> AppResult<MountTable> {
    parse_mounts(text, true)
}
fn visible_mounts(parsed: &[ParsedMount]) -> (Vec<RemoteMount>, Vec<MountIssue>) {
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, parsed_mount) in parsed.iter().enumerate() {
        groups
            .entry(parsed_mount.mount.target.as_str())
            .or_default()
            .push(index);
    }

    let mut selected = HashSet::new();
    let mut issues = Vec::new();
    let mut handled = HashSet::new();
    for parsed_mount in parsed {
        let target = parsed_mount.mount.target.as_str();
        if !handled.insert(target) {
            continue;
        }
        let indices = &groups[target];
        if indices.len() == 1 {
            selected.insert(indices[0]);
            continue;
        }

        let mut by_id = HashMap::new();
        let mut child_by_parent = HashMap::new();
        let mut valid = true;
        for &index in indices {
            if by_id.insert(parsed[index].id, index).is_some() {
                valid = false;
            }
        }
        for &index in indices {
            let parent_id = parsed[index].parent_id;
            if by_id.contains_key(&parent_id) && child_by_parent.insert(parent_id, index).is_some()
            {
                valid = false;
            }
        }
        let tops: Vec<_> = indices
            .iter()
            .copied()
            .filter(|index| !child_by_parent.contains_key(&parsed[*index].id))
            .collect();
        if tops.len() != 1 {
            valid = false;
        }
        if valid {
            let top = tops[0];
            let mut current = top;
            let mut visited = HashSet::new();
            loop {
                if !visited.insert(parsed[current].id) {
                    valid = false;
                    break;
                }
                let parent_id = parsed[current].parent_id;
                let Some(parent) = by_id.get(&parent_id) else {
                    break;
                };
                current = *parent;
            }
            if visited.len() != indices.len() {
                valid = false;
            }
            if valid {
                selected.insert(top);
            }
        }
        if !valid {
            issues.push(MountIssue {
                kind: "ambiguousStack",
                target: target.into(),
                layers: indices.len(),
            });
        }
    }

    (
        parsed
            .iter()
            .enumerate()
            .filter(|(index, _)| selected.contains(index))
            .map(|(_, parsed_mount)| parsed_mount.mount.clone())
            .collect(),
        issues,
    )
}
fn parse_mounts(text: &str, for_display: bool) -> AppResult<MountTable> {
    let mut parsed = Vec::new();
    for line in text.lines() {
        let (left, right) = line
            .split_once(" - ")
            .ok_or_else(|| AppError::new("mountFormat", "挂载表格式无效"))?;
        let fields: Vec<_> = left.split(' ').collect();
        let extra: Vec<_> = right.split(' ').collect();
        if fields.len() < 6 || extra.len() < 3 {
            return Err(AppError::new("mountFormat", "挂载表字段无效"));
        }
        let id = fields[0]
            .parse::<u64>()
            .map_err(|_| AppError::new("mountFormat", "挂载表字段无效"))?;
        let parent_id = fields[1]
            .parse::<u64>()
            .map_err(|_| AppError::new("mountFormat", "挂载表字段无效"))?;
        let target = unescape(fields[4])?;
        if !target.starts_with('/') || target.contains('\0') {
            return Err(AppError::new("mountFormat", "挂载位置无效"));
        }
        parsed.push(ParsedMount {
            id,
            parent_id,
            mount: RemoteMount {
                key: fields[0].into(),
                target,
                source: unescape(extra[1])?,
                fs_root: unescape(fields[3])?,
                fs_type: extra[0].into(),
                read_only: fields[5]
                    .split(',')
                    .chain(extra[2].split(','))
                    .any(|v| v == "ro"),
            },
        });
    }
    if parsed.is_empty() {
        return Err(AppError::new("mountFormat", "挂载表为空"));
    }
    // Stacked mounts form a parent chain at the same target. Select the only
    // topmost node when the complete chain can be proven; never rely on line order.
    let (mounts, issues) = if for_display {
        visible_mounts(&parsed)
    } else {
        (
            parsed.into_iter().map(|parsed| parsed.mount).collect(),
            Vec::new(),
        )
    };
    Ok(MountTable {
        version: format!("{:x}", Sha256::digest(text.as_bytes())),
        status: "ready",
        mounts,
        issues,
        message: None,
    })
}
fn usage(key: &str, output: &str) -> AppResult<MountUsage> {
    let fields = output
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::new("mountFormat", "文件系统容量格式无效"))?;
    let [size, blocks, free, available] = fields.as_slice() else {
        return Err(AppError::new("mountFormat", "文件系统容量字段无效"));
    };
    if *size == 0 || free > blocks || available > free {
        return Err(AppError::new("mountFormat", "文件系统容量数值无效"));
    }
    if *blocks == 0 {
        return Ok(MountUsage::unavailable(
            key,
            "unsupported",
            "此文件系统不提供常规容量",
        ));
    }
    let used = u128::from(blocks - free);
    let denominator = used + u128::from(*available);
    let bytes = |n: u128| (n * u128::from(*size)).to_string();
    Ok(MountUsage {
        mount_key: key.into(),
        status: "ready",
        total_bytes: Some(bytes(u128::from(*blocks))),
        used_bytes: Some(bytes(used)),
        available_bytes: Some(bytes(u128::from(*available))),
        used_percent: (denominator != 0).then(|| ((used * 100).div_ceil(denominator)) as u32),
        updated_at: Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        ),
        error: None,
    })
}
pub(crate) fn has_capacity(fs: &str) -> bool {
    !matches!(
        fs,
        "proc"
            | "sysfs"
            | "cgroup"
            | "cgroup2"
            | "devpts"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "configfs"
            | "pstore"
            | "mqueue"
            | "hugetlbfs"
            | "autofs"
            | "fusectl"
            | "binfmt_misc"
            | "rpc_pipefs"
    )
}

fn deadline_check(deadline: Instant, check: &impl Fn() -> AppResult<()>) -> AppResult<()> {
    check()?;
    if Instant::now() >= deadline {
        return Err(AppError::new("timeout", "挂载查询超时"));
    }
    Ok(())
}
fn retry<T>(
    deadline: Instant,
    check: &impl Fn() -> AppResult<()>,
    mut call: impl FnMut() -> Result<T, ssh2::Error>,
) -> AppResult<T> {
    loop {
        deadline_check(deadline, check)?;
        match call() {
            Ok(result) => return Ok(result),
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
}
fn exec(
    session: &Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    session.set_blocking(false);
    let result = (|| {
        let deadline = Instant::now() + QUERY_TIMEOUT;
        let mut channel = retry(deadline, check, || session.channel_session())?;
        retry(deadline, check, || channel.exec(command))?;
        let mut output = [Vec::new(), Vec::new()];
        let mut buffer = [0; 8192];
        loop {
            deadline_check(deadline, check)?;
            let mut received = false;
            for (i, bytes) in output.iter_mut().enumerate() {
                match channel.stream(i as i32).read(&mut buffer) {
                    Ok(n) => {
                        received |= n > 0;
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
                if bytes.len() > LIMIT {
                    return Err(AppError::new("limit", "挂载查询输出过大"));
                }
            }
            if channel.eof() && !received {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        retry(deadline, check, || channel.wait_close())?;
        Ok(ssh::ExecResult {
            code: channel.exit_status()?,
            stdout: String::from_utf8(output[0].clone())
                .map_err(|_| AppError::new("mountFormat", "挂载查询输出编码无效"))?,
            stderr: String::from_utf8_lossy(&output[1]).into_owned(),
        })
    })();
    session.set_blocking(true);
    result
}
fn sftp_mountinfo(session: &Session, check: &impl Fn() -> AppResult<()>) -> AppResult<String> {
    session.set_blocking(false);
    let result = (|| {
        let deadline = Instant::now() + QUERY_TIMEOUT;
        let sftp = retry(deadline, check, || session.sftp())?;
        let mut file = retry(deadline, check, || {
            sftp.open(Path::new("/proc/self/mountinfo"))
        })?;
        let mut bytes = Vec::new();
        // libssh2 requests up to 4 * buffer.len() bytes ahead, splitting them
        // into 30,000-byte SFTP READ packets. /proc seq_file reads may return a
        // short page even before EOF. Multiple queued offsets then skip data
        // (or return EOF early). Keep read-ahead below one packet so the next
        // offset is based on bytes actually received, including short reads.
        let mut buffer = [0; 4096];
        loop {
            deadline_check(deadline, check)?;
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e.into()),
            }
            if bytes.len() > LIMIT {
                return Err(AppError::new("limit", "挂载表过大"));
            }
        }
        String::from_utf8(bytes).map_err(|_| AppError::new("mountFormat", "挂载表编码无效"))
    })();
    session.set_blocking(true);
    result
}
fn verified_table(session: &Session, check: &impl Fn() -> AppResult<()>) -> AppResult<MountTable> {
    verified_mounts(session, check, true)
}
fn verified_mounts(
    session: &Session,
    check: &impl Fn() -> AppResult<()>,
    for_display: bool,
) -> AppResult<MountTable> {
    let output = exec(
        session,
        "test \"$(uname -s)\" = Linux && cat /proc/self/mountinfo",
        check,
    )?;
    if output.code != 0 {
        return Err(AppError::new(
            "unsupported",
            "挂载信息需要 Linux 远端及可读取的 /proc/self/mountinfo",
        ));
    }
    let sftp = sftp_mountinfo(session, check).map_err(|e| {
        if matches!(
            e.kind.as_str(),
            "timeout" | "disconnected" | "permission" | "cancelled"
        ) {
            e
        } else {
            AppError::new("unsupported", "无法确认 SFTP 与命令执行的挂载视图一致")
        }
    })?;
    if output.stdout != sftp {
        return Err(AppError::new(
            "mountChanged",
            "挂载表正在变化或 SFTP 与命令执行的路径视图不同",
        ));
    }
    if for_display {
        parse_table(&sftp)
    } else {
        parse_mounts(&sftp, false)
    }
}

#[tauri::command]
pub async fn list_remote_mounts(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
) -> AppResult<MountTable> {
    let session_id = state.context_key(&context, window.label())?;
    crate::moves::workspace_only(&window)?;
    let spec = state.workspace_context(&session_id, window.label())?;
    let owner = window.label().to_owned();
    let state = state.inner().clone();
    blocking(move || {
        let check = || state.workspace_context(&session_id, &owner).map(|_| ());
        query_table(&spec, &Cancellation::default(), &check)
    })
    .await
}

#[tauri::command]
pub async fn get_remote_mount_usage(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    mount_table_version: String,
    mount_keys: Vec<String>,
) -> AppResult<Vec<MountUsage>> {
    let session_id = state.context_key(&context, window.label())?;
    crate::moves::workspace_only(&window)?;
    let spec = state.workspace_context(&session_id, window.label())?;
    if mount_keys.is_empty() || mount_keys.len() > 16 {
        return Err(AppError::new("validation", "每次查询需要 1 至 16 个挂载点"));
    }
    let owner = window.label().to_owned();
    let state = state.inner().clone();
    blocking(move || {
        let check = || state.workspace_context(&session_id, &owner).map(|_| ());
        query_usage(
            &spec,
            &mount_table_version,
            mount_keys,
            &Cancellation::default(),
            &check,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    const TABLE: &str = "20 1 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n21 20 8:2 / /data rw - xfs /dev/sdb1 rw\n22 20 8:2 /projects /srv/projects ro - xfs /dev/sdb1 rw\n23 21 0:9 / /data/archive rw - nfs4 nas:/archive rw\n";
    #[test]
    fn parses_nested_bind_and_read_only_mounts() {
        let table = parse_table(TABLE).unwrap();
        assert_eq!(table.mounts.len(), 4);
        assert_eq!(table.mounts[2].fs_root, "/projects");
        assert!(table.mounts[2].read_only);
        assert_eq!(table.mounts[3].target, "/data/archive");
        assert_ne!(
            table.version,
            parse_table(&TABLE.replace("/dev/sdb1", "/dev/sdc1"))
                .unwrap()
                .version
        );
    }
    #[test]
    fn decodes_paths_without_double_decoding() {
        let table = parse_table(
            "1 0 8:1 /中文 /my\\040disk\\011tab\\012line\\134040 rw - ext4 /dev/test rw\n",
        )
        .unwrap();
        assert_eq!(table.mounts[0].target, "/my disk\ttab\nline\\040");
        assert_eq!(table.mounts[0].fs_root, "/中文");
        assert!(unescape("/bad\\999").is_err());
    }
    #[test]
    fn selects_topmost_stacked_mount_from_parent_chain() {
        let table = parse_table(&format!(
            "{TABLE}24 21 8:3 / /data rw - ext4 /dev/sdc1 rw\n"
        ))
        .unwrap();
        let data = table.mounts.iter().find(|m| m.target == "/data").unwrap();
        assert_eq!(data.key, "24");
        assert_eq!(data.source, "/dev/sdc1");
        assert!(table.issues.is_empty());
    }
    #[test]
    fn resolves_systemd_automount_stack_independent_of_line_order() {
        let lower = "38 26 0:33 / /proc/sys/fs/binfmt_misc rw - autofs systemd-1 rw\n";
        let upper = "108 38 0:37 / /proc/sys/fs/binfmt_misc rw - binfmt_misc binfmt_misc rw\n";
        for text in [format!("{lower}{upper}"), format!("{upper}{lower}")] {
            let table = parse_table(&text).unwrap();
            assert_eq!(table.mounts.len(), 1);
            assert_eq!(table.mounts[0].key, "108");
            assert_eq!(table.mounts[0].fs_type, "binfmt_misc");
            assert!(table.issues.is_empty());
        }
    }
    #[test]
    fn reports_ambiguous_stacked_mount_without_guessing() {
        let table = parse_table(&format!(
            "{TABLE}24 20 8:3 / /data rw - ext4 /dev/sdc1 rw\n"
        ))
        .unwrap();
        assert!(!table.mounts.iter().any(|m| m.target == "/data"));
        assert_eq!(table.issues.len(), 1);
        assert_eq!(table.issues[0].kind, "ambiguousStack");
        assert_eq!(table.issues[0].target, "/data");
        assert_eq!(table.issues[0].layers, 2);
        let complete = parse_mounts(
            &format!("{TABLE}24 20 8:3 / /data rw - ext4 /dev/sdc1 rw\n"),
            false,
        )
        .unwrap();
        assert_eq!(
            complete
                .mounts
                .iter()
                .filter(|m| m.target == "/data")
                .count(),
            2
        );
    }
    #[test]
    fn capacity_uses_available_blocks_and_keeps_large_integers() {
        let value = usage("1", "4096 100 40 30\n").unwrap();
        assert_eq!(value.used_percent, Some(67));
        assert_eq!(value.total_bytes.as_deref(), Some("409600"));
        assert_eq!(value.available_bytes.as_deref(), Some("122880"));
        assert_eq!(
            usage("1", "4096 9007199254740993 0 0")
                .unwrap()
                .total_bytes
                .unwrap(),
            "36893488147419107328"
        );
        assert_eq!(usage("1", "4096 0 0 0").unwrap().status, "unsupported");
        assert!(usage("1", "4096 10 11 0").is_err());
        assert!(usage("1", "4096 10 5 6").is_err());
        assert!(usage("1", "injected stdout").is_err());
        assert!(!has_capacity("autofs"));
        assert!(has_capacity("tmpfs"));
    }

    #[test]
    #[ignore = "requires the isolated OpenSSH mount fixture"]
    fn real_ssh_mount_table_and_capacity() {
        let host = std::env::var("MOUNT_TEST_HOST").expect("MOUNT_TEST_HOST");
        let socket = std::net::TcpStream::connect((host.as_str(), 22)).unwrap();
        let mut session = Session::new().unwrap();
        session.set_tcp_stream(socket);
        session.set_timeout(6000);
        session.handshake().unwrap();
        session
            .userauth_password("ops", "fixture-password")
            .unwrap();
        let raw = exec(&session, "cat /proc/self/mountinfo", &|| Ok(())).unwrap();
        assert!(
            raw.stdout.len() > 8192,
            "fixture must exercise multi-page procfs reads"
        );
        let via_sftp = sftp_mountinfo(&session, &|| Ok(())).unwrap();
        assert!(
            raw.stdout == via_sftp,
            "mountinfo differs: exec={} bytes, SFTP={} bytes",
            raw.stdout.len(),
            via_sftp.len()
        );
        let table = verified_table(&session, &|| Ok(())).unwrap();
        let expected_mounts = std::env::var("MOUNT_TEST_MOUNT_COUNT")
            .unwrap_or_else(|_| "48".into())
            .parse::<usize>()
            .unwrap();
        assert_eq!(
            table
                .mounts
                .iter()
                .filter(|m| m.target.starts_with("/mnt/check-many/"))
                .count(),
            expected_mounts
        );
        let memory = table
            .mounts
            .iter()
            .find(|m| m.target == "/mnt/mount-check")
            .unwrap();
        assert_eq!(memory.fs_type, "tmpfs");
        let readonly = table
            .mounts
            .iter()
            .find(|m| m.target == "/mnt/mount-ro")
            .unwrap();
        assert!(readonly.read_only);
        assert!(table.mounts.iter().any(|m| m.target == "/"));
        let response = exec(
            &session,
            "LC_ALL=C stat -f -c '%S %b %f %a' -- /mnt/mount-check",
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(response.code, 0);
        let capacity = usage(&memory.key, &response.stdout).unwrap();
        assert_eq!(capacity.status, "ready");
        assert_eq!(capacity.total_bytes.as_deref(), Some("33554432"));
        let second = verified_table(&session, &|| Ok(())).unwrap();
        assert_eq!(table.version, second.version);
        assert!(exec(&session, "sleep 9", &|| Ok(())).is_err());
        // Timing out a separate exec channel does not prevent subsequent SFTP queries.
        assert!(verified_table(&session, &|| Ok(())).is_ok());
    }
}

// Deletion must retain every boundary, including stacked and bind mounts hidden in the UI.
pub(crate) fn deletion_boundaries(
    session: &Session,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, Vec<String>)> {
    let table = verified_mounts(session, check, false)?;
    Ok((
        table.version,
        table.mounts.into_iter().map(|m| m.target).collect(),
    ))
}

pub(crate) fn query_table(
    spec: &connection::SharedContext,
    cancel: &Cancellation,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<MountTable> {
    let _slot = QuerySlot::acquire(check)?;
    check()?;
    let connection =
        connection::connect_verified(spec, cancel, crate::traffic::TrafficPurpose::MountMonitor)?;
    match verified_table(connection.session(), check) {
        Err(e) if e.kind == "unsupported" => Ok(MountTable {
            version: String::new(),
            status: "unsupported",
            mounts: vec![],
            issues: vec![],
            message: Some(e.message),
        }),
        result => result,
    }
}

pub(crate) fn query_usage(
    spec: &connection::SharedContext,
    mount_table_version: &str,
    mount_keys: Vec<String>,
    cancel: &Cancellation,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<MountUsage>> {
    let _slot = QuerySlot::acquire(check)?;
    check()?;
    let connection =
        connection::connect_verified(spec, cancel, crate::traffic::TrafficPurpose::MountMonitor)?;
    let session = connection.session();
    let table = verified_table(session, check)?;
    if table.version != mount_table_version {
        return Err(AppError::new("mountChanged", "挂载关系已变化，请刷新"));
    }
    let keys: HashSet<_> = mount_keys.into_iter().collect();
    if keys
        .iter()
        .any(|key| !table.mounts.iter().any(|m| &m.key == key))
    {
        return Err(AppError::new("mountChanged", "挂载点已失效，请刷新"));
    }
    let mut results = Vec::new();
    for mount in table.mounts.iter().filter(|m| keys.contains(&m.key)) {
        check()?;
        if !has_capacity(&mount.fs_type) {
            results.push(MountUsage::unavailable(
                &mount.key,
                "unsupported",
                "此文件系统不提供常规容量",
            ));
            continue;
        }
        // Only numeric output is parsed. Paths are quoted as one shell argument.
        let result = exec(
            session,
            &format!(
                "LC_ALL=C stat -f -c '%S %b %f %a' -- {}",
                ssh::quote(&mount.target)
            ),
            &check,
        )
        .and_then(|output| {
            if output.code == 0 {
                usage(&mount.key, &output.stdout)
            } else {
                Err(AppError::new(
                    "mountUsage",
                    "无法读取容量，远端需要支持 stat -f -c 且当前用户能够访问挂载点",
                ))
            }
        });
        results
            .push(result.unwrap_or_else(|e| {
                MountUsage::unavailable(&mount.key, "unavailable", &e.message)
            }));
    }
    // A mount may have been replaced while stat was in flight.
    let after = exec(session, "cat /proc/self/mountinfo", &check)?;
    if after.code != 0 || parse_table(&after.stdout)?.version != table.version {
        return Err(AppError::new(
            "mountChanged",
            "查询期间挂载关系已变化，请刷新",
        ));
    }
    check()?;
    Ok(results)
}
