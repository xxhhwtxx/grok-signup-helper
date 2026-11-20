mod mailbox;
mod store;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize)]
struct Session {
    id: String,
    email: String,
    since: i64,
    stage: String,
    polling: bool,
    mail: Option<mailbox::Mail>,
    error: String,
    mode: String,
    #[serde(skip)]
    delivered: String,
}
struct AppState {
    root: PathBuf,
    store_lock: Mutex<()>,
    session: Mutex<Option<Session>>,
}
fn local(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("此命令只能在助手主窗口使用。".into())
    }
}
fn account(state: &AppState, email: &str) -> Result<store::Account, String> {
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    store::load(&state.root)?
        .accounts
        .into_iter()
        .find(|a| a.email.eq_ignore_ascii_case(email))
        .ok_or("找不到此邮箱，请先导入。".into())
}
fn notify(app: &AppHandle) {
    let _ = app.emit_to("main", "grok://changed", ());
}
fn status(state: &AppState, email: &str, value: &str, note: &str) -> Result<(), String> {
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    let mut db = store::load(&state.root)?;
    let a = db
        .accounts
        .iter_mut()
        .find(|a| a.email.eq_ignore_ascii_case(email))
        .ok_or("找不到此邮箱。")?;
    a.status = value.into();
    a.note = note.into();
    a.updated_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    store::save(&state.root, &db)
}
fn handle_stage(app: &AppHandle, id: &str, stage: &str, detail: &str, identity: &str) {
    if ![
        "form",
        "consent_required",
        "captcha",
        "code_requested",
        "verification",
        "code_filled",
        "submitted",
        "manual_required",
        "registered",
        "authenticated",
        "access_blocked",
        "page",
        "error",
    ]
    .contains(&stage)
    {
        return;
    }
    let state = app.state::<AppState>();
    let changed = {
        let Ok(mut guard) = state.session.lock() else {
            return;
        };
        let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
            return;
        };
        // Filling or submitting a code is not evidence that registration succeeded.
        if stage == "authenticated" && !identity.eq_ignore_ascii_case(&s.email) {
            return;
        }
        s.stage = stage.into();
        if ["code_requested", "verification"].contains(&stage) && s.mode != "work" {
            s.polling = !s.mail.as_ref().is_some_and(|m| m.eligible);
        }
        if ["registered", "authenticated", "error"].contains(&stage) {
            s.polling = false;
        }
        if stage == "error" {
            s.error = detail.chars().take(300).collect();
        } else if stage != "page" {
            s.error.clear();
        }
        (
            s.email.clone(),
            stage == "registered" || stage == "authenticated",
        )
    };
    if changed.1 {
        let saved = status(
            &state,
            &changed.0,
            "done",
            if stage == "authenticated" {
                "官网显示所选邮箱已登录。"
            } else {
                "官网显示注册成功。"
            },
        );
        if let Err(error) = saved {
            if let Ok(mut guard) = state.session.lock() {
                if let Some(s) = guard.as_mut().filter(|s| s.id == id) {
                    s.stage = "error".into();
                    s.error = format!("官网已确认成功，但保存注册记录失败：{error}");
                }
            }
        }
    }
    notify(app);
}

#[cfg(windows)]
fn attach_bridge(window: &WebviewWindow, app: AppHandle, id: String) -> Result<(), String> {
    use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
    use windows::core::PWSTR;
    window
        .with_webview(move |view| {
            let _ = (|| -> windows::core::Result<()> {
                let core = unsafe { view.controller().CoreWebView2()? };
                let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut source = PWSTR::null();
                    unsafe {
                        args.Source(&mut source)?;
                    }
                    if !url::Url::parse(&take_pwstr(source))
                        .is_ok_and(|u| mailbox::trusted_service(&u))
                    {
                        return Ok(());
                    }
                    let mut raw = PWSTR::null();
                    if unsafe { args.TryGetWebMessageAsString(&mut raw) }.is_err() {
                        return Ok(());
                    }
                    let raw = take_pwstr(raw);
                    if raw.len() > 2048 {
                        return Ok(());
                    }
                    let Ok(m) = serde_json::from_str::<Value>(&raw) else {
                        return Ok(());
                    };
                    if m["grokHelper"] == true && m["id"].as_str() == Some(id.as_str()) {
                        if let Some(stage) = m["stage"].as_str() {
                            handle_stage(
                                &app,
                                &id,
                                stage,
                                m["detail"].as_str().unwrap_or(""),
                                m["identity"].as_str().unwrap_or(""),
                            );
                        }
                    }
                    Ok(())
                }));
                let mut token = 0;
                unsafe {
                    core.add_WebMessageReceived(&handler, &mut token)?;
                }
                Ok(())
            })();
        })
        .map_err(|_| "无法连接页面状态。".into())
}

