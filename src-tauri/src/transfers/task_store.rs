use super::{
    model::Receipt,
    planner::{Entry, Plan},
};
use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection};
use std::path::Path;

pub struct TaskStore(Connection);
impl TaskStore {
    pub fn open(path: &Path) -> AppResult<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, receipt TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS entries(task TEXT NOT NULL, position INTEGER NOT NULL,
                entry TEXT NOT NULL, PRIMARY KEY(task, position));",
        )?;
        Ok(Self(db))
    }
    pub fn save(&self, receipt: &Receipt) -> AppResult<()> {
        let id = &receipt
            .view
            .as_ref()
            .ok_or_else(|| AppError::new("internal", "任务缺少视图"))?
            .id;
        self.0.execute("INSERT INTO tasks VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET receipt=excluded.receipt",
            params![id, json(receipt)?])?;
        Ok(())
    }
    pub fn plan(&mut self, id: &str, plan: &Plan) -> AppResult<()> {
        let tx = self.0.transaction()?;
        tx.execute("DELETE FROM entries WHERE task=?1", [id])?;
        for (position, entry) in plan.entries.iter().enumerate() {
            tx.execute(
                "INSERT INTO entries VALUES(?1,?2,?3)",
                params![id, position as i64, json(entry)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn entry(&self, id: &str, index: usize, entry: &Entry) -> AppResult<()> {
        self.0.execute("INSERT INTO entries VALUES(?1,?2,?3) ON CONFLICT(task,position) DO UPDATE SET entry=excluded.entry",
            params![id, index as i64, json(entry)?])?;
        Ok(())
    }
    pub fn load(&self) -> AppResult<Vec<Receipt>> {
        let mut rows = self
            .0
            .prepare("SELECT id,receipt FROM tasks ORDER BY rowid")?;
        let values = rows.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = vec![];
        for value in values {
            let (id, value) = value?;
            let mut receipt: Receipt =
                serde_json::from_str(&value).map_err(|e| AppError::new("taskStore", e))?;
            receipt.plan.skipped = receipt.skipped_paths.clone();
            let mut entries = self
                .0
                .prepare("SELECT entry FROM entries WHERE task=?1 ORDER BY position")?;
            for entry in entries.query_map([id], |row| row.get::<_, String>(0))? {
                receipt.plan.entries.push(
                    serde_json::from_str(&entry?).map_err(|e| AppError::new("taskStore", e))?,
                );
            }
            if let Some(view) = receipt.view.as_mut() {
                view.items = receipt
                    .plan
                    .entries
                    .iter()
                    .filter(|e| e.error.is_some())
                    .take(200)
                    .map(|e| super::model::ItemView {
                        source: e.source.clone(),
                        target: e.target.clone(),
                        status: e.outcome.clone(),
                        error: e.error.clone(),
                    })
                    .collect();
                view.files_done = receipt
                    .plan
                    .entries
                    .iter()
                    .filter(|e| !e.directory && e.outcome == "completed")
                    .count() as u64;
                view.committed_bytes = receipt
                    .plan
                    .entries
                    .iter()
                    .filter(|e| e.committed())
                    .map(|e| e.size)
                    .sum();
                if receipt.pending_commit.is_some() {
                    view.status = "needsAttention".into();
                    view.error = Some("上次提交结果未确认，请核对具体目标文件".into());
                } else if view.active() {
                    let copied = receipt.plan.entries.iter().any(|e| e.outcome == "copied");
                    view.status = if copied {
                        "sourceRetained"
                    } else {
                        "interrupted"
                    }
                    .into();
                    view.error = Some(
                        if copied {
                            "目标已提交，来源删除步骤尚未完成；重试只继续剩余步骤"
                        } else {
                            "上次执行中断；可核对结果或使用新临时目录重试"
                        }
                        .into(),
                    );
                }
                view.waiting_reason = None;
                view.cleanup_pending = receipt
                    .attempts
                    .iter()
                    .any(|a| !a.target_root.is_empty() || a.source_root.is_some());
            }
            if let Some(mut view) = receipt.view.take() {
                receipt.update_resources(&mut view);
                receipt.view = Some(view);
            }
            result.push(receipt);
        }
        Ok(result)
    }
}
fn json(value: &impl serde::Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::new("taskStore", e))
}
