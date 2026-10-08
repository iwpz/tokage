//! Official install commands per provider and platform. Copy-only: Tokage shows these
//! commands but never runs them. Sources are each CLI's own install docs.

use serde::Serialize;

use crate::{env::ShellEnv, providers::ProviderId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallKind {
    Script,
    Powershell,
    Cmd,
    Homebrew,
    Macports,
    Npm,
    Apt,
    Dnf,
    Yum,
    Apk,
    Winget,
    Pipx,
    Uv,
    Scoop,
}

impl InstallKind {
    /// Executable that must be on PATH for the command to work.
    fn tool(self) -> &'static str {
        match self {
            Self::Script => "curl",
            Self::Powershell => "powershell",
            Self::Cmd => "curl",
            Self::Homebrew => "brew",
            Self::Macports => "port",
            Self::Npm => "npm",
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Yum => "yum",
            Self::Apk => "apk",
            Self::Winget => "winget",
            Self::Pipx => "pipx",
            Self::Uv => "uv",
            Self::Scoop => "scoop",
        }
    }

    /// System package managers are only offered when present, so Ubuntu sees apt-get,
    /// Fedora sees dnf, older RHEL/CentOS sees yum.
    fn only_if_available(self) -> bool {
        matches!(
            self,
            Self::Apt | Self::Dnf | Self::Yum | Self::Apk | Self::Macports
        )
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMethod {
    pub kind: InstallKind,
    pub command: String,
    pub recommended: bool,
    /// Whether the tool the command relies on was found on PATH.
    pub available: bool,
    /// Runtime prerequisite, e.g. "Node.js 22+".
    pub requires: Option<&'static str>,
}

/// Which installer tools exist on this machine; computed once per scan.
pub struct Toolbox {
    found: Vec<InstallKind>,
}

const ALL_KINDS: &[InstallKind] = &[
    InstallKind::Script,
    InstallKind::Powershell,
    InstallKind::Cmd,
    InstallKind::Homebrew,
    InstallKind::Macports,
    InstallKind::Npm,
    InstallKind::Apt,
    InstallKind::Dnf,
    InstallKind::Yum,
    InstallKind::Apk,
    InstallKind::Winget,
    InstallKind::Pipx,
    InstallKind::Uv,
    InstallKind::Scoop,
];

impl Toolbox {
    pub fn detect(env: &ShellEnv) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let found = ALL_KINDS
            .iter()
            .copied()
            .filter(|k| which::which_in(k.tool(), env.path(), &cwd).is_ok())
            .collect();
        Self { found }
    }

    fn has(&self, kind: InstallKind) -> bool {
        self.found.contains(&kind)
    }
}

struct Candidate {
    kind: InstallKind,
    command: String,
    recommended: bool,
    requires: Option<&'static str>,
}

fn c(kind: InstallKind, command: impl Into<String>) -> Candidate {
    Candidate {
        kind,
        command: command.into(),
        recommended: false,
        requires: None,
    }
}

impl Candidate {
    fn recommended(mut self) -> Self {
        self.recommended = true;
        self
    }
    fn requires(mut self, r: &'static str) -> Self {
        self.requires = Some(r);
        self
    }
}

const CLAUDE_RPM_REPO: &str = "<<'EOF'
[claude-code]
name=Claude Code
baseurl=https://downloads.claude.ai/claude-code/rpm/stable
enabled=1
gpgcheck=1
gpgkey=https://downloads.claude.ai/keys/claude-code.asc
EOF";

fn candidates(id: ProviderId, os: &str) -> Vec<Candidate> {
    use InstallKind::*;
    let unix = os == "macos" || os == "linux";
    let mut v = Vec::new();

    match id {
        // https://code.claude.com/docs/en/setup
        ProviderId::ClaudeCode => {
            if unix {
                v.push(c(Script, "curl -fsSL https://claude.ai/install.sh | bash").recommended());
            }
            if os == "macos" {
                v.push(c(Homebrew, "brew install --cask claude-code"));
            }
            if os == "linux" {
                v.push(c(
                    Apt,
                    "sudo install -d -m 0755 /etc/apt/keyrings\n\
                     sudo curl -fsSL https://downloads.claude.ai/keys/claude-code.asc -o /etc/apt/keyrings/claude-code.asc\n\
                     echo \"deb [signed-by=/etc/apt/keyrings/claude-code.asc] https://downloads.claude.ai/claude-code/apt/stable stable main\" | sudo tee /etc/apt/sources.list.d/claude-code.list\n\
                     sudo apt-get update && sudo apt-get install claude-code",
                ));
                v.push(c(Dnf, format!("sudo tee /etc/yum.repos.d/claude-code.repo {CLAUDE_RPM_REPO}\nsudo dnf install claude-code")));
                v.push(c(Yum, format!("sudo tee /etc/yum.repos.d/claude-code.repo {CLAUDE_RPM_REPO}\nsudo yum install claude-code")));
                v.push(c(
                    Apk,
                    "wget -O /etc/apk/keys/claude-code.rsa.pub https://downloads.claude.ai/keys/claude-code.rsa.pub\n\
                     echo \"https://downloads.claude.ai/claude-code/apk/stable\" >> /etc/apk/repositories\n\
                     apk add claude-code",
                ));
            }
            if os == "windows" {
                v.push(c(Powershell, "irm https://claude.ai/install.ps1 | iex").recommended());
                v.push(c(Winget, "winget install Anthropic.ClaudeCode"));
                v.push(c(Cmd, "curl -fsSL https://claude.ai/install.cmd -o install.cmd && install.cmd && del install.cmd"));
            }
            v.push(c(Npm, "npm install -g @anthropic-ai/claude-code").requires("Node.js 22+"));
        }
        // https://github.com/openai/codex
        ProviderId::CodexCli => {
            if unix {
                v.push(
                    c(
                        Script,
                        "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
                    )
                    .recommended(),
                );
                v.push(c(Homebrew, "brew install --cask codex"));
            }
            if os == "windows" {
                v.push(c(Powershell, "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"").recommended());
            }
            v.push(c(Npm, "npm install -g @openai/codex"));
        }
        // https://antigravity.google/docs/cli/install
        ProviderId::AntigravityCli => {
            if unix {
                v.push(
                    c(
                        Script,
                        "curl -fsSL https://antigravity.google/cli/install.sh | bash",
                    )
                    .recommended(),
                );
            }
            if os == "windows" {
                v.push(
                    c(
                        Powershell,
                        "irm https://antigravity.google/cli/install.ps1 | iex",
                    )
                    .recommended(),
                );
                v.push(c(
                    Cmd,
                    "curl -fsSL https://antigravity.google/cli/install.cmd -o install.cmd && install.cmd && del install.cmd",
                ));
            }
        }
        // https://aider.chat/docs/install.html — avoids `pip install` into an externally
        // managed Python (PEP 668), which fails on Homebrew / distro Pythons.
        ProviderId::Aider => {
            if unix {
                v.push(c(Script, "curl -LsSf https://aider.chat/install.sh | sh").recommended());
                v.push(c(Homebrew, "brew install aider"));
            }
            if os == "windows" {
                v.push(c(Powershell, "powershell -ExecutionPolicy ByPass -c \"irm https://aider.chat/install.ps1 | iex\"").recommended());
            }
            v.push(c(
                Uv,
                "uv tool install --force --python python3.12 --with pip aider-chat@latest",
            ));
            v.push(c(Pipx, "pipx install aider-chat"));
        }
        // https://cursor.com/docs/cli/overview
        ProviderId::CursorCli => {
            if unix {
                v.push(c(Script, "curl https://cursor.com/install -fsS | bash").recommended());
            }
            if os == "windows" {
                v.push(
                    c(
                        Powershell,
                        "irm 'https://cursor.com/install?win32=true' | iex",
                    )
                    .recommended(),
                );
            }
        }
        // https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli
        ProviderId::CopilotCli => {
            v.push(
                c(Npm, "npm install -g @github/copilot")
                    .recommended()
                    .requires("Node.js 22+"),
            );
            if unix {
                v.push(c(Script, "curl -fsSL https://gh.io/copilot-install | bash"));
                v.push(c(Homebrew, "brew install --cask copilot-cli"));
            }
            if os == "windows" {
                v.push(c(Winget, "winget install GitHub.Copilot"));
            }
        }
        // https://opencode.ai/docs/
        ProviderId::Opencode => {
            if unix {
                v.push(c(Script, "curl -fsSL https://opencode.ai/install | bash").recommended());
                v.push(c(Homebrew, "brew install anomalyco/tap/opencode"));
            }
            if os == "windows" {
                v.push(c(Scoop, "scoop install opencode"));
            }
            v.push(c(Npm, "npm install -g opencode-ai"));
        }
        // https://reasonix.io/ / https://github.com/esengine/reasonix
        ProviderId::Reasonix => {
            v.push(c(Npm, "npm install -g reasonix").recommended());
            if unix {
                v.push(c(Homebrew, "brew install esengine/reasonix/reasonix"));
            }
        }
        // https://docs.x.ai/build/overview
        ProviderId::GrokCli => {
            if unix {
                v.push(c(Script, "curl -fsSL https://x.ai/cli/install.sh | bash").recommended());
            }
            if os == "windows" {
                v.push(c(Powershell, "irm https://x.ai/cli/install.ps1 | iex").recommended());
            }
        }
        // https://kiro.dev/docs/cli/
        ProviderId::KiroCli => {
            if unix {
                v.push(c(Script, "curl -fsSL https://cli.kiro.dev/install | bash").recommended());
            }
            if os == "windows" {
                v.push(c(Powershell, "irm 'https://cli.kiro.dev/install.ps1' | iex").recommended());
            }
        }
        // https://github.com/QwenLM/qwen-code
        ProviderId::QwenCode => {
            if unix {
                v.push(c(Script, "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh | bash").recommended());
                v.push(c(Homebrew, "brew install qwen-code"));
            }
            v.push(c(Npm, "npm install -g @qwen-code/qwen-code").requires("Node.js 22+"));
        }
        // https://www.kimi.com/code/docs/en/
        ProviderId::KimiCode => {
            if unix {
                v.push(
                    c(
                        Script,
                        "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
                    )
                    .recommended(),
                );
                v.push(c(Homebrew, "brew install kimi-code"));
            }
            if os == "windows" {
                v.push(
                    c(
                        Powershell,
                        "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
                    )
                    .recommended(),
                );
            }
            v.push(c(Npm, "npm install -g @moonshot-ai/kimi-code").requires("Node.js 22.19+"));
        }
        // https://docs.qoder.com/cli/installation
        ProviderId::QoderCli => {
            if unix {
                v.push(c(Script, "curl -fsSL https://qoder.com/install | bash").recommended());
            }
            if os == "windows" {
                v.push(c(Powershell, "irm https://qoder.com/install.ps1 | iex").recommended());
            }
            v.push(c(Npm, "npm install -g @qoder-ai/qodercli").requires("Node.js 20+"));
        }
        // https://ampcode.com/manual
        ProviderId::Amp => {
            if unix {
                v.push(c(Script, "curl -fsSL https://ampcode.com/install.sh | bash").recommended());
            }
            v.push(c(Npm, "npm install -g @ampcode/cli"));
        }
        // https://github.com/charmbracelet/crush
        ProviderId::Crush => {
            if unix {
                v.push(c(Homebrew, "brew install charmbracelet/tap/crush"));
            }
            if os == "windows" {
                v.push(c(Winget, "winget install charmbracelet.crush"));
            }
            v.push(c(Npm, "npm install -g @charmland/crush"));
        }
        // https://goose-docs.ai/
        ProviderId::Goose => {
            if unix {
                v.push(c(Script, "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash").recommended());
                v.push(c(Homebrew, "brew install block-goose-cli"));
            }
        }
        // https://docs.cline.bot/cline-cli/overview
        ProviderId::Cline => {
            v.push(
                c(Npm, "npm install -g cline")
                    .recommended()
                    .requires("Node.js 20+"),
            );
        }
        // https://docs.factory.com/droid-cli/quickstart
        ProviderId::Droid => {
            if unix {
                v.push(c(Script, "curl -fsSL https://app.factory.ai/cli | sh").recommended());
                v.push(c(Homebrew, "brew install --cask droid"));
            }
            if os == "windows" {
                v.push(c(Powershell, "irm https://app.factory.ai/cli/windows | iex").recommended());
            }
        }
        // https://docs.augmentcode.com/cli/overview
        ProviderId::Auggie => {
            v.push(
                c(Npm, "npm install -g @augmentcode/auggie")
                    .recommended()
                    .requires("Node.js 22+"),
            );
        }
        // https://dev.meta.ai/docs/muse-code
        ProviderId::MuseCode => {
            if unix {
                v.push(c(Script, "curl -fsSL https://dev.meta.ai/install.sh | sh").recommended());
            }
            if os == "windows" {
                v.push(c(Powershell, "irm https://dev.meta.ai/install.ps1 | iex").recommended());
            }
        }
        // https://docs.trae.cn/cli_what-is-trae-cli (TRAE Enterprise only)
        ProviderId::TraeCli => {
            if unix {
                v.push(
                    c(
                        Script,
                        "sh -c \"$(curl -fsSL https://trae.cn/trae-cli/install_v2.sh)\"",
                    )
                    .recommended(),
                );
            }
        }
    }
    v
}

pub fn methods(id: ProviderId, tools: &Toolbox) -> Vec<InstallMethod> {
    let os = std::env::consts::OS;
    // On Windows, curl.exe ships with the OS and PowerShell is always there.
    let builtin =
        |k: InstallKind| os == "windows" && matches!(k, InstallKind::Powershell | InstallKind::Cmd);
    // dnf and yum share the same repo file; offer yum only where dnf is missing.
    let skip_yum = tools.has(InstallKind::Dnf);

    let mut list: Vec<InstallMethod> = candidates(id, os)
        .into_iter()
        .filter(|cand| !(cand.kind == InstallKind::Yum && skip_yum))
        .filter_map(|cand| {
            let available = builtin(cand.kind) || tools.has(cand.kind);
            if cand.kind.only_if_available() && !available {
                return None;
            }
            Some(InstallMethod {
                kind: cand.kind,
                command: cand.command,
                recommended: cand.recommended,
                available,
                requires: cand.requires,
            })
        })
        .collect();
    // Recommended first, then whatever this machine can run; stable otherwise.
    list.sort_by_key(|m| (!m.recommended, !m.available));
    list
}
