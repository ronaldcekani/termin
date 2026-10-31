//! Hand-written CLI argument parser and execution client.
//!
//! See `docs/specs/ipc-cli.md §4`.

use std::path::PathBuf;
use termin_ipc::protocol::*;
use termin_ipc::{resolve_socket_path, Client, IpcError};

/// Exit codes per `docs/specs/ipc-cli.md §4`:
/// 0 ok, 1 error response / parse error, 2 cannot connect.
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_CANNOT_CONNECT: i32 = 2;

/// Parsed CLI invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct CliInvocation {
    pub json: bool,
    pub socket_path: Option<PathBuf>,
    pub command: Subcommand,
}

/// Supported CLI subcommands.
#[derive(Clone, Debug, PartialEq)]
pub enum Subcommand {
    Help,
    Version,
    Ls,
    New {
        name: Option<String>,
        cwd: Option<String>,
        command: Option<String>,
    },
    Split {
        direction: SplitDirection,
        pane: Option<u32>,
        cwd: Option<String>,
        command: Option<String>,
    },
    Tab {
        pane: Option<u32>,
        cwd: Option<String>,
        command: Option<String>,
    },
    Focus {
        id: u32,
    },
    Send {
        tab: Option<u32>,
        text: String,
        unescape: bool,
    },
    Keys {
        tab: Option<u32>,
        keys: Vec<String>,
    },
    Read {
        tab: Option<u32>,
        lines: Option<usize>,
        scrollback: bool,
    },
    Notify {
        tab: Option<u32>,
        title: Option<String>,
        body: String,
    },
    Status {
        workspace: Option<u32>,
        text: String,
    },
    Action {
        name: String,
        args: Option<serde_json::Value>,
    },
    Rpc {
        raw_json: String,
    },
}

/// Parse error details.
#[derive(Clone, Debug, PartialEq)]
pub struct CliParseError(pub String);

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CliParseError {}

/// Read an environment variable as a u32 id.
fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok())
}

/// Unescape standard escape sequences for `termin send -e`.
pub fn unescape_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('0') => out.push('\0'),
                Some('e') => out.push('\x1b'),
                Some('x') => {
                    // Two hex digits
                    let mut hex = String::new();
                    if let Some(&h1) = chars.peek() {
                        if h1.is_ascii_hexdigit() {
                            hex.push(chars.next().unwrap());
                            if let Some(&h2) = chars.peek() {
                                if h2.is_ascii_hexdigit() {
                                    hex.push(chars.next().unwrap());
                                }
                            }
                        }
                    }
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        out.push(byte as char);
                    } else {
                        out.push('\\');
                        out.push('x');
                        out.push_str(&hex);
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Parse command-line arguments without using external parser dependencies.
pub fn parse_args<I, T>(args: I) -> Result<CliInvocation, CliParseError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let mut raw_args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

    // Strip leading program name if present
    if !raw_args.is_empty() {
        let first = &raw_args[0];
        if first == "termin" || first.ends_with("/termin") || first.ends_with("\\termin") {
            raw_args.remove(0);
        }
    }

    let mut json = false;
    let mut socket_path: Option<PathBuf> = None;
    let mut filtered_args = Vec::new();

    let mut i = 0;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--json" => {
                json = true;
                i += 1;
            }
            "--socket" => {
                if i + 1 >= raw_args.len() {
                    return Err(CliParseError("missing value for --socket".into()));
                }
                socket_path = Some(PathBuf::from(&raw_args[i + 1]));
                i += 2;
            }
            "--help" | "-h" if filtered_args.is_empty() => {
                return Ok(CliInvocation {
                    json,
                    socket_path,
                    command: Subcommand::Help,
                });
            }
            "--version" | "-v" if filtered_args.is_empty() => {
                return Ok(CliInvocation {
                    json,
                    socket_path,
                    command: Subcommand::Version,
                });
            }
            _ => {
                filtered_args.push(raw_args[i].clone());
                i += 1;
            }
        }
    }

    if filtered_args.is_empty() {
        return Ok(CliInvocation {
            json,
            socket_path,
            command: Subcommand::Help,
        });
    }

    let subcmd = &filtered_args[0];
    let rest = &filtered_args[1..];

    let command = match subcmd.as_str() {
        "help" => Subcommand::Help,
        "version" => Subcommand::Version,
        "ls" | "list" => Subcommand::Ls,
        "new" => parse_new(rest)?,
        "split" => parse_split(rest)?,
        "tab" => parse_tab(rest)?,
        "focus" => parse_focus(rest)?,
        "send" => parse_send(rest)?,
        "keys" => parse_keys(rest)?,
        "read" => parse_read(rest)?,
        "notify" => parse_notify(rest)?,
        "status" => parse_status(rest)?,
        "action" => parse_action(rest)?,
        "rpc" => parse_rpc(rest)?,
        other => {
            return Err(CliParseError(format!(
                "unknown subcommand '{}'. Run 'termin --help' for usage.",
                other
            )))
        }
    };

    Ok(CliInvocation {
        json,
        socket_path,
        command,
    })
}

