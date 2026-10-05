//! Manifest parser for Cargo.toml, package.json, pyproject.toml, go.mod, etc. (P5.m1a, P5.m1b).

use std::collections::HashMap;
use std::path::Path;
use tokage_core::planning::{ManifestSummary, ProjectCommands};

/// Parses supported project manifests in the specified directory.
pub async fn parse_manifests(root: &Path) -> (Vec<ManifestSummary>, ProjectCommands) {
    let mut manifests = Vec::new();
    let mut commands = ProjectCommands::default();

    // 1. Cargo.toml (Rust)
    let cargo_path = root.join("Cargo.toml");
    if cargo_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&cargo_path).await {
            if let Some(summary) = parse_cargo_toml(&content, "Cargo.toml") {
                manifests.push(summary);
            }
            // Rust conventions
            commands.build.push("cargo build".to_string());
            commands.test.push("cargo test".to_string());
            commands.check.push("cargo check".to_string());
            commands.lint.push("cargo clippy".to_string());
        }
    }

    // 2. package.json (Node / TS / Web)
    let pkg_path = root.join("package.json");
    if pkg_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&pkg_path).await {
            let pm = detect_node_package_manager(root).await;
            if let Some(summary) = parse_package_json(&content, "package.json", pm.as_deref()) {
                // Extract scripts from package.json into project commands
                for (script_name, script_cmd) in &summary.scripts {
                    let run_prefix = match pm.as_deref() {
                        Some("pnpm") => "pnpm run",
                        Some("yarn") => "yarn run",
                        Some("bun") => "bun run",
                        _ => "npm run",
                    };
                    let full_cmd = format!("{run_prefix} {script_name}");
                    let name_lower = script_name.to_lowercase();
                    let cmd_lower = script_cmd.to_lowercase();

                    if name_lower.contains("test") || cmd_lower.contains("vitest") || cmd_lower.contains("jest") {
                        if !commands.test.contains(&full_cmd) {
                            commands.test.push(full_cmd.clone());
                        }
                    } else if name_lower.contains("build") || name_lower.contains("bundle") {
                        if !commands.build.contains(&full_cmd) {
                            commands.build.push(full_cmd.clone());
                        }
                    } else if name_lower.contains("lint") || name_lower.contains("eslint") {
                        if !commands.lint.contains(&full_cmd) {
                            commands.lint.push(full_cmd.clone());
                        }
                    } else if name_lower.contains("check") || name_lower.contains("typecheck") || cmd_lower.contains("tsc") {
                        if !commands.check.contains(&full_cmd) {
                            commands.check.push(full_cmd.clone());
                        }
                    }
                }
                manifests.push(summary);
            }
        }
    }

    // 3. pyproject.toml (Python)
    let pyproject_path = root.join("pyproject.toml");
    if pyproject_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&pyproject_path).await {
            if let Some(summary) = parse_pyproject_toml(&content, "pyproject.toml") {
                manifests.push(summary);
            }
            if !commands.test.contains(&"pytest".to_string()) {
                commands.test.push("pytest".to_string());
            }
            if !commands.lint.contains(&"ruff check".to_string()) {
                commands.lint.push("ruff check".to_string());
            }
        }
    }

    // 4. go.mod (Go)
    let gomod_path = root.join("go.mod");
    if gomod_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&gomod_path).await {
            if let Some(summary) = parse_go_mod(&content, "go.mod") {
                manifests.push(summary);
            }
            commands.build.push("go build ./...".to_string());
            commands.test.push("go test ./...".to_string());
            commands.lint.push("golangci-lint run".to_string());
        }
    }

    // Deduplicate commands
    commands.build.dedup();
    commands.test.dedup();
    commands.check.dedup();
    commands.lint.dedup();

    (manifests, commands)
}

async fn detect_node_package_manager(root: &Path) -> Option<String> {
    if root.join("pnpm-lock.yaml").exists() {
        Some("pnpm".to_string())
    } else if root.join("yarn.lock").exists() {
        Some("yarn".to_string())
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        Some("bun".to_string())
    } else if root.join("package-lock.json").exists() {
        Some("npm".to_string())
    } else {
        None
    }
}

pub fn parse_cargo_toml(content: &str, path: &str) -> Option<ManifestSummary> {
    let parsed: toml::Value = toml::from_str(content).ok()?;
    let mut name = None;
    let mut version = None;
    let mut dependencies = Vec::new();
    let mut dev_dependencies = Vec::new();

    if let Some(package) = parsed.get("package").and_then(|v| v.as_table()) {
        name = package.get("name").and_then(|v| v.as_str()).map(ToString::to_string);
        version = package.get("version").and_then(|v| v.as_str()).map(ToString::to_string);
    } else if let Some(workspace) = parsed.get("workspace").and_then(|v| v.as_table()) {
        if let Some(pkg) = workspace.get("package").and_then(|v| v.as_table()) {
            version = pkg.get("version").and_then(|v| v.as_str()).map(ToString::to_string);
        }
    }

    if let Some(deps) = parsed.get("dependencies").and_then(|v| v.as_table()) {
        for (k, _) in deps {
            dependencies.push(k.clone());
        }
    }
    if let Some(deps) = parsed.get("workspace").and_then(|v| v.get("dependencies")).and_then(|v| v.as_table()) {
        for (k, _) in deps {
            if !dependencies.contains(k) {
                dependencies.push(k.clone());
            }
        }
    }

    if let Some(dev_deps) = parsed.get("dev-dependencies").and_then(|v| v.as_table()) {
        for (k, _) in dev_deps {
            dev_dependencies.push(k.clone());
        }
    }

    Some(ManifestSummary {
        manifest_type: "cargo".to_string(),
        path: path.to_string(),
        name,
        version,
        package_manager: Some("cargo".to_string()),
        dependencies,
        dev_dependencies,
        scripts: HashMap::new(),
    })
}

