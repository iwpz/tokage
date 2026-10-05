//! Authorization modes (docs/11 §2): what happens to a classified command or edit under
//! `ask_always` / `ask_sensitive` / `full_auto`, and which mode applies when global, project
//! and session settings disagree. No IO: callers load the layers and pass them in.

use serde::{Deserialize, Serialize};

use crate::command_policy::{Level, Verdict};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    AskAlways,
    AskSensitive,
    FullAuto,
}

impl AuthMode {
    pub const DEFAULT: AuthMode = AuthMode::AskSensitive;

    pub fn parse(s: &str) -> Option<AuthMode> {
        Some(match s {
            "ask_always" => AuthMode::AskAlways,
            "ask_sensitive" => AuthMode::AskSensitive,
            "full_auto" => AuthMode::FullAuto,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AuthMode::AskAlways => "ask_always",
            AuthMode::AskSensitive => "ask_sensitive",
            AuthMode::FullAuto => "full_auto",
        }
    }
}

/// Task role of the CLI window (docs/01 §3). Modes apply to `EXECUTOR` / `TESTER` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    Planner,
    Executor,
    Tester,
    Reviewer,
}

impl Role {
    pub const DEFAULT: Role = Role::Executor;

    pub fn is_read_only(self) -> bool {
        matches!(self, Role::Planner | Role::Reviewer)
    }

    pub fn allows_writes(self) -> bool {
        matches!(self, Role::Executor | Role::Tester)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Planner => "PLANNER",
            Role::Executor => "EXECUTOR",
            Role::Tester => "TESTER",
            Role::Reviewer => "REVIEWER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// User's response when approving a command (docs/11 §8, P2.m3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDecision {
    AllowOnce,
    AllowSession,
    AllowProject,
    Deny,
}

impl UserDecision {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "allow_once" => Self::AllowOnce,
            "allow_session" => Self::AllowSession,
            "allow_project" => Self::AllowProject,
            "deny" => Self::Deny,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowSession => "allow_session",
            Self::AllowProject => "allow_project",
            Self::Deny => "deny",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub decision: Decision,
    /// Stable key for the UI: `forbidden`, `read_only_role`, `unresolved`, …
    pub reason: &'static str,
    /// Every write targets a git-tracked file ("can be restored"), when that is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
}

/// The decision for one verdict (from `Policy::classify` or `Policy::classify_edit`) under
/// `mode`, for a window in `role`. `mode` is the effective one ([`Layers::resolve`]).
///
/// | level       | ask_always | ask_sensitive | full_auto |
/// |-------------|------------|---------------|-----------|
/// | read_only   | allow      | allow         | allow     |
/// | project     | ask        | allow         | allow     |
/// | sensitive   | ask        | ask           | allow     |
/// | forbidden   | deny       | deny          | deny      |
///
/// A verdict that is not statically resolved is asked about in every mode, `full_auto`
/// included. Edits inside the workspace and `write_scope` classify as `project`, so
/// `ask_always` asks about every file write.
pub fn decide(mode: AuthMode, role: Role, v: &Verdict) -> Outcome {
    let out = |decision, reason| Outcome {
        decision,
        reason,
        recoverable: v.recoverable,
    };
    if v.level == Level::Forbidden {
        return out(Decision::Deny, "forbidden");
    }
    match role {
        // Planning and review never change anything, whatever the mode.
        Role::Planner | Role::Reviewer => {
            return if v.level == Level::ReadOnly && v.resolved {
                out(Decision::Allow, "read_only")
            } else {
                out(Decision::Deny, "read_only_role")
            };
        }
        // Testers run the project's own commands (tests among them), nothing riskier.
        Role::Tester if v.level == Level::Sensitive => return out(Decision::Deny, "tester_role"),
        _ => {}
    }
    if !v.resolved {
        return out(Decision::Ask, "unresolved");
    }
    match (mode, v.level) {
        (_, Level::ReadOnly) => out(Decision::Allow, "read_only"),
        (AuthMode::AskAlways, _) => out(Decision::Ask, "ask_always"),
        (AuthMode::AskSensitive, Level::Project) => out(Decision::Allow, "project"),
        (AuthMode::AskSensitive, _) => out(Decision::Ask, "sensitive"),
        (AuthMode::FullAuto, _) => out(Decision::Allow, "full_auto"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Project,
    Session,
}

/// The mode set at each scope; a narrower scope overrides a wider one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layers {
    pub global: AuthMode,
    pub project: Option<AuthMode>,
    pub session: Option<AuthMode>,
}

impl Default for Layers {
    fn default() -> Layers {
        Layers {
            global: AuthMode::DEFAULT,
            project: None,
            session: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub mode: AuthMode,
    /// Where `mode` comes from.
    pub scope: Scope,
    /// A narrower scope asked for `full_auto`, which this CLI can't have (no OS sandbox or
    /// the full-auto grant is not active); the next scope down applies instead.
    pub full_auto_unavailable: bool,
}

impl Layers {
    /// The mode in force. `full_auto_allowed` says whether `full_auto` may apply to this CLI
    /// right now (docs/11 §2: OS sandbox, confirmed, not expired); when it may not, that
    /// layer is skipped. The global layer never yields `full_auto`.
    pub fn resolve(&self, full_auto_allowed: bool) -> Resolved {
        let mut skipped = false;
        for (mode, scope) in [(self.session, Scope::Session), (self.project, Scope::Project)] {
            match mode {
                Some(AuthMode::FullAuto) if !full_auto_allowed => skipped = true,
                Some(mode) => {
                    return Resolved {
                        mode,
                        scope,
                        full_auto_unavailable: skipped,
                    }
                }
                None => {}
            }
        }
        let global = if self.global == AuthMode::FullAuto { AuthMode::DEFAULT } else { self.global };
        Resolved {
            mode: global,
            scope: Scope::Global,
            full_auto_unavailable: skipped,
        }
    }
}

#[cfg(test)]
mod tests;
