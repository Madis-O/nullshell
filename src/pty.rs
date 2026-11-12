//! ConPTY host: spawns the shell and moves bytes in and out on worker threads.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::config::ShellConfig;

/// Sent to the owner from worker threads.
#[derive(Debug, Clone, Copy)]
pub enum PtyEvent {
    /// New bytes are waiting in [`Pty::output`].
    Output,
    /// The shell process ended.
    Exit,
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    output: Receiver<Vec<u8>>,
    output_pending: Arc<AtomicBool>,
    /// Short display name of the running shell, e.g. `pwsh`.
    pub shell_name: String,
}

impl Pty {
    /// Spawn the configured shell (or the first available fallback).
    /// `notify` is called from worker threads and must be cheap.
    pub fn spawn<F>(shell: &ShellConfig, cols: u16, rows: u16, notify: F) -> Result<Self>
    where
        F: Fn(PtyEvent) + Send + Clone + 'static,
    {
        let size = PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 };
        let pair = native_pty_system().openpty(size).context("failed to open ConPTY")?;

        let mut last_err = None;
        let mut spawned = None;
        for (program, args) in shell_candidates(shell) {
            let mut cmd = CommandBuilder::new(&program);
            cmd.args(&args);
            cmd.cwd(working_directory(shell));
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
            cmd.env("TERM_PROGRAM", "NullShell");
            match pair.slave.spawn_command(cmd) {
                Ok(child) => {
                    spawned = Some((child, program));
                    break;
                }
                Err(e) => last_err = Some(anyhow!("{}: {e}", program.display())),
            }
        }
        let (mut child, program) = spawned
            .ok_or_else(|| last_err.unwrap_or_else(|| anyhow!("no shell found")))
            .context("failed to start a shell")?;
        drop(pair.slave);

        let shell_name = program
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| "shell".into());

        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().context("pty reader")?;
        let mut pty_writer = pair.master.take_writer().context("pty writer")?;

        // Reader: pty -> channel. Bounded so a flood of output applies backpressure.
        let (out_tx, output) = mpsc::sync_channel::<Vec<u8>>(64);
        let output_pending = Arc::new(AtomicBool::new(false));
        {
            let pending = output_pending.clone();
            let notify = notify.clone();
            thread::Builder::new().name("pty-reader".into()).spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if out_tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                            if !pending.swap(true, Ordering::AcqRel) {
                                notify(PtyEvent::Output);
                            }
                        }
                    }
                }
            })?;
        }

        // Writer: channel -> pty, so a slow child never blocks the UI thread.
        let (writer, in_rx) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new().name("pty-writer".into()).spawn(move || {
            while let Ok(bytes) = in_rx.recv() {
                if pty_writer.write_all(&bytes).is_err() || pty_writer.flush().is_err() {
                    break;
                }
            }
        })?;

        // Waiter: ConPTY does not signal EOF on the pipe when the child exits,
        // so the process handle is watched directly.
        thread::Builder::new().name("pty-wait".into()).spawn(move || {
            let _ = child.wait();
            notify(PtyEvent::Exit);
        })?;

        Ok(Self { master: pair.master, writer, killer, output, output_pending, shell_name })
    }

    /// Queue bytes for the shell's stdin.
    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let bytes = bytes.into();
        if !bytes.is_empty() {
            let _ = self.writer.send(bytes);
        }
    }

    /// Next chunk of shell output, if any is waiting.
    pub fn try_read(&self) -> Option<Vec<u8>> {
        self.output.try_recv().ok()
    }

    /// Call before draining output so the reader thread will notify again.
    pub fn begin_drain(&self) {
        self.output_pending.store(false, Ordering::Release);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Terminate the shell (used when the window is closed first).
    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }
}

fn working_directory(shell: &ShellConfig) -> PathBuf {
    if !shell.working_directory.is_empty() {
        let p = PathBuf::from(&shell.working_directory);
        if p.is_dir() {
            return p;
        }
    }
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// Shells to try, in order: the configured one, then pwsh, powershell, cmd.
fn shell_candidates(shell: &ShellConfig) -> Vec<(PathBuf, Vec<String>)> {
    let mut list = Vec::new();
    if !shell.program.is_empty() {
        let p = PathBuf::from(&shell.program);
        let resolved = if p.is_file() { Some(p) } else { find_in_path(&shell.program) };
        if let Some(p) = resolved {
            list.push((p, shell.args.clone()));
        }
    }

    let mut pwsh = find_in_path("pwsh.exe");
    if pwsh.is_none() {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(base) = std::env::var_os(var) {
                let p = PathBuf::from(base).join("PowerShell").join("7").join("pwsh.exe");
                if p.is_file() {
                    pwsh = Some(p);
                    break;
                }
            }
        }
    }
    if let Some(p) = pwsh {
        list.push((p, vec!["-NoLogo".into()]));
    }
    if let Some(p) = find_in_path("powershell.exe") {
        list.push((p, vec!["-NoLogo".into()]));
    }
    list.push((
        find_in_path("cmd.exe").unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()))
        }),
        Vec::new(),
    ));
    list
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let has_ext = Path::new(name).extension().is_some();
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !has_ext {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}
