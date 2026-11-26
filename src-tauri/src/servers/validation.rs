use super::*;

pub(super) fn host(value: &str) -> Result<String, String> {
    let h = value.trim();
    let h = if h.starts_with('[') && h.ends_with(']') {
        &h[1..h.len() - 1]
    } else {
        h
    };
    if h.is_empty()
        || h.chars()
            .any(|c| c.is_whitespace() || c.is_control() || "/\\@?#".contains(c))
    {
        return Err("请输入主机地址，不包含协议、路径或空格".into());
    }
    Ok(h.into())
}
pub(super) fn validate(c: &Catalog) -> Result<(), String> {
    if c.schema_version != 2 {
        return Err("不支持该服务器目录版本".into());
    }
    let mut ids = HashSet::new();
    for id in c
        .tags
        .iter()
        .map(|g| &g.id)
        .chain(c.servers.iter().map(|s| &s.id))
        .chain(c.proxies.iter().map(|p| &p.id))
    {
        if id.is_empty() || !ids.insert(id) {
            return Err("目录记录 ID 重复或为空".into());
        }
    }
    let mut names = HashSet::new();
    for g in &c.tags {
        if g.name.trim().is_empty() || !names.insert(g.name.to_lowercase()) {
            return Err("标签名称为空或重复".into());
        }
    }
    let server_ids: HashSet<_> = c.servers.iter().map(|s| &s.id).collect();
    let proxy_ids: HashSet<_> = c.proxies.iter().map(|s| &s.id).collect();
    let known_tags: HashSet<_> = c.tags.iter().map(|s| &s.id).collect();
    for s in &c.servers {
        host(&s.host)?;
        if s.port == 0
            || s.username.trim().is_empty()
            || s.name.trim().is_empty()
            || s.protocol != "ssh"
            || !matches!(
                s.authentication,
                Authentication::Password { .. } | Authentication::PrivateKey { .. }
            )
        {
            return Err("服务器字段不完整或认证方式错误".into());
        }
        let mut tag_ids = HashSet::new();
        if s.tag_ids
            .iter()
            .any(|id| !tag_ids.insert(id) || !known_tags.contains(id))
        {
            return Err("服务器引用了不存在的标签".into());
        }
        let mut steps = HashSet::new();
        let mut ssh = HashSet::new();
        for step in &s.route {
            if step.id().is_empty() || !steps.insert(step.id()) {
                return Err("路径步骤 ID 重复或为空".into());
            }
            match step {
                RouteStep::Ssh { server_id, .. } => {
                    if server_id == &s.id
                        || !ssh.insert(server_id)
                        || !server_ids.contains(server_id)
                    {
                        return Err("SSH 路径不能引用自身、重复服务器或已删除的服务器".into());
                    }
                }
                RouteStep::Socks5 { proxy_id, .. } => {
                    if !proxy_ids.contains(proxy_id) {
                        return Err("路径引用了不存在的 SOCKS5 代理".into());
                    }
                }
            }
        }
    }
    for p in &c.proxies {
        host(&p.host)?;
        let valid_auth = match &p.authentication {
            Authentication::None => true,
            Authentication::UsernamePassword { username, .. } => !username.trim().is_empty(),
            _ => false,
        };
        if p.name.trim().is_empty() || p.port == 0 || p.protocol != "socks5" || !valid_auth {
            return Err("代理字段不完整或认证方式错误".into());
        }
    }
    Ok(())
}