fn parse_new(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut name = None;
    let mut cwd = None;
    let mut cmd_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --name".into()));
                }
                name = Some(args[i + 1].clone());
                i += 2;
            }
            "--cwd" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --cwd".into()));
                }
                cwd = Some(args[i + 1].clone());
                i += 2;
            }
            "--" => {
                cmd_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!("unknown flag '{}' for 'new'", other)));
            }
            other => {
                cmd_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    let command = if cmd_parts.is_empty() {
        None
    } else {
        Some(cmd_parts.join(" "))
    };

    Ok(Subcommand::New { name, cwd, command })
}

fn parse_split(args: &[String]) -> Result<Subcommand, CliParseError> {
    if args.is_empty() {
        return Err(CliParseError(
            "split requires direction: 'right' or 'down'".into(),
        ));
    }

    let direction = match args[0].as_str() {
        "right" => SplitDirection::Right,
        "down" => SplitDirection::Down,
        other => {
            return Err(CliParseError(format!(
                "invalid split direction '{}'; expected 'right' or 'down'",
                other
            )))
        }
    };

    let mut pane = None;
    let mut cwd = None;
    let mut cmd_parts = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pane" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --pane".into()));
                }
                pane = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("pane id must be an integer".into())
                })?);
                i += 2;
            }
            "--cwd" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --cwd".into()));
                }
                cwd = Some(args[i + 1].clone());
                i += 2;
            }
            "--" => {
                cmd_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!(
                    "unknown flag '{}' for 'split'",
                    other
                )));
            }
            other => {
                cmd_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    let command = if cmd_parts.is_empty() {
        None
    } else {
        Some(cmd_parts.join(" "))
    };

    let pane = pane.or_else(|| env_u32("TERMIN_PANE"));
    Ok(Subcommand::Split {
        direction,
        pane,
        cwd,
        command,
    })
}

fn parse_tab(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut pane = None;
    let mut cwd = None;
    let mut cmd_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pane" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --pane".into()));
                }
                pane = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("pane id must be an integer".into())
                })?);
                i += 2;
            }
            "--cwd" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --cwd".into()));
                }
                cwd = Some(args[i + 1].clone());
                i += 2;
            }
            "--" => {
                cmd_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!("unknown flag '{}' for 'tab'", other)));
            }
            other => {
                cmd_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    let command = if cmd_parts.is_empty() {
        None
    } else {
        Some(cmd_parts.join(" "))
    };

    let pane = pane.or_else(|| env_u32("TERMIN_PANE"));
    Ok(Subcommand::Tab {
        pane,
        cwd,
        command,
    })
}

