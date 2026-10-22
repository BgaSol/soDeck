use super::*;
pub(super) fn stable_version(value: &str) -> AppResult<semver::Version> {
    semver::Version::parse(value)
        .ok()
        .filter(|v| v.pre.is_empty() && v.build.is_empty() && v.to_string() == value)
        .ok_or_else(|| error("清单包含无效的正式版本。"))
}
pub(super) fn stable_newer(current: &str, next: &str) -> bool {
    matches!((semver::Version::parse(current), stable_version(next)), (Ok(a), Ok(b)) if b > a)
}
pub(super) fn backoff(failures: u32) -> u64 {
    match failures {
        0 => DAY,
        1 => 900,
        2 => 3600,
        _ => 21600,
    }
}
pub(super) fn auto_download(saved: &Saved, candidate: &Candidate, healthy: bool) -> bool {
    healthy
        && saved.auto_download
        && candidate.capability.mode == "inApp"
        && saved.skipped_version.as_deref() != Some(&candidate.release.version)
        && saved.suppressed_version.as_deref() != Some(&candidate.release.version)
}
pub(super) fn startup_result(saved: &mut Saved, actual: &str) -> bool {
    let Some(pending) = saved.pending_install.take() else {
        return false;
    };
    let completed = matches!((stable_version(actual), stable_version(&pending.target_version)), (Ok(a), Ok(b)) if a >= b);
    saved.unread_result = Some(InstallResult {
        id: pending.id,
        target_version: pending.target_version,
        outcome: if completed { "completed" } else { "incomplete" }.into(),
    });
    true
}
