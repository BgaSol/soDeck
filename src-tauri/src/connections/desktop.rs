use super::{Answer, View};
use crate::{windows::DesktopWindows, workspaces::Context};
use serde::Deserialize;
use tauri::{Manager, State, WebviewWindow};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    Start {
        server_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Cancel {
        attempt_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Answer {
        attempt_id: String,
        question_id: String,
        answer: Answer,
    },
    Disconnect {
        context: Context,
    },
    Focus {
        context: Context,
    },
    Snapshot,
}
#[tauri::command]
pub async fn connection(
    window: WebviewWindow,
    desktop: State<'_, DesktopWindows>,
    command: Command,
) -> Result<Vec<View>, String> {
    let core = desktop.core.clone();
    let label = window.label().to_owned();
    let focus = match command {
        Command::Start { server_id } => core.start_connection(&label, &server_id)?.context,
        Command::Cancel { attempt_id } => {
            core.cancel_connection(&label, &attempt_id)?;
            None
        }
        Command::Answer {
            attempt_id,
            question_id,
            answer,
        } => {
            core.answer_connection(&label, &attempt_id, &question_id, answer)?;
            None
        }
        Command::Disconnect { context } => {
            core.disconnect_connection(&label, &context)?;
            None
        }
        Command::Focus { context } => Some(context),
        Command::Snapshot => None,
    };
    if let Some(context) = focus {
        let mut state = core.workspace_state();
        state.validate_identity(&context)?;
        let owner = state.owner(&context.workspace_id).ok_or("工作区已关闭")?;
        state.activate(&owner, Some(&context.workspace_id))?;
        drop(state);
        if let Some(target) = window.app_handle().get_webview_window(&owner) {
            let _ = target.show();
            let _ = target.set_focus();
        }
        core.workspace_changed();
    }
    Ok(core.connection_views())
}
