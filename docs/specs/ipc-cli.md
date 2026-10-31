# Spec: IPC Socket and CLI (`termin-ipc`, `bin/termin`) (D5)

## 1. Transport

- Unix domain stream socket at `~/Library/Application Support/Termin/termin.sock` (path also exported as `TERMIN_SOCKET` to shells). Created mode `0600`, parent dir `0700`.
- On accept: verify peer uid with `getpeereid` == our uid, else close.
- Stale socket on startup: try connect; if refused, unlink and bind. If another instance answers, the new process acts as a client (single-instance behavior: `open -a Termin` twice focuses the running app).
- Served by the IO reactor (non-blocking), requests dispatched to the main thread as `ReactorEvent::Ipc` and executed as `Action`s; replies written back by the reactor.

## 2. Protocol — JSON lines

One JSON object per line, UTF-8, max 1 MiB per line.

Request: `{"id": 1, "method": "split", "params": {"direction": "right", "pane": 12}}`
Response: `{"id": 1, "result": {...}}` or `{"id": 1, "error": {"code": "not_found", "message": "pane 12"}}`
Events (only after `subscribe`): `{"event": "attention", "data": {...}}`

Targets default to the caller's context when omitted: the CLI fills `workspace`/`pane`/`tab` from `TERMIN_*` env vars when present, otherwise "focused in frontmost window".

## 3. Methods (v1)

| Method | Params | Result |
|---|---|---|
| `ping` | — | `{version}` |
| `list_windows` | — | `[{id, workspaces:[id], active}]` |
| `list_workspaces` | `window?` | `[{id, name, window, panes, attention, cwd, git_branch}]` |
| `list_panes` | `workspace?` | `[{id, tabs:[{id, title, cwd, pid, fg_process, mode}], focused}]` |
| `new_workspace` | `window?, name?, cwd?, command?` | `{workspace, pane, tab}` |
| `select_workspace` | `workspace` | `{}` |
| `rename_workspace` | `workspace?, name` | `{}` |
| `close_workspace` | `workspace?` | `{}` |
| `new_tab` | `pane?, cwd?, command?` | `{tab}` |
| `split` | `pane?, direction: right|down, cwd?, command?` | `{pane, tab}` |
| `focus` | `pane? | tab?` | `{}` |
| `close` | `tab? | pane?` | `{}` |
| `send_text` | `tab?, text` | `{}` (raw bytes to PTY) |
| `send_keys` | `tab?, keys: ["ctrl+c","enter"]` | `{}` (encoded via `encode_key`) |
| `read_screen` | `tab?, lines?: n, scrollback?: bool` | `{text}` |
| `notify` | `tab?, title?, body` | `{}` (raises attention like OSC 777) |
| `set_status` | `workspace?, text` (empty clears) | `{}` sidebar status line |
| `run_action` | `name, args?` | `{}` any `Action` by name |
| `subscribe` | `events: ["attention","block_finished","tab_closed"]` | stream |

`command` param: text sent to the new shell after its first prompt (`B` mark) or after 1 s in classic mode — not exec'd directly, so the shell stays.

## 4. CLI

Same binary. `main` decides: if `argv[1]` is a known subcommand → CLI client mode (no AppKit init, tiny startup); otherwise GUI.

```
termin                         # launch GUI (or focus running instance)
termin ls                      # workspaces/panes/tabs tree
termin new [--name N] [--cwd D] [-- cmd...]
termin split right|down [--cwd D] [-- cmd...]
termin tab [--cwd D] [-- cmd...]
termin focus <id>
termin send [--tab ID] <text>         # supports \n escapes with -e
termin keys [--tab ID] ctrl+c enter
termin read [--tab ID] [--lines N]
termin notify [--title T] <body>
termin status <text>
termin action <name> [json-args]
termin rpc '<raw json>'                # escape hatch
termin --version
```

Output: human-readable by default, `--json` for machine output. Exit codes: 0 ok, 1 error response, 2 cannot connect.

`termin-ipc` crate contains: protocol types (serde), a blocking client (std `UnixStream`), and a sans-IO server codec (line splitting, request decode, response encode). The server dispatch lives in `termin-app`.
