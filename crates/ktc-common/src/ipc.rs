use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcEvent {
    #[serde(rename = "state")]
    State {
        workspaces: Vec<WorkspaceInfo>,
        active_workspace: usize,
        focused_window: Option<String>,
    },
    #[serde(rename = "workspace")]
    WorkspaceChanged {
        workspaces: Vec<WorkspaceInfo>,
        active_workspace: usize,
    },
    #[serde(rename = "focus")]
    FocusChanged { window_title: Option<String> },
    #[serde(rename = "title")]
    TitleChanged { window_title: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcCommand {
    #[serde(rename = "get_state")]
    GetState,
    #[serde(rename = "switch_workspace")]
    SwitchWorkspace { workspace: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: usize,
    pub name: String,
    pub window_count: usize,
    pub urgent: bool,
}

impl WorkspaceInfo {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            name: id.to_string(),
            window_count: 0,
            urgent: false,
        }
    }
}

pub fn ipc_socket_path() -> std::path::PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime_dir).join("ktc.sock")
    } else {
        std::path::PathBuf::from("/tmp").join(format!("ktc-{}.sock", unsafe { libc::getuid() }))
    }
}
