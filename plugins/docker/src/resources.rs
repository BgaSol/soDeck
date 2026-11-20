use crate::commands;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Container,
    Image,
    Network,
    Volume,
}
impl Kind {
    pub const ALL: [Self; 4] = [Self::Container, Self::Image, Self::Network, Self::Volume];
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "container" => Ok(Self::Container),
            "image" => Ok(Self::Image),
            "network" => Ok(Self::Network),
            "volume" => Ok(Self::Volume),
            _ => Err("资源类型无效".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Image => "image",
            Self::Network => "network",
            Self::Volume => "volume",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Container => "容器",
            Self::Image => "镜像",
            Self::Network => "网络",
            Self::Volume => "卷",
        }
    }
    pub fn validate(self, id: &str) -> Result<(), String> {
        match self {
            Self::Container | Self::Network => commands::id(id),
            Self::Image => id
                .strip_prefix("sha256:")
                .ok_or("需要完整镜像 ID".into())
                .and_then(commands::id),
            Self::Volume => {
                if !id.is_empty()
                    && id.len() <= 255
                    && id.as_bytes()[0].is_ascii_alphanumeric()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                {
                    Ok(())
                } else {
                    Err("卷名称无效".into())
                }
            }
        }
    }
    pub fn summary(self) -> &'static str {
        match self {
            Self::Container => commands::SUMMARY,
            Self::Image => {
                r#"{"id":{{json .Id}},"tags":{{json (index . "RepoTags")}},"digests":{{json (index . "RepoDigests")}},"created":{{json (index . "Created")}},"size":{{json (index . "Size")}}}"#
            }
            Self::Network => {
                r#"{"id":{{json .Id}},"name":{{json .Name}},"driver":{{json (index . "Driver")}},"scope":{{json (index . "Scope")}},"created":{{json (index . "Created")}}}"#
            }
            Self::Volume => {
                r#"{"id":{{json .Name}},"name":{{json .Name}},"driver":{{json .Driver}},"scope":{{json .Scope}},"created":{{json .CreatedAt}},"observation":{"Name":{{json .Name}},"Driver":{{json .Driver}},"Scope":{{json .Scope}},"CreatedAt":{{json .CreatedAt}},"Options":{{json .Options}},"Labels":{{json .Labels}}}}"#
            }
        }
    }
    pub fn list(self) -> Vec<&'static str> {
        match self {
            Self::Container => vec!["container", "ls", "--all", "--quiet", "--no-trunc"],
            Self::Image => vec!["image", "ls", "--all", "--quiet", "--no-trunc"],
            Self::Network => vec!["network", "ls", "--quiet", "--no-trunc"],
            Self::Volume => vec!["volume", "ls", "--quiet"],
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub kind: Kind,
    pub id: String,
}
impl ResourceRef {
    pub fn read(value: &Value) -> Result<Self, String> {
        let r: Self = serde_json::from_value(value.clone()).map_err(|_| "资源标识无效")?;
        r.kind.validate(&r.id)?;
        Ok(r)
    }
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.name(), self.id)
    }
}
pub fn root(value: &Value) -> Value {
    value
        .as_str()
        .filter(|s| {
            crate::connection::absolute(s) && s.len() <= 4096 && !s.chars().any(char::is_control)
        })
        .map_or(Value::Null, |s| json!(s))
}
pub fn normalize(kind: Kind, mut row: Value) -> Result<Value, String> {
    kind.validate(row["id"].as_str().ok_or("资源 ID 缺失")?)?;
    if kind == Kind::Container {
        row["project"] = json!(commands::text(
            row["labels"]["com.docker.compose.project"]
                .as_str()
                .unwrap_or(""),
            256
        ));
        row["service"] = json!(commands::text(
            row["labels"]["com.docker.compose.service"]
                .as_str()
                .unwrap_or(""),
            256
        ));
        row["search"] = json!(commands::text(&row["labels"].to_string(), 4096));
        row.as_object_mut().ok_or("资源摘要无效")?.remove("labels");
    }
    for (key, max) in [
        ("name", 256),
        ("image", 512),
        ("driver", 256),
        ("scope", 64),
    ] {
        if let Some(text) = row[key].as_str() {
            row[key] = json!(commands::text(
                if key == "name" {
                    text.trim_start_matches('/')
                } else {
                    text
                },
                max
            ));
        }
    }
    if kind == Kind::Image {
        row["name"] = row["tags"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or_else(|| row["id"].clone());
    }
    Ok(row)
}
pub const MOUNTS: &str = r#"{"id":{{json .Id}},"name":{{json .Name}},"imageId":{{json .Image}},"mounts":{{json .Mounts}}}"#;
pub const CONTAINER_RELATIONS: &str = r#"{"Id":{{json .Id}},"Image":{{json .Image}},"Networks":{{json .NetworkSettings.Networks}},"Mounts":{{json .Mounts}}}"#;
pub fn relation(kind: Kind, value: &Value) -> Value {
    match kind {
        Kind::Container => {
            let mut refs = vec![
                json!({"group":"image","label":value["Image"],"resource":{"kind":"image","id":value["Image"]}}),
            ];
            for (name, v) in value["Networks"].as_object().into_iter().flatten() {
                if let Some(id) = v["NetworkID"]
                    .as_str()
                    .filter(|id| Kind::Network.validate(id).is_ok())
                {
                    refs.push(json!({"group":"network","label":name,"resource":{"kind":"network","id":id}}));
                }
            }
            for m in value["Mounts"].as_array().into_iter().flatten() {
                let mut r = json!({"group":"mount","label":format!("{} → {}",m[if m["Type"]=="volume"{"Name"}else{"Source"}].as_str().unwrap_or("挂载"),m["Destination"].as_str().unwrap_or("")),"mountType":m["Type"]});
                if m["Type"] == "volume" {
                    r["resource"] = json!({"kind":"volume","id":m["Name"]});
                }
                refs.push(r);
            }
            json!(refs)
        }
        Kind::Network => json!(value["Containers"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, v)| json!({"label":v["Name"],"resource":{"kind":"container","id":id}}))
            .collect::<Vec<_>>()),
        _ => json!([]),
    }
}
pub fn volume_observation(value: &Value) -> Value {
    json!({"Name":value["Name"],"Driver":value["Driver"],"Scope":value["Scope"],"CreatedAt":value["CreatedAt"],"Options":value["Options"],"Labels":value["Labels"]})
}
pub fn verify_delete(
    target: &ResourceRef,
    current: &Value,
    observation: &Value,
    references: &[Value],
    complete: bool,
) -> Result<Vec<String>, String> {
    target.kind.validate(&target.id)?;
    if target.kind == Kind::Container {
        return Err("请使用容器操作入口".into());
    }
    if !complete {
        return Err("引用检查不完整，不能删除；请重新读取".into());
    }
    if !references.is_empty() {
        return Err(format!("仍被 {} 个容器引用，不能删除", references.len()));
    }
    match target.kind {
        Kind::Image if current["Id"] != target.id => return Err("镜像身份已改变".into()),
        Kind::Network => {
            if current["Id"] != target.id {
                return Err("网络身份已改变".into());
            }
            if ["bridge", "host", "none", "nat", "Default Switch"]
                .contains(&current["Name"].as_str().unwrap_or(""))
            {
                return Err("系统网络不能删除".into());
            }
            if current["Containers"]
                .as_object()
                .is_none_or(|m| !m.is_empty())
            {
                return Err("网络端点未知或仍有连接，不能删除".into());
            }
        }
        Kind::Volume => {
            if current["Name"] != target.id
                || current["CreatedAt"].as_str().is_none_or(str::is_empty)
                || current["Driver"].as_str().is_none_or(str::is_empty)
                || current["Scope"].as_str().is_none_or(str::is_empty)
                || !current
                    .as_object()
                    .is_some_and(|m| m.contains_key("Options") && m.contains_key("Labels"))
                || volume_observation(current) != *observation
            {
                return Err("卷的完整身份观察已改变或缺失，请刷新并重新确认".into());
            }
            // Docker has no compare-and-delete for volumes; a concurrent same-name rebuild can race rm.
        }
        _ => {}
    }
    let mut args = vec![target.kind.name().into(), "rm".into()];
    if target.kind == Kind::Image {
        args.push("--no-prune".into());
    }
    args.push(target.id.clone());
    Ok(args)
}
pub fn pull_reference(input: &str) -> Result<String, String> {
    let text = input.trim();
    if text.is_empty()
        || text.len() > 512
        || text.starts_with('-')
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/:@".contains(&b))
    {
        return Err("请输入有效的公开镜像引用".into());
    }
    let base = text.split('@').next().unwrap();
    let tail = base.rsplit('/').next().unwrap();
    if base.starts_with('/') || base.ends_with('/') || base.contains("//") || tail.is_empty() {
        return Err("镜像引用无效".into());
    }
    if text.contains('@') {
        let (_, digest) = text.split_once('@').unwrap();
        Kind::Image.validate(digest)?;
        if text.matches('@').count() != 1 {
            return Err("镜像摘要无效".into());
        }
        Ok(text.into())
    } else if tail.contains(':') {
        let (_, tag) = tail.rsplit_once(':').unwrap();
        if tag.is_empty()
            || tag.len() > 128
            || !tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || tag.starts_with(['.', '-'])
        {
            return Err("镜像标签无效".into());
        }
        Ok(text.into())
    } else {
        Ok(format!("{text}:latest"))
    }
}
pub fn fields(kind: Kind, value: &Value) -> Value {
    let keys: &[&str] = match kind {
        Kind::Image => &[
            "Id",
            "RepoTags",
            "RepoDigests",
            "Created",
            "Size",
            "Architecture",
            "Os",
        ],
        Kind::Network => &[
            "Id",
            "Name",
            "Driver",
            "Scope",
            "Created",
            "Internal",
            "Attachable",
            "IPAM",
            "Options",
            "Labels",
            "Containers",
        ],
        Kind::Volume => &[
            "Name",
            "Driver",
            "Scope",
            "CreatedAt",
            "Mountpoint",
            "Options",
            "Labels",
            "Status",
        ],
        Kind::Container => &[],
    };
    json!({"basic":keys.iter().filter(|k|!value[**k].is_null()).map(|k|json!({"key":k,"value":commands::text(&value[*k].to_string(),4096)})).collect::<Vec<_>>()})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_and_roots() {
        assert!(Kind::Image
            .validate(&format!("sha256:{}", "a".repeat(64)))
            .is_ok());
        assert!(Kind::Volume.validate("../data").is_err());
        assert_eq!(
            root(&json!("/home/ops/.local/share/docker")),
            json!("/home/ops/.local/share/docker")
        );
        assert!(root(&json!("docker")).is_null());
        assert_eq!(
            pull_reference("registry:5000/team/api").unwrap(),
            "registry:5000/team/api:latest"
        );
        assert!(pull_reference("demo; rm -rf /").is_err());
    }
    #[test]
    fn deletion_needs_complete_identity_and_references() {
        let target = ResourceRef {
            kind: Kind::Volume,
            id: "data".into(),
        };
        let current = json!({"Name":"data","Driver":"local","Scope":"local","CreatedAt":"2026-11-16T00:00:00Z","Options":null,"Labels":null});
        let observation = volume_observation(&current);
        assert_eq!(
            verify_delete(&target, &current, &observation, &[], true).unwrap(),
            ["volume", "rm", "data"]
        );
        assert!(verify_delete(
            &target,
            &current,
            &observation,
            &[json!({"id":"stopped"})],
            true
        )
        .is_err());
        let mut newer = current.clone();
        newer["CreatedAt"] = json!("new");
        assert!(verify_delete(&target, &newer, &observation, &[], true).is_err());
        assert!(verify_delete(&target, &current, &observation, &[], false).is_err());
    }
}