fn parse_focus(args: &[String]) -> Result<Subcommand, CliParseError> {
    if args.is_empty() {
        return Err(CliParseError("focus requires an ID argument".into()));
    }

    let id = args[0]
        .parse::<u32>()
        .map_err(|_| CliParseError("focus ID must be an integer".into()))?;

    Ok(Subcommand::Focus { id })
}

fn parse_send(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut tab = None;
    let mut unescape = false;
    let mut text_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tab" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --tab".into()));
                }
                tab = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("tab id must be an integer".into())
                })?);
                i += 2;
            }
            "-e" => {
                unescape = true;
                i += 1;
            }
            "--" => {
                text_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!(
                    "unknown flag '{}' for 'send'",
                    other
                )));
            }
            other => {
                text_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    if text_parts.is_empty() {
        return Err(CliParseError("send requires text to send".into()));
    }

    let raw_text = text_parts.join(" ");
    let text = if unescape {
        unescape_string(&raw_text)
    } else {
        raw_text
    };

    let tab = tab.or_else(|| env_u32("TERMIN_TAB"));
    Ok(Subcommand::Send { tab, text, unescape })
}

fn parse_keys(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut tab = None;
    let mut keys = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tab" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --tab".into()));
                }
                tab = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("tab id must be an integer".into())
                })?);
                i += 2;
            }
            "--" => {
                keys.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!(
                    "unknown flag '{}' for 'keys'",
                    other
                )));
            }
            other => {
                keys.push(other.to_string());
                i += 1;
            }
        }
    }

    if keys.is_empty() {
        return Err(CliParseError("keys requires key sequences to send".into()));
    }

    let tab = tab.or_else(|| env_u32("TERMIN_TAB"));
    Ok(Subcommand::Keys { tab, keys })
}

fn parse_read(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut tab = None;
    let mut lines = None;
    let mut scrollback = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tab" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --tab".into()));
                }
                tab = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("tab id must be an integer".into())
                })?);
                i += 2;
            }
            "--lines" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --lines".into()));
                }
                lines = Some(args[i + 1].parse::<usize>().map_err(|_| {
                    CliParseError("lines must be an integer".into())
                })?);
                i += 2;
            }
            "--scrollback" => {
                scrollback = true;
                i += 1;
            }
            other => {
                return Err(CliParseError(format!(
                    "unknown argument '{}' for 'read'",
                    other
                )));
            }
        }
    }

    let tab = tab.or_else(|| env_u32("TERMIN_TAB"));
    Ok(Subcommand::Read {
        tab,
        lines,
        scrollback,
    })
}

fn parse_notify(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut tab = None;
    let mut title = None;
    let mut body_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tab" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --tab".into()));
                }
                tab = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("tab id must be an integer".into())
                })?);
                i += 2;
            }
            "--title" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --title".into()));
                }
                title = Some(args[i + 1].clone());
                i += 2;
            }
            "--" => {
                body_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!(
                    "unknown flag '{}' for 'notify'",
                    other
                )));
            }
            other => {
                body_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    if body_parts.is_empty() {
        return Err(CliParseError("notify requires a message body".into()));
    }

    let tab = tab.or_else(|| env_u32("TERMIN_TAB"));
    Ok(Subcommand::Notify {
        tab,
        title,
        body: body_parts.join(" "),
    })
}

fn parse_status(args: &[String]) -> Result<Subcommand, CliParseError> {
    let mut workspace = None;
    let mut text_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--workspace" => {
                if i + 1 >= args.len() {
                    return Err(CliParseError("missing value for --workspace".into()));
                }
                workspace = Some(args[i + 1].parse::<u32>().map_err(|_| {
                    CliParseError("workspace id must be an integer".into())
                })?);
                i += 2;
            }
            "--" => {
                text_parts.extend_from_slice(&args[i + 1..]);
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliParseError(format!(
                    "unknown flag '{}' for 'status'",
                    other
                )));
            }
            other => {
                text_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    let workspace = workspace.or_else(|| env_u32("TERMIN_WORKSPACE"));
    Ok(Subcommand::Status {
        workspace,
        text: text_parts.join(" "),
    })
}

