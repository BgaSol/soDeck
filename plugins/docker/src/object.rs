use crate::{commands, ports, resources::Kind};
use serde::Serialize;
use serde_json::{json, Value};

// Only fields used by the inline resource view leave this projection.
pub const CONTAINER: &str = r#"{"Id":{{json .Id}},"Image":{{json .Image}},"Config":{"Image":{{json .Config.Image}}},"State":{{json .State}},"HostConfig":{"RestartPolicy":{{json .HostConfig.RestartPolicy}}},"Mounts":{{json .Mounts}},"NetworkSettings":{{json .NetworkSettings}}}"#;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ObjectInfo {
    Container {
        image: String,
        image_id: String,
        status: String,
        health: String,
        started: String,
        finished: String,
        exit_code: Option<i64>,
        restart: String,
        ports: Value,
        mounts: Vec<Value>,
        networks: Vec<Value>,
        partial: bool,
    },
    Image {
        size: Option<u64>,
        platform: String,
        aliases: Vec<Value>,
        partial: bool,
    },
    Network {
        driver: String,
        scope: String,
        subnets: Vec<Value>,
        partial: bool,
    },
    Volume {
        driver: String,
        scope: String,
        mountpoint: String,
    },
}
fn text(value: &Value) -> String {
    commands::text(value.as_str().unwrap_or(""), 4096)
}
impl ObjectInfo {
    pub fn read(kind: Kind, value: &Value) -> Self {
        match kind {
            Kind::Container => {
                let mounts = value["Mounts"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                let networks = value["NetworkSettings"]["Networks"].as_object();
                Self::Container {
                    image: text(&value["Config"]["Image"]), image_id: text(&value["Image"]), status: text(&value["State"]["Status"]), health: text(&value["State"]["Health"]["Status"]),
                    started: text(&value["State"]["StartedAt"]), finished: text(&value["State"]["FinishedAt"]), exit_code: value["State"]["ExitCode"].as_i64(), restart: text(&value["HostConfig"]["RestartPolicy"]["Name"]),
                    ports: ports::catalog(value),
                    mounts: mounts.iter().take(100).map(|m| json!({"type":text(&m["Type"]),"source":text(&m[if m["Type"]=="volume"{"Name"}else{"Source"}]),"destination":text(&m["Destination"]),"readOnly":m["RW"]==false,"resource":if m["Type"]=="volume"{json!({"kind":"volume","id":text(&m["Name"])})}else{Value::Null}})).collect(),
                    networks: networks.into_iter().flatten().take(100).map(|(name,n)|json!({"name":commands::text(name,256),"address":text(&n["IPAddress"]),"ipv6":text(&n["GlobalIPv6Address"]),"resource":{"kind":"network","id":text(&n["NetworkID"])}})).collect(),
                    partial: mounts.len()>100 || networks.is_some_and(|n|n.len()>100),
                }
            }
            Kind::Image => Self::Image {
                partial: value["RepoTags"].as_array().is_some_and(|a| a.len() > 100),
                size: value["Size"].as_u64(),
                platform: format!("{}/{}", text(&value["Os"]), text(&value["Architecture"])),
                aliases: value["RepoTags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(100)
                    .map(|v| json!(text(v)))
                    .collect(),
            },
            Kind::Network => Self::Network {
                partial: value["IPAM"]["Config"]
                    .as_array()
                    .is_some_and(|a| a.len() > 100),
                driver: text(&value["Driver"]),
                scope: text(&value["Scope"]),
                subnets: value["IPAM"]["Config"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(100)
                    .map(|v| json!({"subnet":text(&v["Subnet"]),"gateway":text(&v["Gateway"])}))
                    .collect(),
            },
            Kind::Volume => Self::Volume {
                driver: text(&value["Driver"]),
                scope: text(&value["Scope"]),
                mountpoint: text(&value["Mountpoint"]),
            },
        }
    }
    pub fn summary(&self) -> Value {
        match self {
            Self::Container {
                image,
                image_id,
                status,
                health,
                started,
                finished,
                exit_code,
                restart,
                ports,
                mounts,
                networks,
                partial,
            } => {
                json!({"kind":"container","image":image,"imageId":image_id,"status":status,"health":health,"started":started,"finished":finished,"exitCode":exit_code,"restart":restart,"counts":{"ports":ports["rows"].as_array().map_or(0,Vec::len),"mounts":mounts.len(),"networks":networks.len()},"partial":*partial||ports["partial"]==true})
            }
            Self::Image {
                size,
                platform,
                aliases,
                partial,
            } => {
                json!({"kind":"image","size":size,"platform":platform,"counts":{"aliases":aliases.len()},"partial":partial})
            }
            Self::Network {
                driver,
                scope,
                subnets,
                partial,
            } => {
                json!({"kind":"network","driver":driver,"scope":scope,"counts":{"subnets":subnets.len()},"partial":partial})
            }
            Self::Volume {
                driver,
                scope,
                mountpoint,
            } => json!({"kind":"volume","driver":driver,"scope":scope,"mountpoint":mountpoint}),
        }
    }
    pub fn collection(&self, name: &str) -> Result<Vec<Value>, String> {
        Ok(match (self, name) {
            (Self::Container { ports, .. }, "ports") => {
                ports["rows"].as_array().cloned().unwrap_or_default()
            }
            (Self::Container { mounts, .. }, "mounts") => mounts.clone(),
            (Self::Container { networks, .. }, "networks") => networks.clone(),
            (Self::Image { aliases, .. }, "aliases") => aliases.clone(),
            (Self::Network { subnets, .. }, "subnets") => subnets.clone(),
            _ => return Err("此对象不支持该信息集合".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_collections_report_truncation() {
        for (kind, value, collection) in [
            (Kind::Image, json!({"RepoTags":vec!["tag";101]}), "aliases"),
            (
                Kind::Network,
                json!({"IPAM":{"Config":vec![json!({"Subnet":"10.0.0.0/24"});101]}}),
                "subnets",
            ),
        ] {
            let info = ObjectInfo::read(kind, &value);
            assert_eq!(info.summary()["partial"], true);
            assert_eq!(info.collection(collection).unwrap().len(), 100);
        }
    }
    #[test]
    fn inline_projection_keeps_actual_ports_and_excludes_configuration_secrets() {
        let value = json!({"Id":"a".repeat(64),"Config":{"Image":"nginx:1","Env":["TOKEN=secret"],"Labels":{"secret":"secret"}},"State":{"Status":"running"},"NetworkSettings":{"Ports":{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"18080"}]}},"Mounts":[]});
        let info = ObjectInfo::read(Kind::Container, &value);
        assert_eq!(
            info.collection("ports").unwrap()[0]["target"]["port"],
            18080
        );
        assert!(!serde_json::to_string(&info).unwrap().contains("secret"));
        assert!(info.collection("environment").is_err());
    }
}
