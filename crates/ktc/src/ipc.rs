use ktc_common::{ipc_socket_path, IpcCommand, IpcEvent, WorkspaceInfo};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::{UnixListener, UnixStream};

pub struct IpcServer {
    listener: UnixListener,
    clients: HashMap<u64, IpcClient>,
    next_client_id: u64,
}

struct IpcClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl IpcServer {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let socket_path = ipc_socket_path();

        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        listener.set_nonblocking(true)?;

        log::info!("IPC server listening on {}", socket_path.display());

        Ok(Self {
            listener,
            clients: HashMap::new(),
            next_client_id: 0,
        })
    }

    pub fn fd(&self) -> BorrowedFd<'_> {
        self.listener.as_fd()
    }

    pub fn accept_connections(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(e) = stream.set_nonblocking(true) {
                        log::warn!("Failed to set IPC client non-blocking: {}", e);
                        continue;
                    }

                    let id = self.next_client_id;
                    self.next_client_id += 1;

                    let reader = BufReader::new(match stream.try_clone() {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("Failed to clone stream: {}", e);
                            continue;
                        }
                    });

                    self.clients.insert(id, IpcClient { stream, reader });
                    log::info!("IPC client {} connected", id);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    log::warn!("IPC accept error: {}", e);
                    break;
                }
            }
        }
    }

    pub fn poll_commands(&mut self) -> Vec<IpcCommand> {
        let mut commands = Vec::new();
        let mut disconnected = Vec::new();

        for (&id, client) in &mut self.clients {
            let mut line = String::new();
            loop {
                line.clear();
                match client.reader.read_line(&mut line) {
                    Ok(0) => {
                        disconnected.push(id);
                        break;
                    }
                    Ok(_) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<IpcCommand>(line) {
                            Ok(cmd) => commands.push(cmd),
                            Err(e) => log::warn!("Invalid IPC command from {}: {}", id, e),
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        log::warn!("IPC read error from {}: {}", id, e);
                        disconnected.push(id);
                        break;
                    }
                }
            }
        }

        for id in disconnected {
            self.clients.remove(&id);
            log::info!("IPC client {} disconnected", id);
        }

        commands
    }

    pub fn broadcast(&mut self, event: &IpcEvent) {
        let json = match serde_json::to_string(event) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to serialize IPC event: {}", e);
                return;
            }
        };

        let msg = format!("{}\n", json);
        let mut disconnected = Vec::new();

        for (&id, client) in &mut self.clients {
            if let Err(e) = client.stream.write_all(msg.as_bytes()) {
                log::warn!("Failed to send to IPC client {}: {}", id, e);
                disconnected.push(id);
            }
        }

        for id in disconnected {
            self.clients.remove(&id);
        }
    }

    pub fn send_state(
        &mut self,
        workspaces: Vec<WorkspaceInfo>,
        active: usize,
        focused_title: Option<String>,
    ) {
        let event = IpcEvent::State {
            workspaces,
            active_workspace: active,
            focused_window: focused_title,
        };
        self.broadcast(&event);
    }

    pub fn notify_workspace_change(&mut self, workspaces: Vec<WorkspaceInfo>, active: usize) {
        log::debug!(
            "[ipc] Broadcasting workspace change: active={} clients={}",
            active,
            self.clients.len()
        );
        let event = IpcEvent::WorkspaceChanged {
            workspaces,
            active_workspace: active,
        };
        self.broadcast(&event);
    }

    pub fn notify_focus_change(&mut self, title: Option<String>) {
        let event = IpcEvent::FocusChanged {
            window_title: title,
        };
        self.broadcast(&event);
    }

    pub fn notify_title_change(&mut self, title: String) {
        let event = IpcEvent::TitleChanged {
            window_title: title,
        };
        self.broadcast(&event);
    }

    #[allow(dead_code)]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let socket_path = ipc_socket_path();
        let _ = std::fs::remove_file(socket_path);
    }
}
