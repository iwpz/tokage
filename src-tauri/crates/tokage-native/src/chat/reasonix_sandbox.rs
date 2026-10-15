//! macOS cannot reapply Seatbelt inside a restricted process. A turn-local IPC
//! broker launches Reasonix's shell commands as sandboxed siblings, preserving
//! both Tokage's boundary and the native command profile. No credential or user
//! configuration is copied or changed.

#[cfg(target_os = "macos")]
mod macos {
    use crate::{env::ShellEnv, sandbox};
    use serde::{Deserialize, Serialize};
    use std::{
        collections::HashMap,
        io::{BufRead, Write},
        path::{Path, PathBuf},
        process::Stdio,
    };
    use tokage_core::ProviderId;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    const HELPER: &str = "--tokage-reasonix-sandbox-helper";
    const MAX_REQUEST: usize = 1024 * 1024;
    const MAX_OUTPUT: usize = 8 * 1024 * 1024;
    #[derive(Serialize, Deserialize)]
    struct Request {
        args: Vec<String>,
        env: HashMap<String, String>,
        cwd: PathBuf,
    }
    #[derive(Serialize, Deserialize)]
    struct Response {
        code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    pub(crate) struct Runtime {
        dir: tempfile::TempDir,
        task: tokio::task::JoinHandle<()>,
        sbpl: String,
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn quote_path(path: &Path) -> String {
        format!(
            "\"{}\"",
            path.to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    }
    fn deny_write(sbpl: &mut String, path: &Path) {
        sbpl.push_str(&format!(
            "\n(deny file-write* (subpath {}))\n",
            quote_path(path)
        ));
        if let Ok(canonical) = path.canonicalize() {
            sbpl.push_str(&format!(
                "(deny file-write* (subpath {}))\n",
                quote_path(&canonical)
            ));
        }
    }

    impl Runtime {
        pub(crate) async fn start(
            env: &ShellEnv,
            cwd: &str,
            app_data: Option<&str>,
            port: Option<u16>,
            read_only: bool,
        ) -> Result<Self, String> {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::Builder::new()
                .prefix("tokage-rx-broker-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            let socket = dir.path().join("broker.sock");
            let listener = tokio::net::UnixListener::bind(&socket).map_err(|e| e.to_string())?;
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let helper = super::super::adapter::shell_single_quote(&exe.to_string_lossy());
            // The lib test harness is not the desktop entry point. Its ignored
            // helper executes precisely the same IPC client as the shipped app.
            #[cfg(test)]
            let helper = format!("{helper} --exact chat::reasonix_sandbox::macos::tests::helper --ignored --nocapture --");
            #[cfg(test)]
            let redirect = "exec 3>&1\n";
            #[cfg(not(test))]
            let redirect = "";
            #[cfg(test)]
            let suffix = " >/dev/null";
            #[cfg(not(test))]
            let suffix = "";
            let wrapper = format!(
                "#!/bin/sh\n{redirect}exec {helper} {HELPER} {} \"$@\"{suffix}\n",
                super::super::adapter::shell_single_quote(&socket.to_string_lossy())
            );
            let wrapper_path = dir.path().join("sandbox-exec");
            std::fs::write(&wrapper_path, wrapper).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&wrapper_path, std::fs::Permissions::from_mode(0o500))
                .map_err(|e| e.to_string())?;
            let profile = sandbox::builtin_profiles()
                .remove(&ProviderId::Reasonix)
                .ok_or("Missing Reasonix sandbox profile")?;
            let home = env
                .vars
                .get("HOME")
                .ok_or("Missing HOME for Reasonix sandbox")?;
            let reasonix_home = env
                .vars
                .get("REASONIX_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(home).join(".reasonix"));
            let mut sbpl = sandbox::build_sbpl_for_turn(
                ProviderId::Reasonix,
                &profile,
                cwd,
                None,
                Some(home),
                app_data,
                port,
            )?;
            deny_write(&mut sbpl, dir.path());
            // A forced TCP proxy must not prevent the turn-local helper IPC.
            // Permit only this immutable socket, never arbitrary Unix sockets.
            if port.is_some() {
                let socket = socket.canonicalize().map_err(|e| e.to_string())?;
                sbpl.push_str(&format!(
                    "\n(allow network-outbound (literal {}))\n",
                    quote_path(&socket)
                ));
            }
            if read_only {
                deny_write(&mut sbpl, Path::new(cwd));
            }
            let mut tool_profile = profile.clone();
            // The CLI needs its own keys; its tools never do. The broker runs
            // outside the CLI and can therefore deny those files to tools only.
            for root in [
                reasonix_home.clone(),
                env.vars
                    .get("REASONIX_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| reasonix_home.clone()),
            ] {
                for name in [".env", "credentials", "remote", "schedules"] {
                    let path = root.join(name);
                    tool_profile.deny.push(path.to_string_lossy().into_owned());
                    if let Ok(path) = path.canonicalize() {
                        tool_profile.deny.push(path.to_string_lossy().into_owned());
                    }
                }
            }
            let mut baseline = sandbox::build_sbpl_for_turn(
                ProviderId::Reasonix,
                &tool_profile,
                cwd,
                None,
                Some(home),
                app_data,
                port,
            )?;
            deny_write(&mut baseline, dir.path());
            deny_write(&mut baseline, &reasonix_home);
            if let Some(state) = env.vars.get("REASONIX_STATE_HOME") {
                deny_write(&mut baseline, Path::new(state));
            }
            if read_only {
                deny_write(&mut baseline, Path::new(cwd));
            }
            // The socket is visible to the CLI. Freeze configured native bounds
            // in the host baseline too, so even a forged client profile cannot
            // relax them. This affects tools only, never model connectivity.
            baseline.push_str(&configured_bounds(&reasonix_home, cwd, env)?);
            let task = tokio::spawn(async move {
                let mut clients = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        connection = listener.accept() => match connection {
                            Ok((stream, _)) => { let baseline = baseline.clone(); clients.spawn(async move { serve(stream, baseline).await; }); }
                            Err(_) => break,
                        },
                        _ = clients.join_next(), if !clients.is_empty() => {},
                    }
                }
            });
            Ok(Self { dir, task, sbpl })
        }

        pub(crate) fn command(
            &self,
            exe: &Path,
            args: &[String],
        ) -> Result<tokio::process::Command, String> {
            let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
            cmd.arg("-p").arg(&self.sbpl).arg(exe).args(args);
            Ok(cmd)
        }
        pub(crate) fn apply_env(&self, cmd: &mut tokio::process::Command, env: &ShellEnv) {
            // Only the CLI's subprocess lookup is redirected. The outer host
            // invocation always uses the absolute real /usr/bin/sandbox-exec.
            let mut paths = vec![self.dir.path().to_path_buf()];
            paths.extend(std::env::split_paths(
                env.vars
                    .get("PATH")
                    .map(String::as_str)
                    .unwrap_or("/usr/bin:/bin"),
            ));
            if let Ok(path) = std::env::join_paths(paths) {
                cmd.env("PATH", path);
            }
        }
    }

    fn configured_bounds(home: &Path, cwd: &str, env: &ShellEnv) -> Result<String, String> {
        let mut result = String::new();
        for path in [
            home.join("config.toml"),
            Path::new(cwd).join("reasonix.toml"),
        ] {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("Cannot read Reasonix sandbox configuration".into()),
            };
            let config: toml::Value = text
                .parse()
                .map_err(|_| "Invalid Reasonix sandbox configuration")?;
            let Some(policy) = config.get("sandbox") else {
                continue;
            };
            if let Some(value) = policy.get("network") {
                match value.as_bool() {
                    Some(false) => result.push_str("\n(deny network*)\n"),
                    Some(true) => {}
                    None => return Err("Invalid Reasonix network policy".into()),
                }
            }
            if let Some(paths) = policy.get("forbid_read") {
                for value in paths
                    .as_array()
                    .ok_or("Invalid Reasonix forbid_read policy")?
                {
                    let raw = value.as_str().ok_or("Invalid Reasonix forbid_read path")?;
                    let mut expanded = raw.to_string();
                    for (name, value) in &env.vars {
                        expanded = expanded.replace(&format!("${{{name}}}"), value);
                    }
                    if expanded.contains('$') || !Path::new(&expanded).is_absolute() {
                        return Err("Cannot preserve unresolved Reasonix forbid_read path".into());
                    }
                    let path = PathBuf::from(expanded);
                    result.push_str(&format!(
                        "(deny file-read* (subpath {}))\n",
                        quote_path(&path)
                    ));
                    if let Ok(path) = path.canonicalize() {
                        result.push_str(&format!(
                            "(deny file-read* (subpath {}))\n",
                            quote_path(&path)
                        ));
                    }
                }
            }
        }
        Ok(result)
    }

    async fn serve(stream: tokio::net::UnixStream, baseline: String) {
        let (read, mut write) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(read);
        let mut bytes = Vec::new();
        // Bound malformed clients before allocating or launching a process.
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            (&mut reader)
                .take((MAX_REQUEST + 1) as u64)
                .read_until(b'\n', &mut bytes),
        )
        .await;
        let response = if !matches!(read, Ok(Ok(_))) || bytes.len() > MAX_REQUEST {
            Response {
                code: 1,
                stdout: vec![],
                stderr: b"Invalid Reasonix sandbox request\n".to_vec(),
            }
        } else {
            match serde_json::from_slice::<Request>(&bytes) {
                Ok(request) => execute(request, &baseline, &mut reader).await,
                Err(_) => Response {
                    code: 1,
                    stdout: vec![],
                    stderr: b"Invalid Reasonix sandbox request\n".to_vec(),
                },
            }
        };
        if let Ok(mut bytes) = serde_json::to_vec(&response) {
            bytes.push(b'\n');
            let _ = write.write_all(&bytes).await;
        }
    }

