//! Prefix identities exclude labels, draft IDs, suffixes and selected hop routes.
use super::{digest, ResolvedStep, RoutePlan};
use serde_json::{json, Value};

pub fn normalize_host(host:&str)->String {
    let value=host.trim_matches(['[',']']);
    value.parse::<std::net::IpAddr>().map(|ip|ip.to_string()).unwrap_or_else(|_|idna::domain_to_ascii_strict(value.trim_end_matches('.')).unwrap_or_else(|_|value.into()).to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Server,route::{AccessPath,plan::{SshStep,ProxyStep},SocksProxy,ProxyResolve}};
    fn plan()->RoutePlan {
        RoutePlan::from_steps(vec![
            ResolvedStep::Ssh(SshStep{step_id:crate::model::id(),credential_revision:0,server:Server{id:"a".into(),name:"A".into(),host:"Host.Example.".into(),port:22,username:"user".into(),auth_type:"key".into(),revision:1,has_secret:false}}),
            ResolvedStep::Socks5(ProxyStep{step_id:crate::model::id(),credential_revision:0,resolve:ProxyResolve::Proxy,proxy:SocksProxy{id:"p".into(),name:"P".into(),host:"127.0.0.1".into(),port:12021,username:"proxy".into(),auth_type:"usernamePassword".into(),revision:1,has_preset:false}}),
            ResolvedStep::Ssh(SshStep{step_id:"target".into(),credential_revision:0,server:Server{id:"t".into(),name:"T".into(),host:"target.invalid".into(),port:22,username:"target".into(),auth_type:"key".into(),revision:1,has_secret:false}})
        ],AccessPath::default()).unwrap()
    }
    #[test]
    fn prefix_separates_loopback_and_fingerprints_but_ignores_labels_ids_and_suffix() {
        let p=plan();let context=p.verified_context(1,&["key-a".into()]);let mut changed=p.clone();
        if let ResolvedStep::Ssh(s)=&mut changed.steps[0]{s.step_id=crate::model::id();s.server.name="renamed".into();s.server.revision+=1;s.server.host="host.example".into();}
        if let ResolvedStep::Ssh(s)=&mut changed.steps[2]{s.server.host="different-target.invalid".into();s.server.id="different-target".into();}
        assert_eq!(context,changed.verified_context(1,&["key-a".into()]));
        assert_ne!(context,p.verified_context(1,&["key-b".into()]));
        if let ResolvedStep::Ssh(s)=&mut changed.steps[0]{s.server.host="other.invalid".into();}
        assert_ne!(context,changed.verified_context(1,&["key-a".into()]));
    }
    #[test]
    fn proxy_resolution_changes_only_downstream_identity_and_credential_epoch_only_execution() {
        let p=plan();let mut steps=p.steps.clone();
        if let ResolvedStep::Socks5(s)=&mut steps[1]{s.resolve=ProxyResolve::Local;}
        let changed=RoutePlan::from_steps(steps,AccessPath::default()).unwrap();
        assert_eq!(p.credential_context(1),changed.credential_context(1));assert_ne!(p.credential_context(2),changed.credential_context(2));
        let mut steps=p.steps.clone();if let ResolvedStep::Ssh(s)=&mut steps[0]{s.credential_revision=2;}
        let rotated=RoutePlan::from_steps(steps,AccessPath::default()).unwrap();
        assert_eq!(p.credential_context(2),rotated.credential_context(2));assert_ne!(p.revision,rotated.revision);
    }
}
pub fn proxy_identity(proxy:&super::SocksProxy)->Value {json!(["socks5",proxy.id,normalize_host(&proxy.host),proxy.port,proxy.username,proxy.auth_type])}
pub fn identity(step: &ResolvedStep) -> Value {
    match step {
        ResolvedStep::Ssh(s) => json!(["ssh",s.server.id,normalize_host(&s.server.host),s.server.port,s.server.username,s.server.auth_type]),
        ResolvedStep::Socks5(s) => proxy_identity(&s.proxy),
    }
}
impl RoutePlan {
    pub fn credential_context(&self, index: usize) -> String {
        let prefix: Vec<_> = self.steps[..=index].iter().enumerate().map(|(i, step)| {
            json!([identity(step), if i < index { match step { ResolvedStep::Socks5(s) => Some(s.resolve), _ => None } } else { None }])
        }).collect();
        digest(&json!(prefix))
    }
    pub fn verified_context(&self, index: usize, verified: &[String]) -> String {
        digest(&json!([self.credential_context(index),verified]))
    }
    pub fn trust_key(&self, index: usize, verified: &[String]) -> String {
        let (host, port) = self.steps[index].endpoint();
        if index == 0 { return format!("{}:{port}",normalize_host(host)); }
        format!("route:{}", digest(&json!([self.credential_context(index),verified,host,port])))
    }
}
