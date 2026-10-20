use super::model::Preview;
use crate::{
    connection::WindowOwner,
    model::{AppError, AppResult},
};
use std::collections::HashMap;

#[derive(Clone)]
pub(super) struct Draft {
    pub preview: Preview,
    pub identity: String,
    pub session: String,
    pub owner: WindowOwner,
    pub fingerprint: String,
    pub create: Option<super::create::Spec>,
    pub transfer: Option<super::transfer::Spec>,
    pub retain_until: u64,
}
impl Draft {
    pub fn authorize(
        &self,
        session: &str,
        identity: &str,
        owner: &WindowOwner,
        now: u64,
    ) -> AppResult<()> {
        if self.session != session
            || self.identity != identity
            || self.owner.label != owner.label
            || self.owner.generation != owner.generation
        {
            return Err(AppError::new("permission", "预览不属于当前窗口或会话"));
        }
        if self.retain_until <= now {
            return Err(unavailable());
        }
        Ok(())
    }
}
fn unavailable() -> AppError {
    AppError::new("previewUnavailable", "草稿已失效，请返回编辑后重新预检")
}
#[derive(Default)]
pub(super) struct DraftRegistry {
    entries: HashMap<String, Draft>,
}
impl DraftRegistry {
    pub fn insert(&mut self, draft: Draft, now: u64) -> AppResult<Preview> {
        self.expire(now);
        if self.entries.len() >= 100 {
            return Err(AppError::new("busy", "确认请求过多，请稍后重试"));
        }
        let preview = draft.preview.clone();
        self.entries.insert(preview.draft_token.clone(), draft);
        Ok(preview)
    }
    pub fn current(&self, token: &str) -> AppResult<&Draft> {
        self.entries.get(token).ok_or_else(unavailable)
    }
    pub fn get(&self, preview_id: &str) -> Option<&Draft> {
        self.entries.values().find(|d| d.preview.id == preview_id)
    }
    pub fn remove(&mut self, preview_id: &str) {
        let token = self.get(preview_id).map(|d| d.preview.draft_token.clone());
        if let Some(token) = token {
            self.entries.remove(&token);
        }
    }
    // The source must still exist after IO. Submission/release wins over renewal.
    pub fn replace(
        &mut self,
        token: &str,
        expected: &str,
        mut draft: Draft,
        now: u64,
    ) -> AppResult<Preview> {
        let current = self.current(token)?;
        if current.retain_until <= now {
            return Err(unavailable());
        }
        if current.preview.id != expected {
            return Ok(current.preview.clone());
        }
        draft.retain_until = current.retain_until;
        draft.preview.draft_token = token.into();
        let preview = draft.preview.clone();
        self.entries.insert(token.into(), draft);
        Ok(preview)
    }
    pub fn release(&mut self, token: &str, session: &str, owner: &WindowOwner) -> AppResult<()> {
        if let Some(d) = self.entries.get(token) {
            if d.session != session
                || d.owner.label != owner.label
                || d.owner.generation != owner.generation
            {
                return Err(AppError::new("permission", "预览不属于当前窗口或会话"));
            }
        }
        self.entries.remove(token);
        Ok(())
    }
    pub fn expire(&mut self, now: u64) {
        self.entries.retain(|_, d| d.retain_until > now);
    }
    pub fn remove_owner(&mut self, label: &str) {
        self.entries.retain(|_, d| d.owner.label != label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::model::{Action, EngineBinding};
    fn draft(token: &str, id: &str) -> Draft {
        Draft {
            preview: Preview {
                id: id.into(),
                draft_token: token.into(),
                engine: EngineBinding {
                    token: "engine".into(),
                    generation: "g".into(),
                    engine_generation: 1,
                    endpoint: "unix:///docker.sock".into(),
                    daemon_id: None,
                },
                target_id: "target".into(),
                target_name: "name".into(),
                action: Action::Remove,
                consequences: vec![],
                expires_at: 60,
            },
            identity: "identity".into(),
            session: "session".into(),
            owner: WindowOwner {
                label: "main".into(),
                generation: 1,
            },
            fingerprint: "fp".into(),
            create: None,
            transfer: None,
            retain_until: 900,
        }
    }
    #[test]
    fn expiration_retains_source_but_renewal_replaces_only_current_version() {
        let mut r = DraftRegistry::default();
        r.insert(draft("token", "old"), 0).unwrap();
        r.expire(61);
        assert!(r.get("old").is_some());
        let next = r
            .replace("token", "old", draft("unused", "new"), 61)
            .unwrap();
        assert_eq!(next.draft_token, "token");
        assert!(r.get("old").is_none());
        assert!(r.get("new").is_some());
        assert_eq!(
            r.replace("token", "old", draft("unused", "late"), 62)
                .unwrap()
                .id,
            "new"
        );
        assert_eq!(r.current("token").unwrap().retain_until, 900);
    }
    #[test]
    fn release_and_submission_win_over_inflight_renewal() {
        for release in [false, true] {
            let mut r = DraftRegistry::default();
            let d = draft("token", "old");
            r.insert(d.clone(), 0).unwrap();
            if release {
                r.release("token", "session", &d.owner).unwrap();
            } else {
                r.remove("old");
            }
            assert!(r
                .replace("token", "old", draft("unused", "new"), 1)
                .is_err());
            assert!(r.get("new").is_none());
        }
    }
    #[test]
    fn renewal_is_allowed_at_capacity_without_extending_retention() {
        let mut r = DraftRegistry::default();
        for n in 0..100 {
            r.insert(draft(&format!("t{n}"), &format!("p{n}")), 0)
                .unwrap();
        }
        assert!(r.insert(draft("extra", "extra"), 0).is_err());
        assert!(r.replace("t0", "p0", draft("unused", "new"), 100).is_ok());
        assert!(r
            .replace("t0", "new", draft("unused", "later"), 900)
            .is_err());
        r.expire(900);
        assert!(r.current("t0").is_err());
        assert!(r.insert(draft("extra", "extra"), 0).is_ok());
    }
    #[test]
    fn ownership_requires_session_identity_and_window_generation() {
        let d = draft("token", "old");
        assert!(d.authorize("session", "identity", &d.owner, 1).is_ok());
        assert!(d.authorize("other", "identity", &d.owner, 1).is_err());
        assert!(d.authorize("session", "other", &d.owner, 1).is_err());
        let other = WindowOwner {
            label: "main".into(),
            generation: 2,
        };
        assert!(d.authorize("session", "identity", &other, 1).is_err());
        let mut r = DraftRegistry::default();
        r.insert(d, 0).unwrap();
        assert!(r.release("token", "session", &other).is_err());
        assert!(r.current("token").is_ok());
        r.remove_owner("main");
        assert!(r.current("token").is_err());
    }
}