    async fn execute(
        request: Request,
        baseline: &str,
        reader: &mut tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    ) -> Response {
        let error = |message: String| Response {
            code: 1,
            stdout: vec![],
            stderr: format!("{message}\n").into_bytes(),
        };
        if request.args.len() < 3 || request.args[0] != "-p" || !request.args[2].starts_with('/') {
            return error("Unsupported Reasonix sandbox invocation".into());
        }
        // Native Available() probes only sandbox_apply with /usr/bin/true.
        // Its permissive profile is accepted solely for this exact no-op.
        let probe = request.args[2..] == ["/usr/bin/true"]
            && expressions(&request.args[1]).ok() == expressions("(version 1)(allow default)").ok();
        let profile = if probe {
            baseline.to_string()
        } else {
            match intersect(baseline, &request.args[1]) {
                Ok(p) => p,
                Err(e) => return error(e),
            }
        };
        let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
        cmd.arg("-p")
            .arg(profile)
            .args(&request.args[2..])
            .env_clear()
            .envs(&request.env)
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::proc::isolate(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return error(format!("Cannot start Reasonix sandbox command: {e}")),
        };
        let group = crate::proc::Group::with_dir_cmd(
            &child,
            crate::proc::get_run_dir(),
            Some("Reasonix sandbox command".into()),
        );
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let output = async {
            let (stdout, stderr, status) =
                tokio::try_join!(read_output(stdout), read_output(stderr), async {
                    child.wait().await.map_err(|e| e.to_string())
                })?;
            Ok::<_, String>(Response {
                code: status.code().unwrap_or(1),
                stdout,
                stderr,
            })
        };
        let mut disconnected = [0u8; 1];
        let response = tokio::select! {
            output = output => match output {
                Ok(response) => response,
                Err(e) => error(format!("Reasonix sandbox command failed: {e}")),
            },
            _ = reader.read(&mut disconnected) => error("Reasonix sandbox client disconnected".into()),
        };
        drop(group); // also kills descendants on cancellation / disconnect
        response
    }