fn parse_action(args: &[String]) -> Result<Subcommand, CliParseError> {
    if args.is_empty() {
        return Err(CliParseError("action requires action name".into()));
    }

    let name = args[0].clone();
    let args_val = if args.len() > 1 {
        let json_str = args[1..].join(" ");
        let val: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            CliParseError(format!("action argument is not valid JSON: {}", e))
        })?;
        Some(val)
    } else {
        None
    };

    Ok(Subcommand::Action {
        name,
        args: args_val,
    })
}

fn parse_rpc(args: &[String]) -> Result<Subcommand, CliParseError> {
    if args.is_empty() {
        return Err(CliParseError("rpc requires a raw JSON string".into()));
    }

    let raw_json = args.join(" ");
    // Validate that it is valid JSON
    let _: serde_json::Value = serde_json::from_str(&raw_json)
        .map_err(|e| CliParseError(format!("rpc argument is not valid JSON: {}", e)))?;

    Ok(Subcommand::Rpc { raw_json })
}

/// Print CLI usage guide.
pub fn print_help() {
    println!(
        r#"Termin — native macOS terminal CLI

Usage:
  termin                             Launch GUI (or focus running instance)
  termin ls                          Show workspaces/panes/tabs tree
  termin new [--name N] [--cwd D] [-- cmd...]
                                     Create new workspace
  termin split right|down [--cwd D] [-- cmd...]
                                     Split focused pane
  termin tab [--cwd D] [-- cmd...]   Create new tab
  termin focus <id>                  Focus pane or tab by ID
  termin send [--tab ID] [-e] <text> Send text to tab PTY (-e enables escapes)
  termin keys [--tab ID] <keys...>   Send key combinations (e.g. ctrl+c enter)
  termin read [--tab ID] [--lines N] [--scrollback]
                                     Read screen buffer text
  termin notify [--title T] <body>   Trigger notification attention
  termin status <text>               Set workspace sidebar status line
  termin action <name> [json-args]   Run application action by name
  termin rpc '<raw json>'            Send raw JSON-RPC request
  termin --version                   Print version
  termin --help                      Show this help

Global options:
  --json                             Format output as machine-readable JSON
  --socket <path>                    Override Unix domain socket path
"#
    );
}

/// Print application version.
pub fn print_version() {
    println!("termin {}", env!("CARGO_PKG_VERSION"));
}

/// Run CLI invocation and return process exit code.
pub fn run<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let invocation = match parse_args(args) {
        Ok(inv) => inv,
        Err(err) => {
            eprintln!("error: {}", err);
            eprintln!("Run 'termin --help' for usage.");
            return EXIT_ERROR;
        }
    };

    match &invocation.command {
        Subcommand::Help => {
            print_help();
            return EXIT_OK;
        }
        Subcommand::Version => {
            print_version();
            return EXIT_OK;
        }
        _ => {}
    }

    let socket_path = invocation
        .socket_path
        .unwrap_or_else(resolve_socket_path);

    let mut client = match Client::connect(&socket_path) {
        Ok(c) => c,
        Err(err) => {
            if invocation.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "error": {
                            "code": "cannot_connect",
                            "message": err.to_string(),
                            "socket": socket_path.to_string_lossy()
                        }
                    })
                );
            } else {
                eprintln!(
                    "error: cannot connect to Termin daemon at '{}': {}",
                    socket_path.display(),
                    err
                );
            }
            return EXIT_CANNOT_CONNECT;
        }
    };

    let result = execute_command(&mut client, &invocation.command);
    match result {
        Ok(val) => {
            if invocation.json {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
            } else {
                format_human_output(&invocation.command, &val);
            }
            EXIT_OK
        }
        Err(IpcError::Rpc(err)) => {
            if invocation.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "error": {
                            "code": err.code,
                            "message": err.message
                        }
                    })
                );
            } else {
                eprintln!("error [{}]: {}", err.code, err.message);
            }
            EXIT_ERROR
        }
        Err(other) => {
            if invocation.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "error": {
                            "code": "ipc_error",
                            "message": other.to_string()
                        }
                    })
                );
            } else {
                eprintln!("IPC error: {}", other);
            }
            EXIT_ERROR
        }
    }
}

