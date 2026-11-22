use super::*;
impl AppCore {
    pub(super) async fn load_document(
        &self,
        window: &str,
        token: &str,
        p: &Value,
        view: &crate::component_ui::View,
        r: &Arc<Resource>,
    ) -> Result<Value, String> {
        let mut s = r.state.lock().await;

        if r.editor.lock().unwrap().document.is_some() {
            return Err("此编辑器已经打开文件，刷新不会覆盖草稿".into());
        }
        let target = path(&p)?.to_string();
        let f = sftp(self, &mut s, &view.context).await?;
        let resolved = canonical(&f.raw, &target).await?;
        let a = f.raw.lstat(resolved.clone()).await.map_err(err)?.attrs;
        if !a.file_type().is_file() {
            return Err("只支持编辑普通文件".into());
        }
        let bytes = read_small(&f.raw, &resolved, &a, |n| {
            self.record_business(&r.owner, 0, n as u64)
        })
        .await?;
        let version = version(&a, &bytes);
        drop(s);
        let admission = self.workbench_state();
        if !admission.ui.surfaces.get(token).is_some_and(|surface| {
            surface.window == window && surface.phase == crate::component_ui::SurfacePhase::Active
        }) || admission.ui.resource_view(&view.id)?.context != view.context
            || admission.ui.lifecycle.view_frozen(&view.id)
        {
            return Err("文件输入权已改变".into());
        }
        let mut editor = r.editor.lock().unwrap();
        if editor.document.is_some() {
            return Err("此编辑器已经打开文件".into());
        }
        editor.document = Some(Document {
            id: self.next_id(),
            path: target,
            resolved,
            version,
            attrs: a,
            bytes: bytes.into(),
            revision: 0,
        });
        Ok(editor.describe())
    }
}
