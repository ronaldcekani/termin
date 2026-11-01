//! Main-thread IPC request dispatch against the app model.
//!
//! Socket framing and reply delivery belong to the reactor integration. This module is a pure
//! request-to-model boundary so it can be exercised without AppKit or a live socket.

use std::time::Instant;

use serde_json::{json, Value};
use termin_core::{PaneId, TabId, WindowId, WorkspaceId};
use termin_ipc::{Request, Response};

use crate::model::{AppModel, AttentionKind, Dir};

const METHODS: &[&str] = &[
    "ping",
    "list_windows",
    "list_workspaces",
    "list_panes",
    "new_workspace",
    "select_workspace",
    "rename_workspace",
    "close_workspace",
    "new_tab",
    "split",
    "focus",
    "close",
    "send_text",
    "send_keys",
    "read_screen",
    "notify",
    "set_status",
    "run_action",
    "subscribe",
];

fn err(id: u64, code: &str, msg: impl Into<String>) -> Response {
    Response::error(id, code, msg)
}
fn invalid(id: u64, msg: impl Into<String>) -> Response {
    err(id, "invalid_params", msg)
}
fn missing(id: u64, what: &str, n: u32) -> Response {
    err(id, "not_found", format!("{what} {n}"))
}
fn u32p(p: &Value, key: &str) -> Option<u32> {
    p.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}
