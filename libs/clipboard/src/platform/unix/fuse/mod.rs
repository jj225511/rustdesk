mod cs;

use super::filetype::FileDescription;
use crate::{ClipboardFile, CliprdrError};
use cs::FuseServer;
use fuser::MountOption;
use hbb_common::{config::APP_NAME, log};
use parking_lot::Mutex;
use std::{
    path::PathBuf,
    sync::{mpsc::Sender, Arc},
    time::Duration,
};

lazy_static::lazy_static! {
    static ref FUSE_MOUNT_POINT_CLIENT: Arc<String> = {
        let mnt_path = format!("/tmp/{}/{}", APP_NAME.read().unwrap(), "cliprdr-client");
        // No need to run `canonicalize()` here.
        Arc::new(mnt_path)
    };

    static ref FUSE_MOUNT_POINT_SERVER: Arc<String> = {
        let mnt_path = format!("/tmp/{}/{}", APP_NAME.read().unwrap(), "cliprdr-server");
        // No need to run `canonicalize()` here.
        Arc::new(mnt_path)
    };

    static ref FUSE_CONTEXT_CLIENT: Arc<Mutex<Option<FuseContext>>> = Arc::new(Mutex::new(None));
    static ref FUSE_CONTEXT_SERVER: Arc<Mutex<Option<FuseContext>>> = Arc::new(Mutex::new(None));
}

static FUSE_TIMEOUT: Duration = Duration::from_secs(3);

pub fn get_exclude_paths(is_client: bool) -> Arc<String> {
    if is_client {
        FUSE_MOUNT_POINT_CLIENT.clone()
    } else {
        FUSE_MOUNT_POINT_SERVER.clone()
    }
}

pub fn is_fuse_context_inited(is_client: bool) -> bool {
    if is_client {
        FUSE_CONTEXT_CLIENT.lock().is_some()
    } else {
        FUSE_CONTEXT_SERVER.lock().is_some()
    }
}

pub fn init_fuse_context(is_client: bool) -> Result<(), CliprdrError> {
    let mut fuse_context_lock = if is_client {
        FUSE_CONTEXT_CLIENT.lock()
    } else {
        FUSE_CONTEXT_SERVER.lock()
    };
    if fuse_context_lock.is_some() {
        return Ok(());
    }
    let mount_point = if is_client {
        FUSE_MOUNT_POINT_CLIENT.clone()
    } else {
        FUSE_MOUNT_POINT_SERVER.clone()
    };

    let mount_point = std::path::PathBuf::from(&*mount_point);
    let (server, tx) = FuseServer::new(FUSE_TIMEOUT);
    let server = Arc::new(Mutex::new(server));

    prepare_fuse_mount_point(&mount_point)?;

    let mnt_opts = [
        MountOption::FSName("rustdesk-cliprdr-fs".to_string()),
        MountOption::NoAtime,
        MountOption::RO,
    ];
    log::info!("mounting clipboard FUSE to {}", mount_point.display());

    // Try to mount with retry logic
    let max_retries = 3;
    let mut retry_delay = Duration::from_millis(100);
    let mut last_error = None;

    for attempt in 1..=max_retries {
        log::debug!("FUSE mount attempt {} of {}", attempt, max_retries);

        match fuser::spawn_mount2(
            FuseServer::client(server.clone()),
            mount_point.clone(),
            &mnt_opts,
        ) {
            Ok(session) => {
                log::info!(
                    "Successfully mounted FUSE filesystem on attempt {}",
                    attempt
                );
                let session = Mutex::new(Some(session));
                let ctx = FuseContext {
                    server,
                    tx,
                    mount_point,
                    session,
                    conn_id: 0,
                };
                *fuse_context_lock = Some(ctx);
                return Ok(());
            }
            Err(e) => {
                last_error = Some(e);
                log::error!("FUSE mount attempt {} failed: {:?}", attempt, last_error);

                if attempt < max_retries {
                    // Clean up and retry
                    log::info!(
                        "Cleaning up mount point and retrying after {:?}",
                        retry_delay
                    );
                    cleanup_mount_point(&mount_point);
                    std::thread::sleep(retry_delay);
                    retry_delay *= 2; // Exponential backoff

                    // Re-prepare mount point for next attempt
                    if let Err(e) = prepare_fuse_mount_point(&mount_point) {
                        log::error!("Failed to prepare mount point for retry: {:?}", e);
                    }
                }
            }
        }
    }

    log::error!(
        "Failed to mount FUSE after {} attempts: {:?}",
        max_retries,
        last_error
    );

    Err(CliprdrError::CliprdrInit)
}

pub fn uninit_fuse_context(is_client: bool) {
    uninit_fuse_context_(is_client)
}

pub fn format_data_response_to_urls(
    is_client: bool,
    format_data: Vec<u8>,
    conn_id: i32,
) -> Result<Vec<String>, CliprdrError> {
    let mut ctx = if is_client {
        FUSE_CONTEXT_CLIENT.lock()
    } else {
        FUSE_CONTEXT_SERVER.lock()
    };

    // If FUSE context is not initialized, return a more descriptive error
    if ctx.is_none() {
        log::debug!("FUSE context not initialized for format_data_response_to_urls");
        return Err(CliprdrError::CliprdrInit);
    }

    ctx.as_mut()
        .ok_or(CliprdrError::CliprdrInit)?
        .format_data_response_to_urls(format_data, conn_id)
}