    async fn read_output(pipe: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, String> {
        let mut bytes = vec![];
        pipe.take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_OUTPUT {
            return Err("Reasonix sandbox command output exceeds 8 MiB".into());
        }
        Ok(bytes)
    }

    // Conservative parser for the SBPL subset emitted by Reasonix. Unknown
    // operations/filters fail closed. Never concatenate untrusted allow rules:
    // a trailing allow would undo Tokage's earlier filesystem restrictions.
    #[derive(Clone, Debug, PartialEq)]
    enum Expr {
        Atom(String),
        String(String),
        List(Vec<Expr>),
    }
    impl Expr {
        fn atom(&self) -> Option<&str> {
            if let Self::Atom(s) = self {
                Some(s)
            } else {
                None
            }
        }
        fn render(&self) -> String {
            match self {
                Self::Atom(s) => s.clone(),
                Self::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
                Self::List(xs) => format!(
                    "({})",
                    xs.iter().map(Self::render).collect::<Vec<_>>().join(" ")
                ),
            }
        }
    }
    fn expressions(text: &str) -> Result<Vec<Expr>, String> {
        fn one(
            chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
            depth: usize,
        ) -> Result<Expr, String> {
            if depth > 16 {
                return Err("Reasonix sandbox profile too deeply nested".into());
            }
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            match chars.next() {
                Some('(') => {
                    let mut xs = vec![];
                    loop {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                        if chars.peek() == Some(&')') {
                            chars.next();
                            return Ok(Expr::List(xs));
                        }
                        if chars.peek().is_none() {
                            return Err("Unterminated Reasonix sandbox profile".into());
                        }
                        xs.push(one(chars, depth + 1)?);
                    }
                }
                Some('"') => {
                    let mut s = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '"' => return Ok(Expr::String(s)),
                            '\\' => match chars.next() {
                                Some(c @ ('"' | '\\')) => s.push(c),
                                _ => return Err("Unsupported SBPL escape".into()),
                            },
                            c => s.push(c),
                        }
                    }
                    Err("Unterminated SBPL string".into())
                }
                Some(c) if !c.is_whitespace() && c != ')' => {
                    let mut s = c.to_string();
                    while chars
                        .peek()
                        .is_some_and(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                    {
                        s.push(chars.next().unwrap());
                    }
                    Ok(Expr::Atom(s))
                }
                _ => Err("Invalid Reasonix sandbox profile".into()),
            }
        }
        let mut chars = text.chars().peekable();
        let mut out = vec![];
        loop {
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            if chars.peek().is_none() {
                return Ok(out);
            }
            out.push(one(&mut chars, 0)?);
        }
    }
    fn path_filter(expr: &Expr) -> bool {
        matches!(expr, Expr::List(xs) if xs.len() == 2 && matches!(xs[0].atom(), Some("subpath" | "literal"))
            && matches!(&xs[1], Expr::String(s) if s.starts_with('/') && !s.contains('\n') && !s.contains('\r') && !s.contains('\0')))
    }
    // These predicates occur only inside native deny rules. Preserve their
    // semantics verbatim; never use them to broaden an allow rule.
    fn deny_filter(expr: &Expr) -> bool {
        if path_filter(expr) {
            return true;
        }
        let Expr::List(xs) = expr else {
            return false;
        };
        match xs.first().and_then(Expr::atom) {
            Some("regex") => {
                matches!(xs.as_slice(), [_, Expr::String(s)] if !s.contains(['\n', '\r', '\0']))
            }
            Some("vnode-type") => xs.len() == 2 && xs[1].atom() == Some("SYMLINK"),
            Some("require-all" | "require-any") => xs.len() > 1 && xs[1..].iter().all(deny_filter),
            Some("require-not") => xs.len() == 2 && deny_filter(&xs[1]),
            _ => false,
        }
    }
    fn intersect(baseline: &str, native: &str) -> Result<String, String> {
        let mut writes = vec![];
        let mut denies = vec![];
        let mut version = false;
        let mut default = false;
        let mut write_default = false;
        let mut filtered_deny_seen = false;
        for expr in expressions(native)? {
            let Expr::List(xs) = &expr else {
                return Err("Invalid SBPL rule".into());
            };
            let atoms: Vec<_> = xs.iter().filter_map(Expr::atom).collect();
            match atoms.as_slice() {
                ["version", "1"] if xs.len() == 2 && !version => version = true,
                ["allow", "default"] if xs.len() == 2 && !default => default = true,
                ["deny", "file-write*"] if xs.len() == 2 && !write_default && writes.is_empty() => {
                    write_default = true
                }
                ["allow", "file-write*"]
                    if xs.len() > 2 && !filtered_deny_seen && xs[2..].iter().all(path_filter) =>
                {
                    writes.extend(xs[2..].iter().map(Expr::render))
                }
                ["deny", "file-read*"]
                | ["deny", "file-write*"]
                | ["deny", "file-write-unlink"]
                | ["deny", "file-write-create"]
                    if xs.len() > 2 && xs[2..].iter().all(deny_filter) =>
                {
                    filtered_deny_seen = true;
                    denies.push(expr.render());
                }
                ["deny", "network*"] if xs.len() == 2 => denies.push(expr.render()),
                // Native profiles allow a launchd Unix socket for runtime
                // compatibility. Never let that allow override a forced proxy
                // or native network deny; the outer default already permits it
                // when no stricter network boundary is in force.
                ["allow", "network-outbound"]
                    if xs.len() > 2 && xs[2..].iter().all(path_filter) => {}
                _ => {
                    return Err(format!(
                        "Unsupported Reasonix sandbox rule ({:?}); refusing to relax confinement",
                        atoms
                    ))
                }
            }
        }
        if !version || !default || !write_default || writes.is_empty() {
            return Err("Incomplete Reasonix sandbox profile".into());
        }
        let mut sbpl = baseline.to_string();
        // A deny of the complement intersects write roots instead of overriding
        // the outer allowlist. Native deny rules can only tighten it further.
        sbpl.push_str(&format!(
            "\n(deny file-write* (require-not (require-any {})))\n",
            writes.join(" ")
        ));
        for deny in denies {
            sbpl.push_str(&deny);
            sbpl.push('\n');
        }
        Ok(sbpl)
    }