fn optional_u32(p: &Value, key: &str, request_id: u64) -> Result<Option<u32>, Response> {
    if p.get(key).is_none() {
        return Ok(None);
    }
    u32p(p, key)
        .map(Some)
        .ok_or_else(|| invalid(request_id, format!("{key} must be an unsigned 32-bit id")))
}
fn strp<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
/// Execute one request on the app's owning (main) thread. Unsupported runtime-dependent methods
/// return explicit errors instead of advertising behavior that is not wired to the reactor/UI.
pub fn dispatch_request(model: &mut AppModel, request: &Request) -> Response {
    let id = request.id;
    let p = &request.params;
    if !p.is_object() {
        return invalid(id, "params must be an object");
    }
    if !METHODS.contains(&request.method.as_str()) {
        return err(id, "method_not_found", "unsupported IPC method");
    }
    match request.method.as_str() {
        "ping" => Response::ok(id, json!({"version": env!("CARGO_PKG_VERSION")})),
        "list_windows" => Response::ok(id, Value::Array(model.windows.iter().map(|(wid,w)| json!({
            "id":wid.0, "workspaces":w.workspaces.iter().map(|x|x.0).collect::<Vec<_>>(),
            "active":model.focused_window()==Some(wid)
        })).collect())),
        "list_workspaces" => {
            let filter = u32p(p,"window");
            if p.get("window").is_some() && filter.is_none() { return invalid(id,"window must be an unsigned 32-bit id"); }
            if let Some(w) = filter { if !model.windows.contains(WindowId(w)) { return missing(id,"window",w); } }
            let values = model.workspaces.iter().filter(|(_,ws)| filter.map_or(true,|w|ws.window==WindowId(w))).map(|(wid,ws)| {
                let panes=ws.tree.leaves();
                let cwd=panes.iter().find_map(|pid| model.panes.get(*pid)).and_then(|pane|model.tabs.get(pane.active_tab)).and_then(|tab|tab.cwd.clone());
                json!({"id":wid.0,"name":ws.name.as_deref().unwrap_or(""),"window":ws.window.0,"panes":panes.iter().map(|x|x.0).collect::<Vec<_>>(),"attention":panes.iter().filter_map(|pid|model.panes.get(*pid)).flat_map(|pane|pane.tabs.iter()).filter_map(|tid|model.tabs.get(*tid)).any(|t|t.attention.unread),"cwd":cwd,"git_branch":ws.meta.git_branch})
            }).collect();
            Response::ok(id,Value::Array(values))
        }
        "list_panes" => {
            let wsid = u32p(p,"workspace");
            if p.get("workspace").is_some() && wsid.is_none() { return invalid(id,"workspace must be an unsigned 32-bit id"); }
            if let Some(w) = wsid { if !model.workspaces.contains(WorkspaceId(w)) { return missing(id,"workspace",w); } }
            let values = model.panes.iter().filter(|(_,pane)|wsid.map_or(true,|w|pane.workspace==WorkspaceId(w))).map(|(pid,pane)| {
                let focused=model.workspaces.get(pane.workspace).is_some_and(|ws|ws.focused_pane==pid && model.windows.get(ws.window).is_some_and(|w|w.active_workspace==ws.id));
                let tabs=pane.tabs.iter().filter_map(|tid|model.tabs.get(*tid).map(|t| {
                    let pid=t.session.as_ref().map(|s|s.pid().0 as u32);
                    // The PTY API exposes a foreground PID but no process-name resolver here.
                    let fg: Option<String> = None;
                    json!({"id":t.id.0,"title":t.title.display(),"cwd":t.cwd,"pid":pid,"fg_process":fg,"mode":t.mode.as_str()})
                })).collect::<Vec<_>>();
                json!({"id":pid.0,"tabs":tabs,"focused":focused})
            }).collect();
            Response::ok(id,Value::Array(values))
        }
        "new_workspace" => {
            if p.get("command").is_some() { return err(id,"unsupported","command-after-prompt scheduling is not connected to the runtime"); }
            let explicit_window = match optional_u32(p,"window",id) { Ok(v)=>v, Err(e)=>return e };
            let win = explicit_window.or_else(||model.focused_window().map(|x|x.0));
            let Some(w)=win else {return err(id,"not_found","no window is available");};
            if !model.windows.contains(WindowId(w)) {return missing(id,"window",w);}
            if p.get("name").is_some_and(|v|!v.is_string()) || p.get("cwd").is_some_and(|v|!v.is_string()) {return invalid(id,"name and cwd must be strings");}
            let ws=model.new_workspace(WindowId(w),strp(p,"cwd").map(str::to_owned));
            if let Some(name)=strp(p,"name") {model.rename_workspace(ws,Some(name.to_owned()));}
            let pane=model.workspaces.get(ws).map(|x|x.focused_pane).unwrap_or(PaneId(0));
            let tab=model.panes.get(pane).map(|x|x.active_tab).unwrap_or(TabId(0));
            Response::ok(id,json!({"workspace":ws.0,"pane":pane.0,"tab":tab.0}))
        }
        "select_workspace" => { let Some(w)=u32p(p,"workspace") else{return invalid(id,"workspace is required");}; if model.select_workspace(WorkspaceId(w)){Response::ok(id,json!({}))}else{missing(id,"workspace",w)} }
        "rename_workspace" => { if p.get("name").and_then(Value::as_str).is_none(){return invalid(id,"name is required and must be a string");} let explicit=match optional_u32(p,"workspace",id){Ok(v)=>v,Err(e)=>return e}; let ws=explicit.or_else(||model.focused_workspace().map(|x|x.0)); let Some(w)=ws else{return err(id,"not_found","no focused workspace");}; if model.rename_workspace(WorkspaceId(w),strp(p,"name").map(str::to_owned)){Response::ok(id,json!({}))}else{missing(id,"workspace",w)} }
        "close_workspace" => { let explicit=match optional_u32(p,"workspace",id){Ok(v)=>v,Err(e)=>return e}; let ws=explicit.or_else(||model.focused_workspace().map(|x|x.0)); let Some(w)=ws else{return err(id,"not_found","no focused workspace");}; if model.close_workspace(WorkspaceId(w)){Response::ok(id,json!({}))}else{missing(id,"workspace",w)} }
        "new_tab" => {
            if p.get("command").is_some(){return err(id,"unsupported","command-after-prompt scheduling is not connected to the runtime");}
            if p.get("cwd").is_some_and(|v|!v.is_string()){return invalid(id,"cwd must be a string");}
            let explicit=match optional_u32(p,"pane",id){Ok(v)=>v,Err(e)=>return e}; let pane=explicit.or_else(||model.focused_pane().map(|x|x.0)); let Some(v)=pane else{return err(id,"not_found","no focused pane");};
            if !model.panes.contains(PaneId(v)){return missing(id,"pane",v);}
            let tab=model.new_tab(PaneId(v),false).unwrap_or(TabId(0));
            if let Some(cwd)=strp(p,"cwd"){ if let Some(t)=model.tabs.get_mut(tab){t.cwd=Some(cwd.to_owned());} if let Some(effect)=model.pending_effects.iter_mut().rev().find(|e|matches!(e,super::super::model::ModelEffect::SpawnTab{tab:t,..} if *t==tab)){if let super::super::model::ModelEffect::SpawnTab{cwd:c,..}=effect{*c=Some(cwd.to_owned());}} }
            Response::ok(id,json!({"tab":tab.0}))
        }
        "split" => {
            if p.get("command").is_some(){return err(id,"unsupported","command-after-prompt scheduling is not connected to the runtime");}
            let explicit=match optional_u32(p,"pane",id){Ok(v)=>v,Err(e)=>return e}; let pane=explicit.or_else(||model.focused_pane().map(|x|x.0)); let Some(v)=pane else{return err(id,"not_found","no focused pane");}; if !model.panes.contains(PaneId(v)){return missing(id,"pane",v);}
            let dir=match strp(p,"direction"){Some("right")=>Dir::Right,Some("down")=>Dir::Down,_=>return invalid(id,"direction must be 'right' or 'down'")};
            if p.get("cwd").is_some_and(|x|!x.is_string()){return invalid(id,"cwd must be a string");}
            let Some((np,tab))=model.split_pane(PaneId(v),dir,true) else{return err(id,"internal_error","split failed");};
            if let Some(cwd)=strp(p,"cwd"){ if let Some(t)=model.tabs.get_mut(tab){t.cwd=Some(cwd.to_owned());} if let Some(effect)=model.pending_effects.iter_mut().rev().find(|e|matches!(e,super::super::model::ModelEffect::SpawnTab{tab:t,..} if *t==tab)){if let super::super::model::ModelEffect::SpawnTab{cwd:c,..}=effect{*c=Some(cwd.to_owned());}} }
            Response::ok(id,json!({"pane":np.0,"tab":tab.0}))
        }
        "focus" => {
            let pane=u32p(p,"pane"); let tab=u32p(p,"tab");
            if p.get("pane").is_some()&&pane.is_none() || p.get("tab").is_some()&&tab.is_none() || (pane.is_some()&&tab.is_some()){return invalid(id,"provide at most one valid pane or tab id");}
            let target=if let Some(t)=tab { let Some(te)=model.tabs.get(TabId(t))else{return missing(id,"tab",t);}; let pi=te.pane; let wid=model.panes.get(pi).and_then(|x|model.workspaces.get(x.workspace)).map(|x|x.id); if let Some(w)=wid {model.select_workspace(w); if let Some(ws)=model.workspaces.get_mut(w){ws.focused_pane=pi;} if let Some(pn)=model.panes.get_mut(pi){pn.active_tab=TabId(t);} true}else{false} } else if let Some(v)=pane { let Some(pn)=model.panes.get(PaneId(v))else{return missing(id,"pane",v);}; let ws=pn.workspace; model.select_workspace(ws); if let Some(w)=model.workspaces.get_mut(ws){w.focused_pane=PaneId(v);} true } else {true}; if target{Response::ok(id,json!({}))}else{err(id,"not_found","focus target not found")}
        }
        "close" => { let tab=match optional_u32(p,"tab",id){Ok(v)=>v,Err(e)=>return e};let pane=match optional_u32(p,"pane",id){Ok(v)=>v,Err(e)=>return e}; if tab.is_some()&&pane.is_some(){return invalid(id,"provide either tab or pane");} if let Some(t)=tab {if model.close_tab(TabId(t)){Response::ok(id,json!({}))}else{missing(id,"tab",t)}}else if let Some(v)=pane {if model.close_pane(PaneId(v)){Response::ok(id,json!({}))}else{missing(id,"pane",v)}}else{err(id,"unsupported","closing the focused target requires an explicit tab or pane id")}}
        "send_text" => {let Some(text)=strp(p,"text")else{return invalid(id,"text is required");}; let explicit=match optional_u32(p,"tab",id){Ok(v)=>v,Err(e)=>return e}; let t=explicit.or_else(||model.focused_tab().map(|x|x.0)); let Some(v)=t else{return err(id,"not_found","no focused tab");}; let Some(tab)=model.tabs.get(TabId(v))else{return missing(id,"tab",v);}; let Some(session)=tab.session.as_ref()else{return err(id,"not_ready","tab session is not running");}; session.write(text.as_bytes()); Response::ok(id,json!({}))}
        "read_screen" => {let explicit=match optional_u32(p,"tab",id){Ok(v)=>v,Err(e)=>return e};let t=explicit.or_else(||model.focused_tab().map(|x|x.0)); let Some(v)=t else{return err(id,"not_found","no focused tab");};let Some(tab)=model.tabs.get(TabId(v))else{return missing(id,"tab",v);};let Some(s)=tab.session.as_ref()else{return err(id,"not_ready","tab session is not running");};let lines=p.get("lines").and_then(Value::as_u64).unwrap_or(24).clamp(1,10000) as usize;let scrollback=p.get("scrollback").and_then(Value::as_bool).unwrap_or(false);let term=s.term.lock().unwrap_or_else(|e|e.into_inner());let total=term.scrollback_len()+usize::from(term.rows());let count=if scrollback{lines.min(total)}else{lines.min(usize::from(term.rows()))};let range=termin_vt::LineRange{start:termin_vt::AbsLine(total.saturating_sub(count) as u64),end:termin_vt::AbsLine(total as u64)}; Response::ok(id,json!({"text":term.text_range(range)}))}
        "notify" => {let Some(body)=strp(p,"body")else{return invalid(id,"body is required");};let explicit=match optional_u32(p,"tab",id){Ok(v)=>v,Err(e)=>return e};let t=explicit.or_else(||model.focused_tab().map(|x|x.0));let Some(v)=t else{return err(id,"not_found","no focused tab");};let Some(tab)=model.tabs.get_mut(TabId(v))else{return missing(id,"tab",v);}; tab.attention.unread=true;tab.attention.kind=AttentionKind::Notify;tab.attention.message=Some(body.into());tab.attention.at=Some(Instant::now());if let Some(ws)=model.panes.get(tab.pane).and_then(|pn|model.workspaces.get_mut(pn.workspace)){ws.meta.status=Some(body.to_owned());} Response::ok(id,json!({}))}
        "set_status" => {let Some(text)=strp(p,"text")else{return invalid(id,"text is required");};let explicit=match optional_u32(p,"workspace",id){Ok(v)=>v,Err(e)=>return e};let ws=explicit.or_else(||model.focused_workspace().map(|x|x.0));let Some(v)=ws else{return err(id,"not_found","no focused workspace");};let Some(w)=model.workspaces.get_mut(WorkspaceId(v))else{return missing(id,"workspace",v);};w.meta.status=(!text.is_empty()).then(||text.to_owned());Response::ok(id,json!({}))}
        "send_keys" | "run_action" => err(id,"unsupported","key/action dispatch is not connected to IPC runtime"),
        "subscribe" => err(id,"unsupported","event subscription delivery is not connected to the reactor"),
        _ => err(id,"method_not_found","unsupported IPC method"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::Size;

    fn req(method: &str, params: Value) -> Request {
        Request::new(17, method, params)
    }
    fn model() -> AppModel {
        let mut m = AppModel::new();
        m.new_window(Some(WindowId(3)), Size::new(800.0, 600.0), None);
        m
    }

    #[test]
    fn dispatches_model_routing_and_listing() {
        let mut m = model();
        let ws = dispatch_request(&mut m, &req("new_workspace", json!({"name":"work"})));
        assert!(ws.is_ok());
        let r = dispatch_request(&mut m, &req("list_workspaces", json!({"window":3})));
        assert_eq!(r.result.unwrap().as_array().unwrap().len(), 2);
        let split = dispatch_request(&mut m, &req("split", json!({"direction":"right"})));
        assert!(split.is_ok());
        let new_ws = m.focused_workspace().unwrap().0;
        let panes = dispatch_request(&mut m, &req("list_panes", json!({"workspace":new_ws})));
        assert_eq!(panes.result.unwrap().as_array().unwrap().len(), 2);
        let target = m.panes.ids().next().unwrap().0;
        assert!(dispatch_request(&mut m, &req("focus", json!({"pane":target}))).is_ok());
    }
    #[test]
    fn rejects_invalid_and_unsupported_requests_without_mutation() {
        let mut m = model();
        let n = m.workspaces.len();
        assert_eq!(
            dispatch_request(&mut m, &req("split", json!({"direction":"left"})))
                .error
                .unwrap()
                .code,
            "invalid_params"
        );
        assert_eq!(
            dispatch_request(&mut m, &req("new_workspace", json!({"command":"echo hi"})))
                .error
                .unwrap()
                .code,
            "unsupported"
        );
        assert_eq!(
            dispatch_request(&mut m, &req("send_keys", json!({"keys":["enter"]})))
                .error
                .unwrap()
                .code,
            "unsupported"
        );
        assert_eq!(
            dispatch_request(&mut m, &req("wat", json!({})))
                .error
                .unwrap()
                .code,
            "method_not_found"
        );
        assert_eq!(m.workspaces.len(), n);
    }
    #[test]
    fn close_and_selection_route_explicit_ids() {
        let mut m = model();
        let ws = m.focused_workspace().unwrap();
        assert!(dispatch_request(
            &mut m,
            &req(
                "rename_workspace",
                json!({"workspace":ws.0,"name":"renamed"})
            )
        )
        .is_ok());
        assert_eq!(
            m.workspaces.get(ws).unwrap().name.as_deref(),
            Some("renamed")
        );
        assert!(
            dispatch_request(&mut m, &req("select_workspace", json!({"workspace":ws.0}))).is_ok()
        );
        assert!(
            dispatch_request(&mut m, &req("close_workspace", json!({"workspace":ws.0}))).is_ok()
        );
        assert!(!m.workspaces.contains(ws));
    }

    #[test]
    fn malformed_explicit_targets_never_fall_back_to_focused_entities() {
        let mut m = model();
        let ws = m.focused_workspace().unwrap();
        let pane = m.focused_pane().unwrap();
        let tab = m.focused_tab().unwrap();

        let cases = [
            ("close_workspace", json!({"workspace":"bad"})),
            ("rename_workspace", json!({"workspace":-1,"name":"wrong"})),
            ("new_workspace", json!({"window":"bad"})),
            ("new_tab", json!({"pane":false})),
            ("split", json!({"pane":"bad","direction":"right"})),
            ("send_text", json!({"tab":"bad","text":"must not send"})),
            ("read_screen", json!({"tab":-1})),
            ("notify", json!({"tab":false,"body":"wrong target"})),
            ("set_status", json!({"workspace":"bad","text":"wrong"})),
        ];
        for (method, params) in cases {
            let response = dispatch_request(&mut m, &req(method, params));
            assert_eq!(response.error.unwrap().code, "invalid_params", "{method}");
        }
        assert!(m.workspaces.contains(ws));
        assert!(m.panes.contains(pane));
        assert!(m.tabs.contains(tab));
        assert_eq!(m.workspaces.len(), 1);
        assert_eq!(m.panes.len(), 1);
    }

    #[test]
    fn malformed_close_target_does_not_close_other_valid_target() {
        let mut m = model();
        let pane = m.focused_pane().unwrap();
        let response = dispatch_request(&mut m, &req("close", json!({"tab":"bad","pane":pane.0})));
        assert_eq!(response.error.unwrap().code, "invalid_params");
        assert!(m.panes.contains(pane));
        assert_eq!(m.pending_effects.len(), 1); // Only initial window's spawn effect.
    }
}