pub fn handle_file_content_response(
    is_client: bool,
    clip: ClipboardFile,
) -> Result<(), CliprdrError> {
    // we don't know its corresponding request, no resend can be performed
    let ctx = if is_client {
        FUSE_CONTEXT_CLIENT.lock()
    } else {
        FUSE_CONTEXT_SERVER.lock()
    };
    ctx.as_ref()
        .ok_or(CliprdrError::CliprdrInit)?
        .tx
        .send(clip)
        .map_err(|e| {
            log::error!("failed to send file contents response to fuse: {:?}", e);
            CliprdrError::ClipboardInternalError
        })?;
    Ok(())
}

pub fn empty_local_files(is_client: bool, conn_id: i32) -> bool {
    let ctx = if is_client {
        FUSE_CONTEXT_CLIENT.lock()
    } else {
        FUSE_CONTEXT_SERVER.lock()
    };
    ctx.as_ref()
        .map(|c| c.empty_local_files(conn_id))
        .unwrap_or(false)
}

struct FuseContext {
    server: Arc<Mutex<FuseServer>>,
    tx: Sender<ClipboardFile>,
    mount_point: PathBuf,
    // stores fuse background session handle
    session: Mutex<Option<fuser::BackgroundSession>>,
    // Indicates the connection ID of that set the clipboard content
    conn_id: i32,
}

// This function must be called after the main IPC is up
fn prepare_fuse_mount_point(mount_point: &PathBuf) -> Result<(), CliprdrError> {
    use std::{
        fs::{self, Permissions},
        os::unix::prelude::PermissionsExt,
    };

    // Create parent directories if they don't exist
    if let Some(parent) = mount_point.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log::error!(
                "Failed to create parent directories for mount point: {:?}",
                e
            );
            return Err(CliprdrError::CliprdrInit);
        }
    }

    // Try to create the mount point directory
    match fs::create_dir(mount_point) {
        Ok(_) => log::debug!("Created mount point directory: {:?}", mount_point),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            log::debug!("Mount point already exists: {:?}", mount_point);
        }
        Err(e) => {
            log::error!("Failed to create mount point: {:?}", e);
            return Err(CliprdrError::CliprdrInit);
        }
    }

    // Set permissions
    if let Err(e) = fs::set_permissions(mount_point, Permissions::from_mode(0o777)) {
        log::warn!("Failed to set mount point permissions: {:?}", e);
    }

    // Clean up any existing mount
    cleanup_mount_point(mount_point);

    Ok(())
}

// Helper function to clean up mount point
fn cleanup_mount_point(mount_point: &PathBuf) {
    // Try different umount methods
    let umount_commands = [
        vec!["fusermount", "-u", mount_point.to_str().unwrap_or("")],
        vec!["fusermount3", "-u", mount_point.to_str().unwrap_or("")],
        vec!["umount", mount_point.to_str().unwrap_or("")],
    ];

    for cmd_parts in &umount_commands {
        if cmd_parts.is_empty() {
            continue;
        }

        match std::process::Command::new(cmd_parts[0])
            .args(&cmd_parts[1..])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    log::debug!(
                        "Successfully unmounted with {}: {:?}",
                        cmd_parts[0],
                        mount_point
                    );
                    break;
                } else {
                    log::debug!(
                        "Failed to unmount with {}: {}",
                        cmd_parts[0],
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                log::debug!("Command {} not available: {:?}", cmd_parts[0], e);
            }
        }
    }

    // Check if mount point is still mounted
    if is_mount_point_mounted(mount_point) {
        log::warn!(
            "Mount point still mounted after cleanup attempts: {:?}",
            mount_point
        );
    }
}

// Check if a path is currently a mount point
fn is_mount_point_mounted(path: &PathBuf) -> bool {
    match std::process::Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => {
            // fallback: check /proc/mounts
            if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
                let path_str = path.to_string_lossy();
                mounts
                    .lines()
                    .any(|line| line.split_whitespace().nth(1) == Some(&*path_str))
            } else {
                false
            }
        }
    }
}

fn uninit_fuse_context_(is_client: bool) {
    if is_client {
        let _ = FUSE_CONTEXT_CLIENT.lock().take();
    } else {
        let _ = FUSE_CONTEXT_SERVER.lock().take();
    }
}

impl Drop for FuseContext {
    fn drop(&mut self) {
        if let Some(session) = self.session.lock().take() {
            log::info!(
                "Shutting down FUSE session for {}",
                self.mount_point.display()
            );
            session.join();
        }

        // Clean up the mount point on drop
        cleanup_mount_point(&self.mount_point);
        log::info!(
            "Unmounted clipboard FUSE from {}",
            self.mount_point.display()
        );
    }
}

impl FuseContext {
    pub fn empty_local_files(&self, conn_id: i32) -> bool {
        if conn_id != 0 && self.conn_id != conn_id {
            return false;
        }
        let mut fuse_guard = self.server.lock();
        let _ = fuse_guard.load_file_list(vec![]);
        true
    }

    pub fn format_data_response_to_urls(
        &mut self,
        format_data: Vec<u8>,
        conn_id: i32,
    ) -> Result<Vec<String>, CliprdrError> {
        let files = FileDescription::parse_file_descriptors(format_data, conn_id)?;

        let paths = {
            let mut fuse_guard = self.server.lock();
            fuse_guard.load_file_list(files)?;
            self.conn_id = conn_id;

            fuse_guard.list_root()
        };

        let prefix = self.mount_point.clone();
        Ok(paths
            .into_iter()
            .map(|p| prefix.join(p).to_string_lossy().to_string())
            .collect())
    }
}
