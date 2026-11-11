//! Installs Hopper's MCP server into the user config of the agents Hopper supports.

use std::{path::PathBuf, process::Stdio, time::Duration};

use hopper_core::service::{
    HopperError,
    dto::{AgentConnection, McpAgentSetup, McpSetup, McpStatus},
};
use serde_json::{Value, json};

/// The agents Hopper can install its MCP server into. Adding one means adding a variant here and
/// to [`Agent::ALL`]; the id is what the panel sends back to `mcp_connect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    OpenCode,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::ClaudeCode, Agent::OpenCode];

    pub fn id(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude-code",
            Agent::OpenCode => "opencode",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub fn from_id(id: &str) -> Option<Agent> {
        Agent::ALL.into_iter().find(|agent| agent.id() == id)
    }

    /// The CLI binary names to try. npm installs `.cmd` shims, which CreateProcess does not find
    /// under the bare name.
    fn programs(self) -> &'static [&'static str] {
        match self {
            Agent::ClaudeCode => &["claude", "claude.cmd"],
            Agent::OpenCode => &["opencode", "opencode.cmd"],
        }
    }

    /// The agent was set up on this machine, by its config file or by its CLI.
    pub fn detected(self) -> bool {
        self.config_path().is_some_and(|path| path.is_file())
            || self.programs().iter().any(|program| on_path(program))
    }

    /// The user-scoped config file the agent reads for every project.
    fn config_path(self) -> Option<PathBuf> {
        match self {
            // Claude Code: `$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`.
            Agent::ClaudeCode => std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .or_else(home)
                .map(|dir| dir.join(".claude.json")),
            // OpenCode: `$XDG_CONFIG_HOME/opencode/opencode.json(c)`, else `~/.config/opencode`.
            Agent::OpenCode => {
                let dir = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| home().map(|dir| dir.join(".config")))?
                    .join("opencode");
                ["opencode.json", "opencode.jsonc"]
                    .map(|name| dir.join(name))
                    .into_iter()
                    .find(|path| path.is_file())
                    .or(Some(dir.join("opencode.json")))
            }
        }
    }

    /// Compares the `hopper` entry in the agent's user config with this server's URL and token.
    fn connection(self, url: &str, token: &str) -> AgentConnection {
        let Some(config) = self
            .config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| parse(&text))
        else {
            return AgentConnection::None;
        };
        let entry = match self {
            Agent::ClaudeCode => config.pointer("/mcpServers/hopper"),
            Agent::OpenCode => config
                .pointer("/mcp/servers/hopper")
                .or_else(|| config.pointer("/mcp/hopper")),
        };
        entry_connection(entry, url, token)
    }

    /// The command that installs the entry, as the user would type it (values with spaces quoted).
    fn command(self, url: &str, token: &str) -> String {
        let args = match self {
            Agent::ClaudeCode => claude_args(url, token),
            Agent::OpenCode => opencode_args(url, token),
        };
        let quoted: Vec<String> = args
            .iter()
            .map(|arg| {
                if arg.contains(' ') {
                    format!("\"{arg}\"")
                } else {
                    arg.clone()
                }
            })
            .collect();
        format!("{} {}", self.programs()[0], quoted.join(" "))
    }

    fn json(self, url: &str, token: &str) -> String {
        let entry = json!({"type": transport(self), "url": url, "headers": {"Authorization": format!("Bearer {token}")}});
        let config = match self {
            Agent::ClaudeCode => json!({"mcpServers": {"hopper": entry}}),
            Agent::OpenCode => json!({"mcp": {"servers": {"hopper": entry}}}),
        };
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }
}

fn transport(agent: Agent) -> &'static str {
    match agent {
        Agent::ClaudeCode => "http",
        Agent::OpenCode => "remote",
    }
}

fn entry_connection(entry: Option<&Value>, url: &str, token: &str) -> AgentConnection {
    let Some(entry) = entry else {
        return AgentConnection::None;
    };
    let same_url = entry["url"].as_str() == Some(url);
    let same_token = entry["headers"]["Authorization"].as_str() == Some(&format!("Bearer {token}"));
    if same_url && same_token {
        AgentConnection::Connected
    } else {
        AgentConnection::Stale
    }
}

