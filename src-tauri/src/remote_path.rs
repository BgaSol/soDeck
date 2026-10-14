use crate::model::{AppError, AppResult};

pub fn validate_host(host: &str) -> AppResult<()> {
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('-')
        || (host.contains(':') && host.parse::<std::net::Ipv6Addr>().is_err())
        || host
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || ".-:_".contains(c)))
    {
        return Err(AppError::new(
            "validation",
            "真实地址必须是 IP 或域名，不包含端口、空格或路径",
        ));
    }
    Ok(())
}

pub fn component(name: &str) -> AppResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(AppError::new(
            "path",
            "名称不能为空，也不能包含 /、空字符或使用 . 和 ..",
        ));
    }
    Ok(())
}

pub fn absolute(path: &str) -> AppResult<&str> {
    if !path.starts_with('/')
        || path.len() > 16384
        || path.contains('\0')
        || path.split('/').any(|p| p == "..")
    {
        return Err(AppError::new("path", "远程操作需要明确的绝对路径"));
    }
    Ok(path)
}

pub fn join(parent: &str, name: &str) -> AppResult<String> {
    absolute(parent)?;
    component(name)?;
    Ok(format!("{}/{}", parent.trim_end_matches('/'), name))
}

pub fn parent(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/" } else { p })
        .unwrap_or("/")
}

pub fn name(path: &str) -> AppResult<&str> {
    let value = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    component(value)?;
    Ok(value)
}

pub fn renamed(name: &str, directory: bool, suffix: u32) -> String {
    if !directory {
        if let Some((stem, extension)) = name.rsplit_once('.') {
            if !stem.is_empty() {
                return format!("{stem} ({suffix}).{extension}");
            }
        }
    }
    format!("{name} ({suffix})")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn posix_names_are_not_desktop_names() {
        for name in ["中文 文件", "a'b", "a\\b", "a:b", "a\nb", "$(command)"] {
            component(name).unwrap();
        }
        for name in ["", ".", "..", "a/b", "a\0b"] {
            assert!(component(name).is_err());
        }
        assert_eq!(renamed(".env", false, 1), ".env (1)");
        assert_eq!(renamed("a.txt", false, 1), "a (1).txt");
        for host in ["192.0.2.5", "host.internal", "2001:db8::7"] {
            validate_host(host).unwrap();
        }
        for host in [
            "host:22",
            "192.0.2.5:22",
            "[::1]",
            "-oProxyCommand",
            "host/path",
            "host name",
        ] {
            assert!(validate_host(host).is_err());
        }
    }
}
