use super::*;
impl Io {
    pub(super) async fn preserve_metadata(
        &self,
        temporary: &Endpoint,
        before: &Value,
        original: &Option<Value>,
        content_only: bool,
    ) -> Result<(), OperationError> {
        match temporary {
            Endpoint::Local(path) => {
                let path = path.clone();
                let modified = (!content_only)
                    .then(|| before["mtime"].as_u64())
                    .flatten()
                    .map(|s| std::time::UNIX_EPOCH + Duration::from_secs(s));
                #[cfg(unix)]
                let permissions = original
                    .as_ref()
                    .and_then(|v| v["permissions"].as_u64())
                    .or_else(|| {
                        if content_only {
                            None
                        } else {
                            before["permissions"].as_u64()
                        }
                    });
                tokio::task::spawn_blocking(move || {
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .map_err(local_error)?;
                    if let Some(time) = modified {
                        file.set_times(std::fs::FileTimes::new().set_modified(time))
                            .map_err(local_error)?;
                    }
                    #[cfg(unix)]
                    if let Some(mode) = permissions {
                        use std::os::unix::fs::PermissionsExt;
                        file.set_permissions(std::fs::Permissions::from_mode(mode as u32 & 0o777))
                            .map_err(local_error)?;
                    }
                    Ok::<_, OperationError>(())
                })
                .await
                .map_err(err)??;
            }
            Endpoint::Remote(path) => {
                let mtime = (!content_only)
                    .then(|| before["mtime"].as_u64())
                    .flatten()
                    .and_then(|s| u32::try_from(s).ok());
                let permissions = original
                    .as_ref()
                    .and_then(|v| v["permissions"].as_u64())
                    .or_else(|| {
                        if content_only {
                            None
                        } else {
                            before["permissions"].as_u64()
                        }
                    })
                    .map(|p| p as u32 & 0o777);
                if mtime.is_some() || permissions.is_some() {
                    self.session
                        .raw
                        .setstat(
                            path,
                            FileAttributes {
                                atime: mtime,
                                mtime,
                                permissions,
                                ..Default::default()
                            },
                        )
                        .await
                        .map_err(err)?;
                }
            }
        }

        Ok(())
    }
}
