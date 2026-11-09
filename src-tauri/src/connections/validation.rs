//! Bounded transport verification, absent from production command/RPC registration.
use super::*;
impl AppCore {
    pub(crate) async fn connection_validation_exec(
        &self,
        context: &Context,
        command: &str,
    ) -> Result<(Vec<u8>, u32), String> {
        let session = {
            let entries = self.connections.entries.lock().unwrap();
            self.workspace_state().validate_context(context)?;
            entries
                .get(&context.server_id)
                .filter(|a| a.view().context.as_ref() == Some(context))
                .and_then(|a| a.session.lock().unwrap().clone())
                .ok_or("连接不存在")?
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut channel = session
                .chain
                .last()
                .ok_or("SSH 会话不存在")?
                .channel_open_session()
                .await
                .map_err(|e| e.to_string())?;
            channel
                .exec(true, command)
                .await
                .map_err(|e| e.to_string())?;
            let mut output = Vec::new();
            let mut code = None;
            while let Some(message) = channel.wait().await {
                match message {
                    russh::ChannelMsg::Data { data }
                    | russh::ChannelMsg::ExtendedData { data, .. } => {
                        if output.len() + data.len() > 65536 {
                            let _ = channel.close().await;
                            return Err("探测输出超过上限".into());
                        }
                        output.extend_from_slice(&data);
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                    _ => {}
                }
            }
            Ok((output, code.ok_or("远端未返回退出码")?))
        })
        .await
        .map_err(|_| "探测命令超时")?
    }
}