fn execute_command(client: &mut Client, cmd: &Subcommand) -> Result<serde_json::Value, IpcError> {
    match cmd {
        Subcommand::Help | Subcommand::Version => Ok(serde_json::Value::Null),
        Subcommand::Ls => {
            let workspaces = client.call(METHOD_LIST_WORKSPACES, serde_json::Value::Null)?;
            let panes = client.call(METHOD_LIST_PANES, serde_json::Value::Null)?;
            let windows = client.call(METHOD_LIST_WINDOWS, serde_json::Value::Null)?;
            Ok(serde_json::json!({
                "windows": windows,
                "workspaces": workspaces,
                "panes": panes,
            }))
        }
        Subcommand::New { name, cwd, command } => {
            let window = env_u32("TERMIN_WINDOW");
            let params = NewWorkspaceParams {
                window,
                name: name.clone(),
                cwd: cwd.clone(),
                command: command.clone(),
            };
            client.call(METHOD_NEW_WORKSPACE, serde_json::to_value(params)?)
        }
        Subcommand::Split {
            direction,
            pane,
            cwd,
            command,
        } => {
            let params = SplitParams {
                pane: *pane,
                direction: *direction,
                cwd: cwd.clone(),
                command: command.clone(),
            };
            client.call(METHOD_SPLIT, serde_json::to_value(params)?)
        }
        Subcommand::Tab { pane, cwd, command } => {
            let params = NewTabParams {
                pane: *pane,
                cwd: cwd.clone(),
                command: command.clone(),
            };
            client.call(METHOD_NEW_TAB, serde_json::to_value(params)?)
        }
        Subcommand::Focus { id } => {
            let params = serde_json::json!({ "id": id, "pane": id });
            client.call(METHOD_FOCUS, params)
        }
        Subcommand::Send { tab, text, .. } => {
            let params = SendTextParams {
                tab: *tab,
                text: text.clone(),
            };
            client.call(METHOD_SEND_TEXT, serde_json::to_value(params)?)
        }
        Subcommand::Keys { tab, keys } => {
            let params = SendKeysParams {
                tab: *tab,
                keys: keys.clone(),
            };
            client.call(METHOD_SEND_KEYS, serde_json::to_value(params)?)
        }
        Subcommand::Read {
            tab,
            lines,
            scrollback,
        } => {
            let params = ReadScreenParams {
                tab: *tab,
                lines: *lines,
                scrollback: Some(*scrollback),
            };
            client.call(METHOD_READ_SCREEN, serde_json::to_value(params)?)
        }
        Subcommand::Notify { tab, title, body } => {
            let params = NotifyParams {
                tab: *tab,
                title: title.clone(),
                body: body.clone(),
            };
            client.call(METHOD_NOTIFY, serde_json::to_value(params)?)
        }
        Subcommand::Status { workspace, text } => {
            let params = SetStatusParams {
                workspace: *workspace,
                text: text.clone(),
            };
            client.call(METHOD_SET_STATUS, serde_json::to_value(params)?)
        }
        Subcommand::Action { name, args } => {
            let params = RunActionParams {
                name: name.clone(),
                args: args.clone(),
            };
            client.call(METHOD_RUN_ACTION, serde_json::to_value(params)?)
        }
        Subcommand::Rpc { raw_json } => {
            let parsed: serde_json::Value = serde_json::from_str(raw_json)?;
            if let Some(obj) = parsed.as_object() {
                if let Some(method) = obj.get("method").and_then(|m| m.as_str()) {
                    let params = obj
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return client.call(method, params);
                }
            }
            Err(IpcError::Protocol("raw JSON must contain a 'method' field".into()))
        }
    }
}

