//! PTY fallback execution with portable-pty and vte stable frame capture (docs/01 §7.2, P4.m3c).
//!
//! Fixed size: 200x50.
//! Captures stable frames (300ms without output change).
//! Terminated immediately after completion; never leaves persistent PTY processes.

use std::{
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use vte::{Params, Perform};

const PTY_COLS: usize = 200;
const PTY_ROWS: usize = 50;
const STABLE_FRAME_MS: u64 = 300;

/// In-memory 200x50 virtual screen backed by vte.
#[derive(Clone, Debug)]
pub struct VirtualScreen {
    pub cols: usize,
    pub rows: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub grid: Vec<Vec<char>>,
}

impl VirtualScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            grid: vec![vec![' '; cols]; rows],
        }
    }

    pub fn snapshot(&self) -> String {
        let mut out = String::new();
        for row in &self.grid {
            let line: String = row.iter().collect();
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        out
    }

    fn scroll_up(&mut self) {
        if self.rows > 1 {
            self.grid.remove(0);
            self.grid.push(vec![' '; self.cols]);
        }
    }

    fn clear_all(&mut self) {
        for row in &mut self.grid {
            for cell in row {
                *cell = ' ';
            }
        }
    }

    fn clear_line(&mut self, y: usize) {
        if y < self.rows {
            for cell in &mut self.grid[y] {
                *cell = ' ';
            }
        }
    }
}

