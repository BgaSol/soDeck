use crate::model::{AppError, AppResult};

pub const GUARD_ID: &str = "00000000-0000-0000-0000-000000000000";

#[derive(Debug, PartialEq, Eq)]
pub struct JobState {
    pub phase: String,
    pub code: i32,
}
impl JobState {
    pub fn parse(bytes: &[u8], run: &str, attempt: &str) -> AppResult<Self> {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        let fields: Vec<_> = text.split_ascii_whitespace().collect();
        if bytes.len() > 256
            || fields.len() != 5
            || fields[0] != "2"
            || fields[1] != run
            || fields[2] != attempt
            || !matches!(
                fields[3],
                "ready" | "closed" | "running" | "done" | "failed" | "cancelled" | "rejected"
            )
        {
            return Err(invalid());
        }
        let code: i32 = fields[4].parse().map_err(|_| invalid())?;
        if !(0..=255).contains(&code) {
            return Err(invalid());
        }
        Ok(Self {
            phase: fields[3].into(),
            code,
        })
    }
    pub fn stopped(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "done" | "failed" | "cancelled" | "rejected"
        )
    }
}
fn invalid() -> AppError {
    AppError::new("unknown", "远端任务记录无效或任务代次不一致")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_bind_run_and_attempt_and_reject_extra_data() {
        assert!(JobState::parse(b"2 run item done 0\n", "run", "item")
            .unwrap()
            .stopped());
        for value in [
            "2 old item done 0",
            "2 run old done 0",
            "2 run item done 0 extra",
            "2 run item surprise 0",
            "2 run item done -1",
        ] {
            assert!(JobState::parse(value.as_bytes(), "run", "item").is_err());
        }
    }
}