/// `McpSetup` for the panel: one entry per supported agent, each with its state and copy-paste help.
pub fn setup(status: &McpStatus, token: &str) -> Result<McpSetup, HopperError> {
    let url = status.url.clone().ok_or_else(|| {
        HopperError::new("mcp_unavailable", "the MCP server is not running")
            .hint(status.error.clone().unwrap_or_default())
    })?;
    Ok(McpSetup {
        agents: Agent::ALL
            .into_iter()
            .map(|agent| McpAgentSetup {
                id: agent.id().to_owned(),
                name: agent.name().to_owned(),
                detected: agent.detected(),
                connection: agent.connection(&url, token),
                command: agent.command(&url, token),
                json_config: agent.json(&url, token),
            })
            .collect(),
    })
}

/// Installs the `hopper` entry with the agent's own CLI, so a changed port or token is picked up.
/// Everything runs with an argument vector (no shell) and user scope, so every project sees Hopper.
pub async fn connect(status: &McpStatus, token: &str, id: &str) -> Result<String, HopperError> {
    let agent = Agent::from_id(id).ok_or_else(|| {
        HopperError::new(
            "agent_unknown",
            format!("`{id}` is not an agent Hopper knows"),
        )
    })?;
    let url = status.url.clone().ok_or_else(|| {
        HopperError::new("mcp_unavailable", "the MCP server is not running")
            .hint(status.error.clone().unwrap_or_default())
    })?;
    let missing = || {
        HopperError::new(
            "agent_missing",
            format!("the `{}` command was not found", agent.programs()[0]),
        )
        .hint(format!(
            "install {}, or copy the {} command below into its config",
            agent.name(),
            agent.name()
        ))
    };
    let work = async {
        if agent == Agent::ClaudeCode {
            // Also clears the project-scoped entries older versions wrote, which would shadow the
            // user-scoped one.
            for scope in ["user", "local"] {
                let remove: Vec<String> = ["mcp", "remove", "--scope", scope, "hopper"]
                    .map(String::from)
                    .to_vec();
                match run(agent, &remove).await {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
                    _ => {}
                }
            }
        }
        let args = match agent {
            Agent::ClaudeCode => claude_args(&url, token),
            Agent::OpenCode => opencode_args(&url, token),
        };
        let out = run(agent, &args).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing()
            } else {
                HopperError::new("agent_failed", e.to_string())
            }
        })?;
        let shown = text(&out);
        if out.status.success() {
            Ok(shown)
        } else {
            Err(HopperError::new("agent_failed", shown))
        }
    };
    tokio::time::timeout(Duration::from_secs(60), work)
        .await
        .unwrap_or_else(|_| {
            Err(HopperError::new(
                "timeout",
                format!("the {} command took too long", agent.name()),
            ))
        })
}

pub fn claude_args(url: &str, token: &str) -> Vec<String> {
    [
        "mcp",
        "add",
        "--transport",
        "http",
        "--scope",
        "user",
        "hopper",
        url,
        "--header",
        &format!("Authorization: Bearer {token}"),
    ]
    .map(String::from)
    .to_vec()
}

pub fn opencode_args(url: &str, token: &str) -> Vec<String> {
    [
        "mcp",
        "add",
        "hopper",
        "--global",
        "--url",
        url,
        "--header",
        &format!("Authorization=Bearer {token}"),
    ]
    .map(String::from)
    .to_vec()
}

async fn run(agent: Agent, args: &[String]) -> Result<std::process::Output, std::io::Error> {
    let mut last = None;
    for program in agent.programs() {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        match cmd.spawn() {
            Ok(child) => return child.wait_with_output().await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::NotFound.into()))
}

fn text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_owned()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// Parses a config file, tolerating the comments and trailing commas JSONC allows. OpenCode's
/// config may be `opencode.jsonc`, and both agents let users keep notes in it.
fn parse(text: &str) -> Option<Value> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&strip_jsonc(text)).ok())
}

fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for comment in chars.by_ref() {
                    if comment == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for comment in chars.by_ref() {
                    if previous == '*' && comment == '/' {
                        break;
                    }
                    previous = comment;
                }
            }
            _ => out.push(c),
        }
    }
    strip_trailing_commas(&out)
}