pub fn parse_package_json(content: &str, path: &str, package_manager: Option<&str>) -> Option<ManifestSummary> {
    let val: serde_json::Value = serde_json::from_str(content).ok()?;
    let name = val.get("name").and_then(|v| v.as_str()).map(ToString::to_string);
    let version = val.get("version").and_then(|v| v.as_str()).map(ToString::to_string);

    let mut dependencies = Vec::new();
    if let Some(deps) = val.get("dependencies").and_then(|v| v.as_object()) {
        for k in deps.keys() {
            dependencies.push(k.clone());
        }
    }

    let mut dev_dependencies = Vec::new();
    if let Some(deps) = val.get("devDependencies").and_then(|v| v.as_object()) {
        for k in deps.keys() {
            dev_dependencies.push(k.clone());
        }
    }

    let mut scripts = HashMap::new();
    if let Some(sc) = val.get("scripts").and_then(|v| v.as_object()) {
        for (k, v) in sc {
            if let Some(s) = v.as_str() {
                scripts.insert(k.clone(), s.to_string());
            }
        }
    }

    let pm = val
        .get("packageManager")
        .and_then(|v| v.as_str())
        .map(|s| s.split('@').next().unwrap_or(s).to_string())
        .or_else(|| package_manager.map(ToString::to_string));

    Some(ManifestSummary {
        manifest_type: "npm".to_string(),
        path: path.to_string(),
        name,
        version,
        package_manager: pm,
        dependencies,
        dev_dependencies,
        scripts,
    })
}

pub fn parse_pyproject_toml(content: &str, path: &str) -> Option<ManifestSummary> {
    let parsed: toml::Value = toml::from_str(content).ok()?;
    let mut name = None;
    let mut version = None;
    let mut dependencies = Vec::new();

    if let Some(project) = parsed.get("project").and_then(|v| v.as_table()) {
        name = project.get("name").and_then(|v| v.as_str()).map(ToString::to_string);
        version = project.get("version").and_then(|v| v.as_str()).map(ToString::to_string);
        if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
            for d in deps {
                if let Some(s) = d.as_str() {
                    dependencies.push(s.split_whitespace().next().unwrap_or(s).to_string());
                }
            }
        }
    }

    Some(ManifestSummary {
        manifest_type: "pyproject".to_string(),
        path: path.to_string(),
        name,
        version,
        package_manager: Some("pip/poetry/uv".to_string()),
        dependencies,
        dev_dependencies: Vec::new(),
        scripts: HashMap::new(),
    })
}

pub fn parse_go_mod(content: &str, path: &str) -> Option<ManifestSummary> {
    let mut module_name = None;
    let mut go_version = None;
    let mut dependencies = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("module ") {
            module_name = Some(trimmed.trim_start_matches("module ").trim().to_string());
        } else if trimmed.starts_with("go ") {
            go_version = Some(trimmed.trim_start_matches("go ").trim().to_string());
        } else if !trimmed.starts_with("//") && trimmed.contains('/') && !trimmed.starts_with("require (") {
            let part = trimmed.split_whitespace().next().unwrap_or("");
            if !part.is_empty() && part != "require" {
                dependencies.push(part.to_string());
            }
        }
    }

    Some(ManifestSummary {
        manifest_type: "go".to_string(),
        path: path.to_string(),
        name: module_name,
        version: go_version,
        package_manager: Some("go modules".to_string()),
        dependencies,
        dev_dependencies: Vec::new(),
        scripts: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cargo_toml() {
        let content = r#"
        [package]
        name = "my-project"
        version = "0.2.0"

        [dependencies]
        serde = "1.0"
        tokio = { version = "1", features = ["full"] }

        [dev-dependencies]
        tempfile = "3"
        "#;
        let summary = parse_cargo_toml(content, "Cargo.toml").expect("parsed");
        assert_eq!(summary.name.as_deref(), Some("my-project"));
        assert_eq!(summary.version.as_deref(), Some("0.2.0"));
        assert!(summary.dependencies.contains(&"serde".to_string()));
        assert!(summary.dependencies.contains(&"tokio".to_string()));
        assert!(summary.dev_dependencies.contains(&"tempfile".to_string()));
    }

    #[test]
    fn test_parse_package_json() {
        let content = r#"{
            "name": "frontend",
            "version": "1.0.0",
            "scripts": {
                "dev": "vite",
                "build": "vite build",
                "test": "vitest run",
                "lint": "eslint ."
            },
            "dependencies": {
                "react": "^18.2.0"
            },
            "devDependencies": {
                "typescript": "^5.0.0"
            }
        }"#;
        let summary = parse_package_json(content, "package.json", Some("pnpm")).expect("parsed");
        assert_eq!(summary.name.as_deref(), Some("frontend"));
        assert_eq!(summary.package_manager.as_deref(), Some("pnpm"));
        assert_eq!(summary.scripts.get("build").map(String::as_str), Some("vite build"));
        assert!(summary.dependencies.contains(&"react".to_string()));
    }

    #[test]
    fn test_parse_go_mod() {
        let content = r#"
        module github.com/foo/bar

        go 1.22

        require (
            github.com/gin-gonic/gin v1.9.1
            golang.org/x/crypto v0.14.0
        )
        "#;
        let summary = parse_go_mod(content, "go.mod").expect("parsed");
        assert_eq!(summary.name.as_deref(), Some("github.com/foo/bar"));
        assert_eq!(summary.version.as_deref(), Some("1.22"));
        assert!(summary.dependencies.contains(&"github.com/gin-gonic/gin".to_string()));
    }
}