fn format_human_output(cmd: &Subcommand, val: &serde_json::Value) {
    match cmd {
        Subcommand::Ls => {
            if let Some(workspaces) = val.get("workspaces").and_then(|w| w.as_array()) {
                for ws in workspaces {
                    let id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
                    println!("Workspace {}: {}", id, name);
                }
            } else {
                println!("{}", val);
            }
        }
        Subcommand::New { .. } => {
            let ws = val.get("workspace").and_then(|v| v.as_u64());
            let pane = val.get("pane").and_then(|v| v.as_u64());
            let tab = val.get("tab").and_then(|v| v.as_u64());
            match (ws, pane, tab) {
                (Some(w), Some(p), Some(t)) => {
                    println!("Created workspace {} (pane {}, tab {})", w, p, t);
                }
                _ => println!("{}", val),
            }
        }
        Subcommand::Split { .. } => {
            let pane = val.get("pane").and_then(|v| v.as_u64());
            let tab = val.get("tab").and_then(|v| v.as_u64());
            match (pane, tab) {
                (Some(p), Some(t)) => println!("Created pane {} (tab {})", p, t),
                _ => println!("{}", val),
            }
        }
        Subcommand::Tab { .. } => {
            let tab = val.get("tab").and_then(|v| v.as_u64());
            match tab {
                Some(t) => println!("Created tab {}", t),
                _ => println!("{}", val),
            }
        }
        Subcommand::Focus { id } => {
            println!("Focused {}", id);
        }
        Subcommand::Send { .. } | Subcommand::Keys { .. } => {
            // Raw send is silent on success per UNIX convention
        }
        Subcommand::Read { .. } => {
            if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
                print!("{}", text);
            } else {
                println!("{}", val);
            }
        }
        Subcommand::Notify { .. } => {
            println!("Notification sent");
        }
        Subcommand::Status { .. } => {
            println!("Status updated");
        }
        Subcommand::Action { name, .. } => {
            println!("Action '{}' executed", name);
        }
        Subcommand::Rpc { .. } => {
            println!("{}", serde_json::to_string_pretty(val).unwrap_or_default());
        }
        Subcommand::Help | Subcommand::Version => {}
    }
}