#[tauri::command]
fn get_state(window: WebviewWindow, state: State<AppState>) -> Result<Value, String> {
    local(&window)?;
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    let db = store::load(&state.root)?;
    let rows: Vec<_> = db.accounts.iter().map(|a| json!({"email":a.email,"status":a.status,"note":a.note,"updated_at":a.updated_at,"password_set":!a.password.is_empty()})).collect();
    Ok(
        json!({"accounts":rows,"first_name":db.first_name,"last_name":db.last_name,"default_password_set":!db.default_password.is_empty(),"session":state.session.lock().map_err(|_|"注册状态暂时不可用。")?.clone()}),
    )
}
#[tauri::command]
fn import_text(
    window: WebviewWindow,
    state: State<AppState>,
    text: String,
    password: Option<String>,
) -> Result<store::ImportResult, String> {
    local(&window)?;
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    let mut db = store::load(&state.root)?;
    let r = store::import(&mut db, &text, password.as_deref());
    if r.added + r.updated > 0 {
        store::save(&state.root, &db)?;
    }
    Ok(r)
}
#[tauri::command]
async fn import_file(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    password: Option<String>,
) -> Result<Option<store::ImportResult>, String> {
    local(&window)?;
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("邮箱文本", &["txt"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|_| "文件路径无效。")?;
    if std::fs::metadata(&path)
        .map_err(|_| "无法打开文件。")?
        .len()
        > 1024 * 1024
    {
        return Err("导入文件不能超过 1 MB。".into());
    }
    let text = store::decode_text(&std::fs::read(path).map_err(|_| "无法读取文件。")?)?;
    import_text(window, state, text, password).map(Some)
}
#[tauri::command]
fn credential(
    window: WebviewWindow,
    state: State<AppState>,
    email: String,
) -> Result<String, String> {
    local(&window)?;
    Ok(account(&state, &email)?.password)
}

fn deliver(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let Ok(mut guard) = state.session.lock() else {
        return;
    };
    let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
        return;
    };
    let Some(m) = s.mail.as_ref().filter(|m| m.eligible) else {
        return;
    };
    if m.codes.is_empty() && m.links.len() == 1 && s.stage == "verification" {
        let key = format!("{}:{}:link", s.since, m.id);
        if s.delivered != key {
            if let (Some(win), Ok(uri)) = (
                app.get_webview_window("signup"),
                url::Url::parse(&m.links[0]),
            ) {
                if mailbox::trusted_service(&uri)
                    && win.url().is_ok_and(|u| mailbox::trusted_service(&u))
                    && win.navigate(uri).is_ok()
                {
                    s.delivered = key;
                }
            }
        }
        return;
    }
    if m.codes.len() != 1 {
        return;
    }
    let key = format!("{}:{}:{}", s.since, m.id, m.codes[0]);
    if s.stage == "opening"
        || s.stage == "error"
        || (s.delivered == key && s.stage != "verification")
    {
        return;
    }
    let Some(win) = app.get_webview_window("signup") else {
        return;
    };
    if !win.url().is_ok_and(|u| mailbox::trusted_service(&u)) {
        return;
    }
    if win
        .eval(format!(
            "window.__grokAssistFillCode?.({});",
            json!(m.codes[0])
        ))
        .is_ok()
    {
        s.delivered = key;
    }
}
fn fetch_for(
    app: &AppHandle,
    id: &str,
    email: &str,
    since: i64,
) -> Result<Option<mailbox::Mail>, String> {
    let state = app.state::<AppState>();
    let a = account(&state, email)?;
    if !mailbox::is_graph(&a)
        && mailbox::validate_url(&a.mail_api_url, &a.email)?.host_str() == Some("icsms.top")
    {
        if let Some(w) = app.get_webview_window("mailbox") {
            let _ = w.eval("window.__pickupRefresh?.();");
        }
        return Ok(state
            .session
            .lock()
            .map_err(|_| "注册状态暂时不可用。")?
            .as_ref()
            .filter(|s| s.id == id)
            .and_then(|s| s.mail.clone()));
    }
    let result = mailbox::fetch(&account(&state, email)?, Some(since));
    let mut guard = state.session.lock().map_err(|_| "注册状态暂时不可用。")?;
    let Some(s) = guard.as_mut().filter(|s| s.id == id && s.since == since) else {
        return Ok(None);
    };
    let output = match result {
        Ok(mail) => {
            s.mail = mail.clone();
            if s.stage != "error" {
                s.error.clear();
            }
            if mail.as_ref().is_some_and(|m| m.eligible) {
                s.polling = false;
            }
            Ok(mail)
        }
        Err(e) => {
            s.error = e.clone();
            Err(e)
        }
    };
    drop(guard);
    deliver(app, id);
    notify(app);
    output
}
fn poll(app: AppHandle, id: String) {
    std::thread::spawn(move || loop {
        let snapshot = {
            let state = app.state::<AppState>();
            let Ok(mut guard) = state.session.lock() else {
                return;
            };
            let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
                return;
            };
            if ["registered", "authenticated"].contains(&s.stage.as_str()) {
                return;
            }
            if s.polling && chrono::Utc::now().timestamp_millis() - s.since > 10 * 60 * 1000 {
                s.polling = false;
                s.error = "验证码等待超过 10 分钟。请点击“重发验证码”重新开始。".into();
                notify(&app);
            }
            (s.email.clone(), s.since, s.polling)
        };
        if snapshot.2 {
            let _ = fetch_for(&app, &id, &snapshot.0, snapshot.1);
        }
        deliver(&app, &id);
        // Replay status after the WebView2 message bridge has attached. The
        // first document-created message can arrive before its handler.
        if let Some(win) = app.get_webview_window("signup") {
            if win.url().is_ok_and(|u| mailbox::trusted_service(&u)) {
                let _ = win.eval("window.__grokAssistReport?.();");
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

async fn wait_closed(app: &AppHandle, labels: &[&str]) {
    let app = app.clone();
    let labels: Vec<String> = labels.iter().map(|s| (*s).to_owned()).collect();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if labels.iter().all(|l| app.get_webview_window(l).is_none()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })
    .await;
}

#[tauri::command]
async fn start_session(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    mode: String,
    consent: bool,
) -> Result<(), String> {
    local(&window)?;
    if mode == "signup" && !consent {
        return Err("请先阅读并勾选 Grok 服务条款与隐私政策。".into());
    }
    if !["signup", "login", "work"].contains(&mode.as_str()) {
        return Err("未知操作。".into());
    }
    let a = account(&state, &email)?;
    if mode != "work" && a.password.is_empty() {
        return Err("请先设置注册密码。".into());
    }
    {
        let guard = state.session.lock().map_err(|_| "注册状态暂时不可用。")?;
        if guard
            .as_ref()
            .is_some_and(|s| !["registered", "authenticated"].contains(&s.stage.as_str()))
        {
            return Err("请先结束当前操作。".into());
        }
    }
    let mut closing = Vec::new();
    if let Some(w) = app.get_webview_window("signup") {
        let _ = w.destroy();
        closing.push("signup");
    }
    if let Some(w) = app.get_webview_window("mailbox") {
        let _ = w.destroy();
        closing.push("mailbox");
    }
    if !closing.is_empty() {
        wait_closed(&app, &closing).await;
    }
    let id = uuid::Uuid::new_v4().to_string();
    *state.session.lock().map_err(|_| "注册状态暂时不可用。")? = Some(Session {
        id: id.clone(),
        email: a.email.clone(),
        since: (chrono::Utc::now().timestamp_millis() / 1000) * 1000,
        stage: "opening".into(),
        polling: false,
        mail: None,
        error: String::new(),
        mode: mode.clone(),
        delivered: String::new(),
    });
    if mode == "signup" {
        if let Err(error) = status(&state, &email, "waiting", "自动注册已开始，等待官网确认。")
        {
            *state.session.lock().map_err(|_| "注册状态暂时不可用。")? = None;
            return Err(error);
        }
    }
    notify(&app);
    if !state
        .session
        .lock()
        .map_err(|_| "注册状态暂时不可用。")?
        .as_ref()
        .is_some_and(|s| s.id == id)
    {
        return Err("操作已取消。".into());
    }
    let (first_name, last_name) = {
        let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
        let db = store::load(&state.root)?;
        (db.first_name, db.last_name)
    };
    let config = json!({"id":id,"email":a.email,"password":a.password,"first_name":first_name,"last_name":last_name,"mode":mode,"consent":consent || mode != "signup","hosts":["accounts.x.ai","grok.com","x.ai"],"app_hosts":["grok.com"]});
    let uri = match mode.as_str() {
        "signup" => "https://accounts.x.ai/sign-up?redirect=grok-com",
        "login" => "https://accounts.x.ai/sign-in?redirect=grok-com",
        _ => "https://grok.com/",
    }
    .parse()
    .unwrap();
    let win = tauri::WebviewWindowBuilder::new(&app, "signup", tauri::WebviewUrl::External(uri))
        .title(format!(
            "Grok {} · {}",
            if mode == "signup" {
                "注册"
            } else if mode == "login" {
                "登录"
            } else {
                "官网"
            },
            a.email
        ))
        .inner_size(850.0, 820.0)
        .min_inner_size(620.0, 650.0)
        .initialization_script(
            include_str!("driver.js").replace("__GROK_CONFIG__", &config.to_string()),
        )
        .data_directory(state.root.join("browsers").join(format!(
            "{:x}",
            Sha256::digest(email.to_ascii_lowercase().as_bytes())
        )))
        .on_navigation(|u| mailbox::allowed_navigation(u) || u.as_str() == "about:blank")
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build();
    let win = match win {
        Ok(w) => w,
        Err(_) => {
            *state.session.lock().map_err(|_| "注册状态暂时不可用。")? = None;
            return Err("无法打开官网窗口，请检查 Edge WebView2 是否已安装。".into());
        }
    };
    #[cfg(windows)]
    attach_bridge(&win, app.clone(), id.clone())?;
    let _ = win.eval("window.__grokAssistReport?.();");
    let handle = app.clone();
    let closed_id = id.clone();
    win.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let state = handle.state::<AppState>();
            if let Ok(mut guard) = state.session.lock() {
                if guard.as_ref().is_some_and(|s| s.id == closed_id) {
                    *guard = None;
                }
            }
            notify(&handle);
        }
    });
    if !mailbox::is_graph(&a)
        && mailbox::validate_url(&a.mail_api_url, &a.email)?.host_str() == Some("icsms.top")
    {
        if let Err(e) = open_pickup(&app, &state, &a, &id) {
            handle_stage(&app, &id, "error", &e, "");
            return Err(e);
        }
    }
    poll(app.clone(), id);
    notify(&app);
    Ok(())
}