impl Perform for VirtualScreen {
    fn print(&mut self, c: char) {
        if self.cursor_y >= self.rows {
            self.scroll_up();
            self.cursor_y = self.rows - 1;
        }
        if self.cursor_x < self.cols {
            self.grid[self.cursor_y][self.cursor_x] = c;
            self.cursor_x += 1;
        } else {
            // Line wrap
            self.scroll_up();
            self.cursor_y = self.rows - 1;
            self.cursor_x = 0;
            if self.cursor_x < self.cols {
                self.grid[self.cursor_y][self.cursor_x] = c;
                self.cursor_x += 1;
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                if self.cursor_y + 1 >= self.rows {
                    self.scroll_up();
                } else {
                    self.cursor_y += 1;
                }
            }
            b'\r' => {
                self.cursor_x = 0;
            }
            b'\x08' => {
                self.cursor_x = self.cursor_x.saturating_sub(1);
            }
            b'\t' => {
                let next_tab = (self.cursor_x + 8) / 8 * 8;
                self.cursor_x = next_tab.min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            // Cursor Position (CUP): row, col (1-based)
            'H' | 'f' => {
                let mut iter = params.iter();
                let row = iter.next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                let col = iter.next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                self.cursor_y = (row - 1).min(self.rows - 1);
                self.cursor_x = (col - 1).min(self.cols - 1);
            }
            // Erase in Display (ED)
            'J' => {
                let mode = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(0);
                if mode == 2 || mode == 3 {
                    self.clear_all();
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                }
            }
            // Erase in Line (EL)
            'K' => {
                let mode = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(0);
                if mode == 2 {
                    self.clear_line(self.cursor_y);
                } else if mode == 0 && self.cursor_y < self.rows {
                    for x in self.cursor_x..self.cols {
                        self.grid[self.cursor_y][x] = ' ';
                    }
                }
            }
            // Cursor Up (CUU)
            'A' => {
                let n = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            // Cursor Down (CUD)
            'B' => {
                let n = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                self.cursor_y = (self.cursor_y + n).min(self.rows - 1);
            }
            // Cursor Forward (CUF)
            'C' => {
                let n = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                self.cursor_x = (self.cursor_x + n).min(self.cols - 1);
            }
            // Cursor Back (CUB)
            'D' => {
                let n = params.iter().next().and_then(|p| p.get(0).copied()).unwrap_or(1).max(1) as usize;
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

/// Executes an interactive command in a 200x50 PTY, capturing output through vte
/// until a stable frame (300ms idle) or termination (docs/01 §7.2, docs/06 §4.1).
#[derive(Default)]
pub struct InteractiveOptions {
    pub input_delay: Duration,
    /// Caller must use an authorized scratch directory; this never persists folder trust.
    pub accept_folder_once: bool,
    /// Keep waiting through loading frames; optionally accept a complete animated frame at the deadline.
    pub frame_ready: Option<fn(&str) -> bool>,
    /// Do not type until a known input screen is visible (never answer trust/login dialogs).
    pub input_ready: Option<fn(&str) -> bool>,
    /// Type without Enter first, then require the CLI's built-in command completion.
    pub submit_ready: Option<fn(&str) -> bool>,
}

struct PtyChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
impl std::ops::Deref for PtyChildGuard {
    type Target = dyn portable_pty::Child + Send + Sync;
    fn deref(&self) -> &Self::Target { &*self.0 }
}
impl std::ops::DerefMut for PtyChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.0 }
}
impl Drop for PtyChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.process_id() {
            // portable-pty creates a session leader; never signal an inherited group.
            let pid = pid as i32;
            if unsafe { libc::getpgid(pid) } == pid { unsafe { libc::killpg(pid, libc::SIGKILL); } }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub async fn execute_pty_interactive(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    env_vars: &[(String, String)],
    send_input: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    execute_pty_interactive_after(exe, args, cwd, env_vars, send_input, timeout, InteractiveOptions::default()).await
}

/// Some TUIs render before their input handler is ready. Delay input for those CLIs.
pub async fn execute_pty_interactive_after(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    env_vars: &[(String, String)],
    send_input: Option<&str>,
    timeout: Duration,
    options: InteractiveOptions,
) -> Result<String, String> {
    let InteractiveOptions { input_delay, accept_folder_once, frame_ready, input_ready, submit_ready } = options;
    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS as u16,
            cols: PTY_COLS as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {e}"))?;

    let mut cmd = CommandBuilder::new(exe);
    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env_clear();
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    // Set TERM to xterm-256color for clean VT100/ANSI emulation
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn child in PTY: {e}"))?;
    let mut child = PtyChildGuard(child);

    drop(pair.slave);
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to take PTY writer: {e}"))?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {e}"))?;

    // Background thread reading from PTY master reader
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut screen = VirtualScreen::new(PTY_COLS, PTY_ROWS);
    let mut parser = vte::Parser::new();

    let input_sent_cell = std::sync::atomic::AtomicBool::new(send_input.is_none());
    let started = tokio::time::Instant::now();
    let deadline = started + timeout;

    let mut stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
    let mut accepted_folder = false;
    let mut input_not_before = started + input_delay;
    let mut input_started = None;
    let mut input_typed = false;
    let mut had_output = false;
    let mut last_stable_frame: String;

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = stop_tx.send(());
            if input_sent_cell.load(std::sync::atomic::Ordering::Relaxed) {
                let frame = screen.snapshot();
                if frame_ready.is_some_and(|ready| ready(&frame)) { return Ok(frame); }
            }
            return Err("PTY execution timed out".into());
        }

        let time_to_stable = stable_deadline.saturating_duration_since(now);
        let time_to_timeout = deadline.saturating_duration_since(now);
        let wait_dur = time_to_stable.min(time_to_timeout);

        tokio::select! {
            chunk = rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        had_output = true;
                        parser.advance(&mut screen, &bytes);
                        stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                    }
                    None => {
                        // Preserve output received since the previous stable frame.
                        last_stable_frame = screen.snapshot();
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(wait_dur) => {
                if had_output && tokio::time::Instant::now() >= stable_deadline {
                    // Stable frame reached!
                    last_stable_frame = screen.snapshot();
                    had_output = false;

                    // If we need to send input (e.g. slash command /usage), send now after initial prompt is stable
                    if !input_sent_cell.load(std::sync::atomic::Ordering::Relaxed) {
                        if tokio::time::Instant::now() < input_not_before {
                            had_output = true;
                            stable_deadline = input_not_before;
                            continue;
                        }
                        if accept_folder_once && !accepted_folder && last_stable_frame.contains("Do you trust the files in this folder?") {
                            // The default choice trusts this session only, never “remember this folder”.
                            use std::io::Write;
                            writer.write_all(b"\r").map_err(|e| e.to_string())?;
                            writer.flush().map_err(|e| e.to_string())?;
                            accepted_folder = true;
                            input_not_before = tokio::time::Instant::now() + Duration::from_secs(1);
                            stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                            continue;
                        }
                        if let Some(inp) = send_input {
                            use std::io::Write;
                            if let Some(ready) = submit_ready {
                                if !input_typed {
                                    if input_ready.map_or(true, |ready| !ready(&last_stable_frame)) {
                                        had_output = true;
                                        stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                                        continue;
                                    }
                                    writer.write_all(inp.trim_end_matches(['\r', '\n']).as_bytes()).map_err(|e| e.to_string())?;
                                    writer.flush().map_err(|e| e.to_string())?;
                                    input_typed = true;
                                    had_output = true;
                                    stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                                    continue;
                                }
                                if !ready(&last_stable_frame) {
                                    had_output = true;
                                    stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                                    continue;
                                }
                                writer.write_all(b"\r").map_err(|e| e.to_string())?;
                            } else if !input_delay.is_zero() {
                                // Separate pasted text and Enter: Copilot treats a combined chunk as paste.
                                let text = inp.trim_end_matches(['\r', '\n']);
                                writer.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                                writer.flush().map_err(|e| e.to_string())?;
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                writer.write_all(b"\r").map_err(|e| e.to_string())?;
                            } else { writer.write_all(inp.as_bytes()).map_err(|e| e.to_string())?; }
                            writer.flush().map_err(|e| e.to_string())?;
                            input_sent_cell.store(true, std::sync::atomic::Ordering::Relaxed);
                            input_started = Some(tokio::time::Instant::now());
                            // Reset stable deadline to wait for output produced by the command
                            stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                            continue;
                        }
                    } else {
                        // Give asynchronous slash commands time to render their result after input echo.
                        if let Some(sent) = input_started.filter(|sent| !input_delay.is_zero() && sent.elapsed() < Duration::from_secs(3)) {
                            had_output = true;
                            stable_deadline = sent + Duration::from_secs(3);
                            continue;
                        }
                        if frame_ready.is_some_and(|ready| !ready(&last_stable_frame)) {
                            had_output = true;
                            stable_deadline = tokio::time::Instant::now() + Duration::from_millis(STABLE_FRAME_MS);
                            continue;
                        }
                        break;
                    }
                }
            }
        }

        // Check if child process exited
        if let Ok(Some(_status)) = child.try_wait() {
            // The reader thread can deliver the final output after the exit status.
            // Drain it before capturing the screen, with a bound for inherited PTY handles.
            while let Ok(Some(bytes)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                parser.advance(&mut screen, &bytes);
            }
            last_stable_frame = screen.snapshot();
            break;
        }
    }

    let _ = child.kill();
    let _ = stop_tx.send(());

    if last_stable_frame.is_empty() {
        last_stable_frame = screen.snapshot();
    }

    Ok(last_stable_frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_screen_basic_and_cursor() {
        let mut screen = VirtualScreen::new(20, 5);
        let mut parser = vte::Parser::new();

        let input = b"Hello, World!\r\nLine 2";
        parser.advance(&mut screen, input);

        let snap = screen.snapshot();
        assert!(snap.contains("Hello, World!"));
        assert!(snap.contains("Line 2"));
    }

    #[test]
    fn test_virtual_screen_ansi_codes() {
        let mut screen = VirtualScreen::new(40, 5);
        let mut parser = vte::Parser::new();

        // CSI 2J clears screen, \x1b[1;1H sets cursor to 1,1
        let input = b"Old Junk\x1b[2J\x1b[1;1HNew Content";
        parser.advance(&mut screen, input);

        let snap = screen.snapshot();
        assert!(!snap.contains("Old Junk"));
        assert!(snap.contains("New Content"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delayed_slash_input_survives_session_only_folder_trust() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("tui.sh");
        std::fs::write(&script, r#"
printf 'Do you trust the files in this folder?\n1. Yes\n2. Yes, and remember\n'
read -r choice
test -z "$choice" || exit 1
printf 'Ready>\n'
read -r command
test "$command" = /usage || exit 2
sleep 1
printf 'Plan 29%% used\n'
"#).unwrap();
        let text = execute_pty_interactive_after(std::path::Path::new("/bin/sh"), &[script.to_str().unwrap()], dir.path(), &[], Some("/usage\r"), Duration::from_secs(10), InteractiveOptions { input_delay: Duration::from_millis(100), accept_folder_once: true, ..Default::default() }).await.unwrap();
        assert!(text.contains("Plan 29% used"), "{text}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn completion_predicate_waits_for_results_and_handles_animation() {
        fn ready(frame: &str) -> bool { frame.contains("Complete quota 7% used") }
        for animated in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("quota.sh");
            let tail = if animated {
                "while true; do printf '\\rComplete quota 7%% used'; sleep 0.05; done"
            } else { "printf 'Complete quota 7%% used\\n'; exec sleep 5" };
            std::fs::write(&script, format!("printf 'Ready>\\n'\nread -r command\nprintf 'Loading quota\\n'\nsleep 1\n{tail}\n")).unwrap();
            let frame = execute_pty_interactive_after(Path::new("/bin/sh"), &[script.to_str().unwrap()],
                dir.path(), &[], Some("/usage\r"), Duration::from_secs(3),
                InteractiveOptions { frame_ready: Some(ready), ..Default::default() }).await.unwrap();
            assert!(ready(&frame), "animated={animated}: {frame}");
        }
    }

    #[tokio::test]
    async fn test_pty_echo_stable_frame() {
        let echo_cmd = std::path::Path::new("/bin/echo");
        if !echo_cmd.exists() {
            return;
        }

        let out = execute_pty_interactive(
            echo_cmd,
            &["Tokage PTY Frame Test"],
            std::path::Path::new("/tmp"),
            &[],
            None,
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        assert!(out.contains("Tokage PTY Frame Test"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_command_requires_builtin_completion_before_enter() {
        fn input_ready(frame: &str) -> bool { frame.contains("Ready>") }
        fn submit_ready(frame: &str) -> bool { frame.contains("/usage Show usage") }
        fn complete(frame: &str) -> bool { frame.contains("Complete quota") }
        let python = which::which("python3").expect("python3 is required for the raw PTY regression");
        for recognized in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("tui.py");
            let submitted = dir.path().join("submitted");
            std::fs::write(&script, format!(r#"
import os, sys, tty
tty.setraw(sys.stdin.fileno())
os.write(1, b'Ready>\r\n')
typed = b''
while len(typed) < 6:
    typed += os.read(0, 6 - len(typed))
assert typed == b'/usage'
os.write(1, {completion:?}.encode())
assert os.read(0, 1) == b'\r'
open({path:?}, 'w').write('submitted')
os.write(1, b'Complete quota\r\n')
"#, completion=if recognized { "/usage Show usage\r\n" } else { "Unknown command\r\n" }, path=submitted.to_str().unwrap())).unwrap();
            let result = execute_pty_interactive_after(&python, &[script.to_str().unwrap()], dir.path(),
                &[], Some("/usage\r"), Duration::from_secs(3), InteractiveOptions {
                    input_ready: Some(input_ready), submit_ready: Some(submit_ready), frame_ready: Some(complete), ..Default::default()
                }).await;
            if recognized { assert!(result.unwrap().contains("Complete quota")); }
            else { assert!(result.is_err()); }
            assert_eq!(submitted.exists(), recognized, "unknown commands must not be submitted as prompts");
        }
    }
}