/// Removes a comma that directly precedes `}` or `]`, outside strings.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_has_an_id_and_a_name() {
        for agent in Agent::ALL {
            assert_eq!(Agent::from_id(agent.id()), Some(agent));
            assert!(!agent.name().is_empty());
            assert!(!agent.command("http://x/mcp", "t").is_empty());
            assert!(agent.json("http://x/mcp", "t").contains("hopper"));
        }
        assert_eq!(Agent::from_id("vim"), None);
    }

    #[test]
    fn opencode_uses_its_global_cli_shape() {
        let args = opencode_args("http://127.0.0.1:1/mcp", "t");
        assert_eq!(args[..3], ["mcp", "add", "hopper"]);
        assert!(args.windows(2).any(|w| w == ["--global", "--url"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["--header", "Authorization=Bearer t"])
        );
        let json: Value = serde_json::from_str(&Agent::OpenCode.json("http://x/mcp", "t")).unwrap();
        assert_eq!(json["mcp"]["servers"]["hopper"]["type"], "remote");
        assert_eq!(
            json["mcp"]["servers"]["hopper"]["headers"]["Authorization"],
            "Bearer t"
        );
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_are_tolerated() {
        let text = r#"{
            // a note
            "mcp": {
                "servers": {
                    "hopper": {"url": "http://x/mcp", /* inline */ "headers": {"Authorization": "Bearer t"}},
                },
            },
        }"#;
        let config = parse(text).expect("jsonc parses");
        assert_eq!(
            config["mcp"]["servers"]["hopper"]["headers"]["Authorization"],
            "Bearer t"
        );
        assert_eq!(parse("not json"), None);
        // A comment marker inside a string is not a comment.
        let kept = parse(r#"{"note": "http://x/*y*/", "a": 1}"#).unwrap();
        assert_eq!(kept["note"], "http://x/*y*/");
    }

    #[test]
    fn connection_compares_url_and_token_for_both_shapes() {
        let url = "http://127.0.0.1:1/mcp";
        let entry = |source: &str, pointer: &str| {
            let config: Value = parse(source).unwrap();
            entry_connection(config.pointer(pointer), url, "t")
        };
        for (source, pointer) in [
            (
                r#"{"mcpServers":{"hopper":{"url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer t"}}}}"#,
                "/mcpServers/hopper",
            ),
            (
                r#"{"mcp":{"servers":{"hopper":{"url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer t"}}}}}"#,
                "/mcp/servers/hopper",
            ),
            (
                r#"{"mcp":{"hopper":{"url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer t"}}}}"#,
                "/mcp/hopper",
            ),
        ] {
            assert_eq!(entry(source, pointer), AgentConnection::Connected);
        }
        assert_eq!(
            entry(
                r#"{"mcp":{"servers":{"hopper":{"url":"http://old/mcp","headers":{"Authorization":"Bearer t"}}}}}"#,
                "/mcp/servers/hopper"
            ),
            AgentConnection::Stale
        );
        assert_eq!(
            entry(
                r#"{"mcp":{"servers":{"hopper":{"url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer old"}}}}}"#,
                "/mcp/servers/hopper"
            ),
            AgentConnection::Stale
        );
        assert_eq!(
            entry(r#"{"mcp":{"servers":{}}}"#, "/mcp/servers/hopper"),
            AgentConnection::None
        );
    }

    #[test]
    fn setup_lists_every_agent_and_needs_a_running_server() {
        let status = McpStatus {
            running: true,
            port: Some(38517),
            url: Some("http://127.0.0.1:38517/mcp".into()),
            error: None,
        };
        let found = setup(&status, "tok").unwrap();
        assert_eq!(found.agents.len(), Agent::ALL.len());
        let ids: Vec<&str> = found.agents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["claude-code", "opencode"]);
        for agent in &found.agents {
            assert!(agent.command.contains("http://127.0.0.1:38517/mcp"));
            assert!(agent.json_config.contains("http://127.0.0.1:38517/mcp"));
        }
        assert!(setup(&McpStatus::default(), "tok").is_err());
    }
}
