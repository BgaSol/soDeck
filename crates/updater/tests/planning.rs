mod common;
use common::*;
use sodeck_updater::{catalog::VerifiedCatalog,model::*,plan};

#[test]fn signed_catalog_binds_body_key_and_channel(){
    let (c,_)=fixture();let(mut b,s,k)=signed(&c);assert!(VerifiedCatalog::verify(&b,&s,&k,Channel::Preview).is_err());assert!(VerifiedCatalog::verify(&b,&s,&Default::default(),Channel::Stable).is_err());b[20]^=1;assert!(VerifiedCatalog::verify(&b,&s,&k,Channel::Stable).is_err());
}
#[test]fn refuses_unsafe_assets_and_false_version(){
    let (mut c,_)=fixture();c.plugins.get_mut("sodeck.files").unwrap().current.asset.name="../file.pkg".into();assert!(c.validate(Channel::Stable).is_err());
    let (mut c,_)=fixture();c.core.current.platforms.get_mut("windows-x86_64").unwrap().signed_version="9.0.0".into();assert!(c.validate(Channel::Stable).is_err());
}
#[test]fn plugin_update_does_not_select_or_download_core(){
    let(c,s)=fixture();let p=plan::make(&verified(&c),&s,&["sodeck.terminal".into()]).unwrap();assert!(p.blocked_reasons.is_empty());assert_eq!(p.download_bytes,100);assert!(!p.components[0].selected);
}
#[test]fn all_sixteen_saved_enablement_sets_gate_the_next_core(){
    let(c,mut s)=fixture();for p in &mut s.plugins{p.selected.as_mut().unwrap().requires.plugin_api=">=6.2.0, <6.3.0".into();}
    let catalog=verified(&c);for mask in 0u32..16 {for(i,p)in s.plugins.iter_mut().enumerate(){p.enabled=mask&(1<<i)!=0;}
        let core_only=plan::make(&catalog,&s,&["core".into()]).unwrap();assert_eq!(core_only.components[0].required_component_ids.len(),mask.count_ones()as usize);
        let mut ids=vec!["core".into()];ids.extend(s.plugins.iter().filter(|p|p.enabled).map(|p|p.plugin_id.clone()));let all=plan::make(&catalog,&s,&ids).unwrap();assert!(all.blocked_reasons.is_empty(),"mask {mask}: {:?}",all.blocked_reasons);
    }
}
#[test]fn pending_and_saved_enablement_invalidate_a_plan(){
    let(c,mut s)=fixture();let catalog=verified(&c);let ids=vec!["core".into()];let a=plan::make(&catalog,&s,&ids).unwrap();s.plugins[0].pending=s.plugins[0].selected.clone();s.plugins[0].pending.as_mut().unwrap().requires.plugin_api="<6.3.0".into();
    let b=plan::make(&catalog,&s,&ids).unwrap();assert_ne!(a.plan_revision,b.plan_revision);assert!(!b.blocked_reasons.is_empty());s.plugins[0].enabled=false;assert!(plan::make(&catalog,&s,&ids).unwrap().blocked_reasons.is_empty());
}
#[test]fn future_plugin_is_separate_from_current_context(){
    let(mut c,s)=fixture();c.plugins.get_mut("sodeck.terminal").unwrap().current.requires.plugin_api=">=6.3.0, <7.0.0".into();let catalog=verified(&c);let p=plan::make(&catalog,&s,&["sodeck.terminal".into()]).unwrap();
    let row=p.components.iter().find(|p|p.component_id=="sodeck.terminal").unwrap();assert_eq!(row.required_component_ids,vec!["core"]);assert!(!p.components[0].selected);assert!(matches!(&row.target,Some(plan::Target::Plugin{requires_core:Some(v),..})if v=="2.0.1"));
}
#[test]fn local_packages_and_observation_schema_are_not_silently_replaced(){
    let(mut c,mut s)=fixture();s.plugins[0].origin=Origin::Local;s.plugins[0].follow_official=false;c.core.current.data_formats.insert("observabilityFacts".into(),3);
    let p=plan::make(&verified(&c),&s,&["core".into(),"sodeck.terminal".into()]).unwrap();assert_eq!(p.components[0].blocked_reasons.len(),1);assert!(p.components[1].target.is_none());assert!(!p.blocked_reasons.is_empty());
}
#[test]fn every_persisted_format_is_checked_independently(){
    for name in ["core","observabilityFacts","observabilityLogs","observabilityTraffic","observabilityDiagnostics","activation","enablement","updates"] {
        let(mut c,s)=fixture();*c.core.current.data_formats.get_mut(name).unwrap()+=1;
        let p=plan::make(&verified(&c),&s,&["core".into()]).unwrap();assert!(!p.blocked_reasons.is_empty(),"{name}");
        let(mut c,s)=fixture();c.core.current.data_formats.remove(name);
        assert!(!plan::make(&verified(&c),&s,&["core".into()]).unwrap().blocked_reasons.is_empty(),"missing {name}");
    }
}
#[test]fn cannot_update_uninstalled_ids_or_treat_compatibility_as_app_version(){
    let(c,s)=fixture();let catalog=verified(&c);assert!(plan::make(&catalog,&s,&["sodeck.unknown".into()]).is_err());let p=plan::make(&catalog,&s,&["core".into()]).unwrap();assert_eq!(p.components[0].available_version.as_deref(),Some("2.0.1"));
}