#[cfg(test)]
#[allow(clippy::needless_borrows_for_generic_args)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_and_version() {
        let inv = parse_args(&["termin", "--help"]).unwrap();
        assert_eq!(inv.command, Subcommand::Help);

        let inv = parse_args(&["termin", "-v"]).unwrap();
        assert_eq!(inv.command, Subcommand::Version);

        let inv = parse_args(&["termin", "version"]).unwrap();
        assert_eq!(inv.command, Subcommand::Version);
    }

    #[test]
    fn test_parse_ls() {
        let inv = parse_args(&["termin", "ls"]).unwrap();
        assert_eq!(inv.command, Subcommand::Ls);
        assert!(!inv.json);

        let inv = parse_args(&["termin", "--json", "ls"]).unwrap();
        assert_eq!(inv.command, Subcommand::Ls);
        assert!(inv.json);

        let inv = parse_args(&["termin", "ls", "--json"]).unwrap();
        assert_eq!(inv.command, Subcommand::Ls);
        assert!(inv.json);
    }

    #[test]
    fn test_parse_new() {
        let inv = parse_args(&["termin", "new", "--name", "my-ws", "--cwd", "/tmp"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::New {
                name: Some("my-ws".into()),
                cwd: Some("/tmp".into()),
                command: None,
            }
        );

        let inv = parse_args(&["termin", "new", "--", "cargo", "build"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::New {
                name: None,
                cwd: None,
                command: Some("cargo build".into()),
            }
        );
    }

    #[test]
    fn test_parse_split() {
        let inv = parse_args(&["termin", "split", "right", "--cwd", "/home"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Split {
                direction: SplitDirection::Right,
                pane: None,
                cwd: Some("/home".into()),
                command: None,
            }
        );

        let inv = parse_args(&["termin", "split", "down", "--pane", "42"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Split {
                direction: SplitDirection::Down,
                pane: Some(42),
                cwd: None,
                command: None,
            }
        );
    }

    #[test]
    fn test_parse_tab() {
        let inv = parse_args(&["termin", "tab", "--pane", "5", "--", "htop"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Tab {
                pane: Some(5),
                cwd: None,
                command: Some("htop".into()),
            }
        );
    }

    #[test]
    fn test_parse_focus() {
        let inv = parse_args(&["termin", "focus", "123"]).unwrap();
        assert_eq!(inv.command, Subcommand::Focus { id: 123 });

        assert!(parse_args(&["termin", "focus"]).is_err());
        assert!(parse_args(&["termin", "focus", "abc"]).is_err());
    }

    #[test]
    fn test_parse_send_and_unescape() {
        let inv = parse_args(&["termin", "send", "--tab", "3", "-e", r"hello\nworld"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Send {
                tab: Some(3),
                text: "hello\nworld".into(),
                unescape: true,
            }
        );

        let unescaped = unescape_string(r"foo\tbar\x41\n");
        assert_eq!(unescaped, "foo\tbarA\n");
    }

    #[test]
    fn test_parse_keys() {
        let inv = parse_args(&["termin", "keys", "--tab", "1", "ctrl+c", "enter"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Keys {
                tab: Some(1),
                keys: vec!["ctrl+c".into(), "enter".into()],
            }
        );
    }

    #[test]
    fn test_parse_read() {
        let inv = parse_args(&["termin", "read", "--tab", "2", "--lines", "10", "--scrollback"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Read {
                tab: Some(2),
                lines: Some(10),
                scrollback: true,
            }
        );
    }

    #[test]
    fn test_parse_notify() {
        let inv = parse_args(&["termin", "notify", "--title", "Build", "Complete!"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Notify {
                tab: None,
                title: Some("Build".into()),
                body: "Complete!".into(),
            }
        );
    }

    #[test]
    fn test_parse_status() {
        let inv = parse_args(&["termin", "status", "--workspace", "2", "compiling..."]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Status {
                workspace: Some(2),
                text: "compiling...".into(),
            }
        );
    }

    #[test]
    fn test_parse_action() {
        let inv = parse_args(&["termin", "action", "new_window"]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Action {
                name: "new_window".into(),
                args: None,
            }
        );

        let inv = parse_args(&["termin", "action", "select_tab", r#"{"index": 2}"#]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Action {
                name: "select_tab".into(),
                args: Some(serde_json::json!({"index": 2})),
            }
        );
    }

    #[test]
    fn test_parse_rpc() {
        let inv = parse_args(&["termin", "rpc", r#"{"method": "ping"}"#]).unwrap();
        assert_eq!(
            inv.command,
            Subcommand::Rpc {
                raw_json: r#"{"method": "ping"}"#.into(),
            }
        );
    }

    #[test]
    fn test_socket_override() {
        let inv = parse_args(&["termin", "--socket", "/tmp/custom.sock", "ls"]).unwrap();
        assert_eq!(inv.socket_path, Some(PathBuf::from("/tmp/custom.sock")));
        assert_eq!(inv.command, Subcommand::Ls);
    }

    #[test]
    fn test_cli_run_builtins_and_errors() {
        assert_eq!(run(&["termin", "--version"]), EXIT_OK);
        assert_eq!(run(&["termin", "--help"]), EXIT_OK);
        assert_eq!(run(&["termin", "invalid_subcmd"]), EXIT_ERROR);
        assert_eq!(
            run(&["termin", "--socket", "/tmp/termin-fake-test-nonexistent.sock", "ls"]),
            EXIT_CANNOT_CONNECT
        );
    }
}