    pub(super) fn helper(args: &[String]) -> Option<i32> {
        let index = args.iter().position(|a| a == HELPER)?;
        let result = (|| -> Result<Response, String> {
            let socket = args
                .get(index + 1)
                .ok_or("Missing Reasonix sandbox socket")?;
            let mut stream =
                std::os::unix::net::UnixStream::connect(socket).map_err(|e| e.to_string())?;
            let request = Request {
                args: args[index + 2..].to_vec(),
                env: std::env::vars().collect(),
                cwd: std::env::current_dir().map_err(|e| e.to_string())?,
            };
            let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            if bytes.len() > MAX_REQUEST {
                return Err("Reasonix sandbox request too large".into());
            }
            stream.write_all(&bytes).map_err(|e| e.to_string())?;
            let mut response = String::new();
            std::io::BufReader::new(stream)
                .read_line(&mut response)
                .map_err(|e| e.to_string())?;
            serde_json::from_str(&response)
                .map_err(|_| "Reasonix sandbox broker disconnected".into())
        })();
        Some(match result {
            Ok(response) => {
                #[cfg(test)]
                let mut stdout = {
                    use std::os::fd::FromRawFd;
                    unsafe { std::fs::File::from_raw_fd(3) }
                };
                #[cfg(not(test))]
                let mut stdout = std::io::stdout();
                let _ = stdout.write_all(&response.stdout);
                let _ = std::io::stderr().write_all(&response.stderr);
                response.code
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr(), "Reasonix sandbox helper: {e}");
                1
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn fixture(root: &Path) -> ShellEnv {
            std::fs::create_dir(root.join(".reasonix")).unwrap();
            std::fs::write(root.join(".reasonix/.env"), "TEST_KEY=synthetic-secret\n").unwrap();
            ShellEnv {
                vars: [
                    ("HOME".into(), root.to_string_lossy().into_owned()),
                    ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
                ]
                .into(),
            }
        }
        fn native_profile(root: &Path) -> String {
            format!("(version 1)(allow default)(deny file-write*)(allow file-write* (subpath {}) (subpath \"/dev\"))", quote_path(&root.canonicalize().unwrap()))
        }
        fn client(
            runtime: &Runtime,
            env: &ShellEnv,
            cwd: &Path,
            profile: &str,
            command: &str,
        ) -> tokio::process::Command {
            let args = vec![
                "-p".into(),
                profile.into(),
                "/bin/sh".into(),
                "-c".into(),
                command.into(),
            ];
            let mut cmd = runtime
                .command(&runtime.dir.path().join("sandbox-exec"), &args)
                .unwrap();
            cmd.env_clear()
                .envs(&env.vars)
                .current_dir(cwd)
                .kill_on_drop(true);
            runtime.apply_env(&mut cmd, env);
            cmd
        }
        #[test]
        fn unknown_or_permissive_native_profiles_fail_closed() {
            for profile in [
                "(version 1)(allow default)",
                "(version 1)(allow default)(allow file-read*)",
                "(version 1)(import \"system.sb\")",
                "(version 2)",
                "(version 1",
            ] {
                assert!(intersect("(version 1)(allow default)", profile).is_err());
            }
            let profile = "(version 1)(allow default)(deny file-write*)(allow file-write* (subpath \"/tmp\"))(deny file-read* (subpath \"/secret\"))(allow network-outbound (literal \"/tmp/launchd\"))";
            let merged =
                intersect("(version 1)(allow default)(deny network-outbound)", profile).unwrap();
            assert!(merged.contains("(deny network-outbound)"));
            assert!(!merged.contains("(allow network-outbound"));
            assert!(merged.contains("(deny file-read* (subpath \"/secret\"))"));
        }
        #[test]
        fn git_metadata_denials_survive_intersection() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            let pin = root.join(".git");
            let protected = root.as_path().join("config");
            std::fs::write(&pin, "gitdir: fixture").unwrap();
            std::fs::write(&protected, "protected").unwrap();
            let native = format!("{}(deny file-write-unlink (literal {}))(deny file-write-create (require-all (literal {}) (vnode-type SYMLINK)))(deny file-write* (regex {}))", native_profile(root.as_path()), quote_path(&pin), quote_path(&pin), serde_json::to_string(&format!("^{}/config$", root.as_path().display())).unwrap());
            let merged = intersect(&native_profile(root.as_path()), &native).unwrap();
            for (command, allowed) in [
                ("printf ok > regular", true),
                ("rm .git", false),
                ("printf bad > config", false),
            ] {
                let output = std::process::Command::new("/usr/bin/sandbox-exec")
                    .args(["-p", &merged, "/bin/sh", "-c", command])
                    .current_dir(root.as_path())
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.success(),
                    allowed,
                    "{command}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert_eq!(std::fs::read_to_string(protected).unwrap(), "protected");
            assert!(pin.is_file());
            let unknown = native.replace("(vnode-type SYMLINK)", "(unknown-filter yes)");
            assert!(intersect(&native_profile(root.as_path()), &unknown).is_err());
        }
        #[test]
        fn write_roots_are_intersected_by_the_kernel() {
            let dir = tempfile::tempdir().unwrap();
            let left = dir.path().join("left");
            let right = dir.path().join("right");
            std::fs::create_dir(&left).unwrap();
            std::fs::create_dir(&right).unwrap();
            let baseline = native_profile(&left);
            let profile = native_profile(dir.path());
            let merged = intersect(&baseline, &profile).unwrap();
            let allowed = std::process::Command::new("/usr/bin/sandbox-exec")
                .args(["-p", &merged, "/usr/bin/touch"])
                .arg(left.join("ok"))
                .output()
                .unwrap();
            assert!(
                allowed.status.success(),
                "{}",
                String::from_utf8_lossy(&allowed.stderr)
            );
            let denied = std::process::Command::new("/usr/bin/sandbox-exec")
                .args(["-p", &merged, "/usr/bin/touch"])
                .arg(right.join("bad"))
                .output()
                .unwrap();
            assert!(!denied.status.success());
            assert!(!right.join("bad").exists());
            let disjoint = intersect(&baseline, &native_profile(&right)).unwrap();
            assert!(!std::process::Command::new("/usr/bin/sandbox-exec")
                .args(["-p", &disjoint, "/usr/bin/touch"])
                .arg(left.join("also-bad"))
                .output()
                .unwrap()
                .status
                .success());
        }
        #[tokio::test]
        async fn broker_preserves_native_read_denials_and_workspace_read_only() {
            let dir = tempfile::tempdir().unwrap();
            let env = fixture(dir.path());
            let cwd = dir.path().join("work");
            std::fs::create_dir(&cwd).unwrap();
            let denied_file = dir.path().join("private.txt");
            std::fs::write(&denied_file, "synthetic-private").unwrap();
            let runtime = Runtime::start(&env, cwd.to_str().unwrap(), None, None, true)
                .await
                .unwrap();
            let native = format!(
                "{}(deny file-read* (subpath {}))",
                native_profile(dir.path()),
                quote_path(&denied_file.canonicalize().unwrap())
            );
            for path in [denied_file, dir.path().join(".reasonix/.env")] {
                let command = format!(
                    "cat {}",
                    super::super::super::adapter::shell_single_quote(&path.to_string_lossy())
                );
                let output = client(&runtime, &env, &cwd, &native, &command)
                    .output()
                    .await
                    .unwrap();
                assert!(!output.status.success());
                assert!(output.stdout.is_empty());
            }
            let output = client(
                &runtime,
                &env,
                &cwd,
                &native,
                "printf changed > forbidden.txt",
            )
            .output()
            .await
            .unwrap();
            assert!(!output.status.success());
            assert!(!cwd.join("forbidden.txt").exists());
            let output = client(&runtime, &env, &cwd, &native, "printf actual-output")
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"actual-output");
            let command = format!(
                "printf modified > {}",
                super::super::super::adapter::shell_single_quote(
                    &runtime.dir.path().join("sandbox-exec").to_string_lossy()
                )
            );
            assert!(!client(&runtime, &env, &cwd, &native, &command)
                .output()
                .await
                .unwrap()
                .status
                .success());
        }
        #[tokio::test]
        async fn configured_bounds_survive_a_forged_client_profile() {
            let dir = tempfile::tempdir().unwrap();
            let env = fixture(dir.path());
            let forbidden = dir.path().join("custom-private");
            std::fs::write(&forbidden, "synthetic-private").unwrap();
            std::fs::write(
                dir.path().join(".reasonix/config.toml"),
                format!(
                    "[sandbox]\nnetwork = false\nforbid_read = [{}]\n",
                    quote_path(&forbidden)
                ),
            )
            .unwrap();
            let runtime = Runtime::start(&env, dir.path().to_str().unwrap(), None, None, false)
                .await
                .unwrap();
            let command = format!(
                "cat {}",
                super::super::super::adapter::shell_single_quote(&forbidden.to_string_lossy())
            );
            assert!(!client(
                &runtime,
                &env,
                dir.path(),
                &native_profile(dir.path()),
                &command
            )
            .output()
            .await
            .unwrap()
            .status
            .success());
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let command = format!(
                "/usr/bin/curl --max-time 1 http://127.0.0.1:{}",
                listener.local_addr().unwrap().port()
            );
            assert!(!client(
                &runtime,
                &env,
                dir.path(),
                &native_profile(dir.path()),
                &command
            )
            .output()
            .await
            .unwrap()
            .status
            .success());
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
        #[tokio::test]
        async fn disconnect_and_turn_cancellation_kill_broker_children() {
            for disconnect in [true, false] {
                let dir = tempfile::tempdir().unwrap();
                let env = fixture(dir.path());
                let mut runtime = Some(
                    Runtime::start(&env, dir.path().to_str().unwrap(), None, None, false)
                        .await
                        .unwrap(),
                );
                let mut child = client(
                    runtime.as_ref().unwrap(),
                    &env,
                    dir.path(),
                    &native_profile(dir.path()),
                    "echo $$ > child.pid; sleep 120",
                )
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while !dir.path().join("child.pid").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
                let pid: i32 = std::fs::read_to_string(dir.path().join("child.pid"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if disconnect {
                    child.kill().await.unwrap();
                } else {
                    runtime.take();
                }
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while unsafe { libc::kill(pid, 0) } == 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await
                .expect("broker command survived cancellation");
                drop(runtime);
                let _ = child.wait().await;
            }
        }
        #[tokio::test]
        async fn forced_proxy_allows_helper_ipc_but_blocks_other_ports() {
            let dir = tempfile::tempdir().unwrap();
            let env = fixture(dir.path());
            let allowed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let forbidden = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            forbidden.set_nonblocking(true).unwrap();
            let runtime = Runtime::start(
                &env,
                dir.path().to_str().unwrap(),
                None,
                Some(allowed.local_addr().unwrap().port()),
                false,
            )
            .await
            .unwrap();
            let native = format!(
                "{}(allow network-outbound (literal \"/tmp/launchd\"))",
                native_profile(dir.path())
            );
            let output = client(&runtime, &env, dir.path(), &native, "printf IPC_OK")
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"IPC_OK");
            let command = format!(
                "/usr/bin/curl --max-time 1 http://127.0.0.1:{}",
                forbidden.local_addr().unwrap().port()
            );
            assert!(!client(&runtime, &env, dir.path(), &native, &command)
                .output()
                .await
                .unwrap()
                .status
                .success());
            assert_eq!(
                forbidden.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
        #[tokio::test]
        async fn broker_probe_runs_inside_outer_sandbox() {
            let cwd = tempfile::tempdir().unwrap();
            let env = crate::env::capture().await;
            let runtime = Runtime::start(&env, cwd.path().to_str().unwrap(), None, None, false)
                .await
                .unwrap();
            let args = vec![
                "-p".into(),
                "(version 1)(allow default)".into(),
                "/usr/bin/true".into(),
            ];
            let mut cmd = runtime
                .command(&runtime.dir.path().join("sandbox-exec"), &args)
                .unwrap();
            cmd.env_clear().envs(&env.vars).current_dir(cwd.path());
            runtime.apply_env(&mut cmd, &env);
            let output = cmd.output().await.unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        #[test]
        #[ignore = "subprocess IPC helper, invoked by broker acceptance tests only"]
        fn helper() {
            let args = std::env::args().collect::<Vec<_>>();
            let code = super::helper(&args).expect("sandbox helper arguments");
            std::process::exit(code);
        }
    }
}
#[cfg(target_os = "macos")]
pub(super) use macos::Runtime;

/// Desktop entry point calls this before starting Tauri. Other platforms leave
/// the CLI's native sandbox path unchanged.
pub fn run_helper() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        macos::helper(&std::env::args().collect::<Vec<_>>())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