#[tauri::command]
async fn fetch_mail(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
) -> Result<Option<mailbox::Mail>, String> {
    local(&window)?;
    let a = account(&state, &email)?;
    let session = state
        .session
        .lock()
        .map_err(|_| "注册状态暂时不可用。")?
        .clone()
        .filter(|s| s.email.eq_ignore_ascii_case(&email));
    tauri::async_runtime::spawn_blocking(move || match session {
        Some(s) => fetch_for(&app, &s.id, &email, s.since),
        None => {
            if !mailbox::is_graph(&a)
                && mailbox::validate_url(&a.mail_api_url, &a.email)?.host_str() == Some("icsms.top")
            {
                Err("请先打开此邮箱的注册或登录窗口，助手会自动打开取信页。".into())
            } else {
                mailbox::fetch(&a, None)
            }
        }
    })
    .await
    .map_err(|_| "取信任务中断。")?
}
#[tauri::command]
fn retry_code(window: WebviewWindow, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    local(&window)?;
    let mut guard = state.session.lock().map_err(|_| "注册状态暂时不可用。")?;
    let s = guard.as_mut().ok_or("请先打开注册窗口。")?;
    if s.mode != "signup" || ["registered", "authenticated"].contains(&s.stage.as_str()) {
        return Err("当前不是等待注册验证码的步骤。".into());
    }
    let win = app.get_webview_window("signup").ok_or("官网窗口已关闭。")?;
    if !win.url().is_ok_and(|u| mailbox::trusted_service(&u)) {
        return Err("请在官方注册页面重发。".into());
    }
    s.since = (chrono::Utc::now().timestamp_millis() / 1000) * 1000;
    s.polling = false;
    s.mail = None;
    s.error.clear();
    s.delivered.clear();
    s.stage = "form".into();
    drop(guard);
    win.eval("window.__grokAssistRetry?.();")
        .map_err(|_| "无法请求重发。")?;
    notify(&app);
    Ok(())
}
#[tauri::command]
fn fill_code(
    window: WebviewWindow,
    app: AppHandle,
    state: State<AppState>,
    code: String,
) -> Result<(), String> {
    local(&window)?;
    let guard = state.session.lock().map_err(|_| "注册状态暂时不可用。")?;
    if !guard
        .as_ref()
        .and_then(|s| s.mail.as_ref())
        .is_some_and(|m| m.eligible && m.codes.contains(&code))
    {
        return Err("验证码没有与本次注册匹配。".into());
    }
    let win = app.get_webview_window("signup").ok_or("官网窗口已关闭。")?;
    if !win.url().is_ok_and(|u| mailbox::trusted_service(&u)) {
        return Err("只能填写到 Grok 官网。".into());
    }
    win.eval(format!("window.__grokAssistFillCode?.({});", json!(code)))
        .map_err(|_| "无法填入验证码。".into())
}
#[tauri::command]
fn stop_session(
    window: WebviewWindow,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    local(&window)?;
    *state.session.lock().map_err(|_| "注册状态暂时不可用。")? = None;
    if let Some(w) = app.get_webview_window("mailbox") {
        let _ = w.destroy();
    }
    if let Some(w) = app.get_webview_window("signup") {
        let _ = w.destroy();
    }
    notify(&app);
    Ok(())
}
#[tauri::command]
fn mark_result(
    window: WebviewWindow,
    app: AppHandle,
    state: State<AppState>,
    email: String,
    done: bool,
) -> Result<(), String> {
    local(&window)?;
    status(
        &state,
        &email,
        if done { "done" } else { "failed" },
        if done {
            "用户已在官网确认注册成功。"
        } else {
            "用户标记需重试。"
        },
    )?;
    let matching = state
        .session
        .lock()
        .map_err(|_| "注册状态暂时不可用。")?
        .as_ref()
        .is_some_and(|s| s.email.eq_ignore_ascii_case(&email));
    if matching {
        stop_session(window, app.clone(), state)?;
    }
    notify(&app);
    Ok(())
}
#[tauri::command]
fn open_terms(window: WebviewWindow, app: AppHandle, kind: String) -> Result<(), String> {
    local(&window)?;
    let uri = match kind.as_str() {
        "service" => "https://x.ai/legal/terms-of-service",
        "privacy" => "https://x.ai/privacy-policy",
        _ => return Err("未知条款页面。".into()),
    };
    if let Some(w) = app.get_webview_window("terms") {
        let _ = w.destroy();
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "terms",
        tauri::WebviewUrl::External(uri.parse().unwrap()),
    )
    .title("Grok 官方条款")
    .inner_size(850.0, 800.0)
    .on_navigation(mailbox::trusted_service)
    .build()
    .map_err(|_| "无法打开条款页面。")?;
    Ok(())
}
#[tauri::command]
async fn export_accounts(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    local(&window)?;
    let text = {
        let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
        store::load(&state.root)?
            .accounts
            .iter()
            .map(|a| {
                if mailbox::is_graph(a) {
                    format!(
                        "{}----{}----{}----{}",
                        a.email, a.mail_password, a.mail_client_id, a.mail_refresh_token
                    )
                } else {
                    format!("{}----{}----{}", a.email, a.password, a.mail_api_url)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let Some(file) = app
        .dialog()
        .file()
        .set_file_name("Grok邮箱备份.txt")
        .add_filter("文本", &["txt"])
        .blocking_save_file()
    else {
        return Ok(false);
    };
    std::fs::write(file.into_path().map_err(|_| "保存路径无效。")?, text)
        .map_err(|_| "无法写入备份。")?;
    Ok(true)
}
fn receive_pickup(app: &AppHandle, id: &str, value: Value) {
    let state = app.state::<AppState>();
    let (email, since) = match state.session.lock() {
        Ok(g) => match g.as_ref().filter(|s| s.id == id) {
            Some(s) => (s.email.clone(), s.since),
            None => return,
        },
        Err(_) => return,
    };
    if value["challenge"] == true {
        if let Some(w) = app.get_webview_window("mailbox") {
            let _ = w.show();
            let _ = w.set_focus();
        }
        if let Ok(mut g) = state.session.lock() {
            if let Some(s) = g.as_mut().filter(|s| s.id == id) {
                s.error = "取信页面需要人工验证，请在邮箱窗口完成。".into();
            }
        }
        notify(app);
        return;
    }
    let result = if let Some(error) = value["error"].as_str() {
        Err(error.chars().take(300).collect::<String>())
    } else {
        mailbox::parse(&value["payload"], &email, Some(since))
    };
    if let Ok(mut g) = state.session.lock() {
        if let Some(s) = g.as_mut().filter(|s| s.id == id && s.since == since) {
            match result {
                Ok(mail) => {
                    s.mail = mail;
                    if s.mail.as_ref().is_some_and(|m| m.eligible) {
                        s.polling = false;
                    }
                    if s.stage != "error" {
                        s.error.clear();
                    }
                }
                Err(e) => s.error = e,
            }
        }
    }
    deliver(app, id);
    notify(app);
}
#[cfg(windows)]
fn open_pickup(
    app: &AppHandle,
    state: &AppState,
    a: &store::Account,
    id: &str,
) -> Result<(), String> {
    use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
    use windows::core::PWSTR;
    let uri = mailbox::validate_url(&a.mail_api_url, &a.email)?;
    let config = json!({"id":id,"base":mailbox::base(&a.email)});
    let win =
        tauri::WebviewWindowBuilder::new(app, "mailbox", tauri::WebviewUrl::External(uri.clone()))
            .title(format!("取信 · {}", a.email))
            .visible(false)
            .inner_size(850.0, 720.0)
            .data_directory(state.root.join("mail-browser"))
            .initialization_script(
                include_str!("pickup.js").replace("__PICKUP_CONFIG__", &config.to_string()),
            )
            .on_navigation(move |u| u.as_str() == "about:blank" || u.as_str() == uri.as_str())
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()
            .map_err(|_| "无法打开取信窗口。")?;
    let handle = app.clone();
    let session_id = id.to_owned();
    win.with_webview(move |view| {
        let _ = (|| -> windows::core::Result<()> {
            let core = unsafe { view.controller().CoreWebView2()? };
            let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut source = PWSTR::null();
                unsafe {
                    args.Source(&mut source)?;
                }
                let allowed = url::Url::parse(&take_pwstr(source)).is_ok_and(|u| {
                    u.scheme() == "https"
                        && u.host_str() == Some("icsms.top")
                        && u.port_or_known_default() == Some(443)
                        && u.path() == "/pickup"
                        && u.username().is_empty()
                        && u.password().is_none()
                });
                if !allowed {
                    return Ok(());
                }
                let mut raw = PWSTR::null();
                if unsafe { args.TryGetWebMessageAsString(&mut raw) }.is_err() {
                    return Ok(());
                }
                let raw = take_pwstr(raw);
                if raw.len() > 65536 {
                    return Ok(());
                }
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    if value["pickupHelper"] == true
                        && value["id"].as_str() == Some(session_id.as_str())
                    {
                        receive_pickup(&handle, &session_id, value);
                    }
                }
                Ok(())
            }));
            let mut token = 0;
            unsafe {
                core.add_WebMessageReceived(&handler, &mut token)?;
            }
            Ok(())
        })();
    })
    .map_err(|_| "无法连接取信页面。".to_owned())?;
    let _ = win.eval("window.__pickupRefresh?.();");
    Ok(())
}
#[cfg(not(windows))]
fn open_pickup(_: &AppHandle, _: &AppState, _: &store::Account, _: &str) -> Result<(), String> {
    Err("此版本的网页取信适配仅支持 Windows。".into())
}
#[tauri::command]
fn save_profile(
    window: WebviewWindow,
    state: State<AppState>,
    first_name: String,
    last_name: String,
) -> Result<(), String> {
    local(&window)?;
    if first_name.chars().count() > 100 || last_name.chars().count() > 100 {
        return Err("姓名不能超过 100 个字符。".into());
    }
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    let mut db = store::load(&state.root)?;
    db.first_name = first_name.trim().into();
    db.last_name = last_name.trim().into();
    store::save(&state.root, &db)
}
#[tauri::command]
fn account_text(
    window: WebviewWindow,
    state: State<AppState>,
    email: Option<String>,
) -> Result<String, String> {
    local(&window)?;
    let _lock = state.store_lock.lock().map_err(|_| "账号库暂时不可用。")?;
    let db = store::load(&state.root)?;
    let rows:Vec<_>=db.accounts.iter().filter(|a|email.as_ref().map_or(a.status=="done",|e|a.email.eq_ignore_ascii_case(e))).map(|a|json!({"email":a.email,"configured_password":a.password,"registration_status":a.status})).collect();
    if rows.is_empty() {
        return Err("没有可复制的账号记录。".into());
    }
    serde_json::to_string_pretty(&rows).map_err(|_| "无法复制账号资料。".into())
}
fn seed(root: &Path) -> Result<(), String> {
    if root.join("accounts.json").exists() {
        return Ok(());
    }
    let mut db = store::Database::default();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned))
    {
        if let Ok(raw) = std::fs::read(dir.join("mail-defaults.json")) {
            if let Ok(v) = serde_json::from_slice::<Value>(&raw) {
                db.default_password = v["signup_password"].as_str().unwrap_or("").into();
                db.first_name = v["first_name"].as_str().unwrap_or("").into();
                db.last_name = v["last_name"].as_str().unwrap_or("").into();
            }
        }
        if let Ok(raw) = std::fs::read(dir.join("mailboxes.txt")) {
            if let Ok(text) = store::decode_text(&raw) {
                let _ = store::import(&mut db, &text, None);
            }
        }
    }
    store::save(root, &db)
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_local_data_dir()?;
            seed(&root).map_err(std::io::Error::other)?;
            app.manage(AppState {
                root,
                store_lock: Mutex::new(()),
                session: Mutex::new(None),
            });
            let handle = app.handle().clone();
            if let Some(w) = app.get_webview_window("main") {
                w.on_window_event(move |e| {
                    if matches!(e, tauri::WindowEvent::Destroyed) {
                        handle.exit(0);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            import_text,
            import_file,
            credential,
            start_session,
            fetch_mail,
            retry_code,
            fill_code,
            stop_session,
            mark_result,
            open_terms,
            export_accounts,
            save_profile,
            account_text
        ])
        .run(tauri::generate_context!())
        .expect("无法运行 Grok 注册助手");
}
