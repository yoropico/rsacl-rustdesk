//! rsacl: RS Automation's in-house remote-assistance build of RustDesk (`rsacl-ra.exe`).
//!
//! Everything the fork adds lives here, so upstream merges touch only a few call sites:
//! - `apply_settings` fills RustDesk's own settings maps at start (the maps a signed custom client fills in
//!   `common.rs::read_custom_client_advanced_settings`, without the signature check): our server and key,
//!   click-only approval with no password, the permitted features, and hidden settings pages.
//! - `no_inbound` keeps the controlled side from listening for peers (TCP/UDP/IPv6 punch, WebRTC, direct
//!   access), so Windows Defender Firewall never asks about the exe and every session goes through the relay.
//! - In the employee's portable run (`enter`), a worker reports the registered RustDesk ID to acl and keeps the
//!   state the Flutter `RsaclPage` shows (Main / Connected artboards of the 1안 design); `get_common` and
//!   `set_common` carry that state over the existing `main_get_common` / `main_set_common` bridge calls.

pub mod acl;

pub use acl::{valid_id, Reply, RsaclError};
use hbb_common::config;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use hbb_common::{log, serde_json};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::path::PathBuf;

/// The in-house RustDesk server (infra-cmdb `sys/RUSTDESK`), rendezvous and relay.
pub const SERVER: &str = "rd.rsautomation.co.kr";
/// The server's public key (a public value; the server enforces it with `-k _`).
pub const KEY: &str = "SdU39Df9+bDXWTK2uzsifq9hU6MEqJqnaAPQP7Mmo4s=";

/// Override settings: the user cannot change these (rs-acl plan, Global Constraints).
pub const OVERRIDE_SETTINGS: &[(&str, &str)] = &[
    ("custom-rendezvous-server", SERVER),
    ("relay-server", SERVER),
    ("key", KEY),
    // No password: only the accept click lets a helper in.
    ("approve-mode", "click"),
    ("verification-method", "use-permanent-password"),
    // Screen, keyboard and mouse, file transfer and clipboard only.
    ("enable-keyboard", "Y"),
    ("enable-file-transfer", "Y"),
    ("enable-clipboard", "Y"),
    ("enable-file-copy-paste", "Y"),
    ("enable-audio", "N"),
    ("enable-tunnel", "N"),
    ("enable-terminal", "N"),
    ("enable-remote-restart", "N"),
    ("enable-remote-printer", "N"),
    ("enable-camera", "N"),
    ("enable-privacy-mode", "N"),
    ("enable-block-input", "N"),
    ("enable-record-session", "N"),
    ("enable-lan-discovery", "N"),
    // No listening socket (see `no_inbound`): direct IP access stays off.
    ("direct-server", "N"),
];

/// Built-in settings: settings pages the employee must not see, so a removed feature cannot come back.
pub const BUILTIN_SETTINGS: &[(&str, &str)] = &[
    ("hide-security-settings", "Y"),
    ("hide-network-settings", "Y"),
    ("hide-server-settings", "Y"),
    ("hide-proxy-settings", "Y"),
    ("hide-websocket-settings", "Y"),
    ("hide-remote-printer-settings", "Y"),
    ("hide-help-cards", "Y"),
    ("hide-powered-by-me", "Y"),
    ("hide-recording-button", "Y"),
    // The accept window keeps 수락 and 취소. "수락 및 권한 상승" asks the employee, a standard user, for an
    // administrator's password in a UAC prompt the helper cannot see; it only confuses.
    ("hide-elevate-button-in-accept-window", "Y"),
];

/// Hard settings: `disable-tcp-listen` makes the controlled side relay instead of listening for a TCP punch.
pub const HARD_SETTINGS: &[(&str, &str)] = &[("disable-tcp-listen", "Y")];

/// Fill RustDesk's settings maps. Called at the end of `load_custom_client`, in every process of the exe
/// (main window, connection manager, installed service), so they all agree.
pub fn apply_settings() {
    {
        let mut m = config::OVERWRITE_SETTINGS.write().unwrap();
        for (k, v) in OVERRIDE_SETTINGS {
            m.insert((*k).to_owned(), (*v).to_owned());
        }
    }
    {
        let mut m = config::BUILTIN_SETTINGS.write().unwrap();
        for (k, v) in BUILTIN_SETTINGS {
            m.insert((*k).to_owned(), (*v).to_owned());
        }
    }
    let mut m = config::HARD_SETTINGS.write().unwrap();
    for (k, v) in HARD_SETTINGS {
        m.insert((*k).to_owned(), (*v).to_owned());
    }
}

/// The controlled side never waits for an inbound peer: punch requests (TCP, UDP, IPv6), LAN fetches and
/// WebRTC offers are all answered through the relay. A listening socket is what made Windows Defender
/// Firewall ask about `rustdesk.exe` on both test PCs, and on RS-OP-KPMG its alert hid the accept window.
#[inline]
pub fn no_inbound() -> bool {
    true
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The PC key (first of `%ProgramFiles%\rsacl-ra\key`, `%LOCALAPPDATA%\rsacl-ra\key`, else a new one written
/// to the latter). Empty when no key can be read or written; the caller reports that.
pub fn pc_key() -> String {
    pc_key_result().unwrap_or_default()
}

fn pc_key_result() -> std::io::Result<String> {
    let local = env_dir("LOCALAPPDATA").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "LOCALAPPDATA is not set")
    })?;
    acl::pc_key_in(env_dir("ProgramFiles").as_deref(), &local)
}

/// The PC name acl registers: `COMPUTERNAME`.
pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// `POST ra/request` for this PC with RustDesk ID `id`.
pub fn request(id: &str) -> Result<Reply, RsaclError> {
    acl::request_at(&acl::base_url(), &pc_key(), &pc_name(), id)
}

/// `POST ra/withdraw` for this PC. Errors are logged only: the request also expires on its own.
pub fn withdraw() {
    withdraw_within(acl::TIMEOUT);
}

fn withdraw_within(timeout: std::time::Duration) {
    let key = pc_key();
    if key.is_empty() {
        return;
    }
    if let Err(e) = acl::withdraw_at(&acl::base_url(), &key, timeout) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        log::warn!("rsacl: withdraw failed: {e:?}");
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let _ = e;
    }
}

/// The employee-facing text for a failed request, in the wording of the retired `ra/rsacl-ra.ps1`.
pub fn error_text(e: &RsaclError, pc: &str) -> String {
    match e {
        RsaclError::Offline => format!(
            "회사 서버에 연결하지 못했습니다.\n\n회사 네트워크나 VPN에 연결되어 있는지 확인한 뒤 [원격 지원 요청]을 다시 누르세요. \
             계속 안 되면 전산 담당자에게 이 창을 보여 주세요.\n\n{pc} · 서버 응답 없음"
        ),
        RsaclError::Refused => format!(
            "이 PC는 원격 지원을 요청할 수 없습니다.\n\n등록된 정보와 맞지 않습니다(IP가 바뀐 경우 등). \
             전산 담당자에게 이 창을 보여 주세요.\n\n{pc} · 요청 거절"
        ),
        RsaclError::Server(code) => format!(
            "원격 지원 요청을 보내지 못했습니다.\n\n잠시 뒤 [원격 지원 요청]을 다시 누르세요. \
             계속 안 되면 전산 담당자에게 이 창을 보여 주세요.\n\n{pc} · 서버 오류 {code}"
        ),
    }
}

fn other_error_text(message: &str) -> String {
    format!(
        "원격 지원 요청을 보내지 못했습니다.\n\n{message}\n\n계속 안 되면 전산 담당자에게 이 창을 보여 주세요."
    )
}

// ---------------------------------------------------------------- employee window (desktop)

/// What the employee window shows.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// "번호를 받는 중…": the ID is not registered yet, or acl has not answered.
    Wait,
    /// Main artboard: 접수 번호, waiting for a helper.
    Ready,
    /// Connected artboard: a helper is connected.
    Connected,
    /// No number: the request failed.
    Error,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone)]
pub struct State {
    pub phase: Phase,
    pub number: String,
    pub until: String,
    pub target: String,
    pub helper: String,
    pub error: String,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl State {
    fn to_json(&self) -> String {
        let phase = match self.phase {
            Phase::Wait => "wait",
            Phase::Ready => "ready",
            Phase::Connected => "connected",
            Phase::Error => "error",
        };
        serde_json::json!({
            "phase": phase,
            "number": self.number,
            "until": self.until,
            "target": self.target,
            "helper": self.helper,
            "error": self.error,
        })
        .to_string()
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
lazy_static::lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State {
        phase: Phase::Wait,
        number: String::new(),
        until: String::new(),
        target: pc_name(),
        helper: String::new(),
        error: String::new(),
    });
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Whether the employee asked to end support (connection.rs then closes sessions with a plain reason).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn is_ending() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Whether this process is the employee's portable run, which shows `RsaclPage` instead of RustDesk's main
/// window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// Called by `core_main` for a start with no arguments. Returns false when this run must end at once
/// because the user's first run is still open (its window was brought forward and nothing was sent).
/// A start of the installed copy (a helper's PC) keeps RustDesk's own window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn enter() -> bool {
    #[cfg(windows)]
    {
        if crate::platform::is_cur_exe_the_installed() {
            return true;
        }
        if !win::single_instance() {
            win::focus_first();
            return false;
        }
        ACTIVE.store(true, Ordering::SeqCst);
        std::thread::spawn(worker);
        win::watch_session_end();
    }
    true
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn set_state(f: impl FnOnce(&mut State)) {
    f(&mut STATE.lock().unwrap());
}

/// The helpers connected now, by the name their RustDesk sent (shown in the accept window too).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn connected_helpers() -> Vec<String> {
    let mut names: Vec<String> = crate::server::AUTHED_CONNS
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.session_key.name().to_owned())
        .collect();
    names.dedup();
    names
}

/// The ID registered on our server, or None while it is not (yet) registered.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn registered_id() -> Option<String> {
    if crate::ui_interface::get_connect_status().status_num != 1 {
        return None;
    }
    let id: String = crate::ipc::get_id();
    valid_id(&id).then_some(id)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn worker() {
    use std::time::Duration;
    crate::ui_interface::start_option_status_sync();
    let pc = pc_name();
    let mut reported: Option<String> = None;
    loop {
        if QUITTING.load(Ordering::SeqCst) {
            return;
        }
        let helpers = connected_helpers();
        if !helpers.is_empty() {
            set_state(|s| {
                if s.phase == Phase::Ready || s.phase == Phase::Connected {
                    s.phase = Phase::Connected;
                    s.helper = helpers.join(", ");
                }
            });
        } else {
            set_state(|s| {
                if s.phase == Phase::Connected {
                    s.phase = Phase::Ready;
                    s.helper.clear();
                }
            });
        }
        // Report only a registered ID, once, and again only if the server gave the PC another one.
        if let Some(id) = registered_id() {
            if reported.as_deref() != Some(id.as_str()) && STATE.lock().unwrap().phase != Phase::Error {
                reported = Some(id.clone());
                if pc_key().is_empty() {
                    let msg = pc_key_result()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    set_state(|s| {
                        s.phase = Phase::Error;
                        s.error = other_error_text(&msg);
                    });
                    continue;
                }
                match request(&id) {
                    Ok(r) => {
                        log::info!("rsacl: request {} open for {}", r.number, r.target);
                        let until = acl::until_kst(hbb_common::chrono::Utc::now(), r.expires_in_minutes);
                        set_state(|s| {
                            if s.phase != Phase::Connected {
                                s.phase = Phase::Ready;
                            }
                            s.number = r.number;
                            s.until = until;
                            s.target = r.target;
                        });
                    }
                    Err(e) => {
                        log::warn!("rsacl: request failed: {e:?}");
                        set_state(|s| {
                            s.phase = Phase::Error;
                            s.error = error_text(&e, &pc);
                        });
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 닫기 and 지원 끝내기, step one: withdraw the request, end every session and close the connection manager.
/// Runs on the caller's thread and returns, so the page can then close its window and the process ends the
/// normal way (the first build called `exit` from a background thread, and on RS-OP-KPMG the process stayed
/// with the request open). A watchdog terminates the process 10 s after the request whatever happens.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn end_support() {
    use std::time::Duration;
    if QUITTING.swap(true, Ordering::SeqCst) {
        log::info!("rsacl: end requested again");
        return;
    }
    log::info!("rsacl: end requested");
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(10));
        log::warn!("rsacl: still running 10 s after the end request, terminating");
        terminate();
    });
    withdraw_within(Duration::from_secs(3));
    log::info!("rsacl: withdraw done");
    let conns: Vec<_> = crate::server::AUTHED_CONNS
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.sender.clone())
        .collect();
    let n = conns.len();
    for tx in conns {
        tx.send(crate::ipc::Data::Close).ok();
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut children = std::mem::take(&mut *crate::server::CHILD_PROCESS.lock().unwrap());
    for c in children.iter_mut() {
        c.kill().ok();
    }
    log::info!(
        "rsacl: {n} session(s) closed, {} child process(es) ended",
        children.len()
    );
}

/// End this process at once, without running exit handlers.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn terminate() -> ! {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    std::process::exit(0)
}

/// `main_get_common` keys: `rsacl-mode` ("Y" in the employee's run), `rsacl-state` (JSON for RsaclPage) and
/// `rsacl-quit` (runs `end_support`, answers "ok" when done; the page then closes its window).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn get_common(key: &str) -> Option<String> {
    match key {
        "rsacl-mode" => Some(if is_active() { "Y" } else { "N" }.to_owned()),
        "rsacl-state" => Some(STATE.lock().unwrap().to_json()),
        "rsacl-quit" => {
            if is_active() {
                end_support();
            }
            Some("ok".to_owned())
        }
        _ => None,
    }
}

/// `main_set_common` key `rsacl-quit`: the same end without an answer, then the process is terminated.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn set_common(key: &str, _value: &str) -> bool {
    match key {
        "rsacl-quit" => {
            if is_active() {
                std::thread::spawn(|| {
                    end_support();
                    terminate();
                });
            }
            true
        }
        _ => false,
    }
}

#[cfg(windows)]
mod win {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
    use winapi::{
        shared::{
            minwindef::{LPARAM, LRESULT, TRUE, UINT, WPARAM},
            windef::HWND,
        },
        um::{synchapi::CreateMutexW, winuser::*},
    };

    /// The title `RsaclPage` gives its window; the second run finds the first by it.
    pub const WINDOW_TITLE: &str = "원격 지원 요청";
    const FLUTTER_WINDOW_CLASS: &str = "FLUTTER_RUNNER_WIN32_WINDOW";
    const ERROR_ALREADY_EXISTS: i32 = 183;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// One run per user session: a session-local named mutex, held for the life of the process.
    pub fn single_instance() -> bool {
        let name = wide("Local\\rsacl-ra-employee-window");
        let h = unsafe { CreateMutexW(null_mut(), TRUE, name.as_ptr()) };
        let existed = std::io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS);
        !h.is_null() && !existed
    }

    /// Bring the first run's window forward (this process was started by the user, so it may).
    pub fn focus_first() {
        let class = wide(FLUTTER_WINDOW_CLASS);
        let title = wide(WINDOW_TITLE);
        unsafe {
            let hwnd = FindWindowW(class.as_ptr(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, if IsIconic(hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
                SetForegroundWindow(hwnd);
            }
        }
    }

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_QUERYENDSESSION => TRUE as LRESULT,
            WM_ENDSESSION => {
                // Logoff or shutdown: the process ends without a window close, so withdraw here. Windows
                // waits only a few seconds, hence the short timeout.
                if wparam != 0 {
                    super::withdraw_within(std::time::Duration::from_secs(3));
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    /// A hidden top-level window (message-only windows get no WM_ENDSESSION) on its own thread.
    pub fn watch_session_end() {
        std::thread::spawn(|| unsafe {
            let class = wide("rsacl-ra-session-watch");
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wnd_proc);
            wc.lpszClassName = class.as_ptr();
            if RegisterClassW(&wc) == 0 {
                return;
            }
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            if hwnd.is_null() {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::{config::Config, password_security};

    #[test]
    fn settings_have_every_option() {
        apply_settings();
        for (k, v) in OVERRIDE_SETTINGS {
            assert_eq!(Config::get_option(k), *v, "{k}");
        }
        assert_eq!(Config::get_option("custom-rendezvous-server"), "rd.rsautomation.co.kr");
        assert_eq!(Config::get_option("relay-server"), "rd.rsautomation.co.kr");
        assert_eq!(Config::get_option("key"), "SdU39Df9+bDXWTK2uzsifq9hU6MEqJqnaAPQP7Mmo4s=");
        for k in [
            "enable-keyboard",
            "enable-file-transfer",
            "enable-clipboard",
            "enable-file-copy-paste",
        ] {
            assert_eq!(Config::get_option(k), "Y", "{k}");
        }
        for k in [
            "enable-audio",
            "enable-tunnel",
            "enable-terminal",
            "enable-remote-restart",
            "enable-remote-printer",
            "enable-camera",
            "enable-privacy-mode",
            "enable-block-input",
            "enable-record-session",
            "enable-lan-discovery",
            "direct-server",
        ] {
            assert_eq!(Config::get_option(k), "N", "{k}");
        }
        for k in [
            "hide-security-settings",
            "hide-network-settings",
            "hide-server-settings",
            "hide-proxy-settings",
            "hide-websocket-settings",
            "hide-remote-printer-settings",
            "hide-help-cards",
            "hide-powered-by-me",
            "hide-recording-button",
            "hide-elevate-button-in-accept-window",
        ] {
            assert_eq!(crate::common::get_builtin_option(k), "Y", "{k}");
        }
        assert!(config::is_disable_tcp_listen());
        // The user cannot change an override setting.
        Config::set_option("approve-mode".to_owned(), "password".to_owned());
        assert_eq!(Config::get_option("approve-mode"), "click");
    }

    #[test]
    fn no_password_mode() {
        apply_settings();
        assert_eq!(password_security::approve_mode(), password_security::ApproveMode::Click);
        assert!(!password_security::temporary_enabled());
    }

    #[test]
    fn rendezvous_server_is_ours() {
        apply_settings();
        assert_eq!(Config::get_rendezvous_server(), "rd.rsautomation.co.kr:21116");
        assert!(no_inbound());
    }

    #[test]
    fn error_texts_name_the_pc() {
        let t = error_text(&RsaclError::Refused, "RS-OP-KPMG");
        assert!(t.starts_with("이 PC는 원격 지원을 요청할 수 없습니다."));
        assert!(t.ends_with("RS-OP-KPMG · 요청 거절"));
        assert!(error_text(&RsaclError::Server(429), "PC").ends_with("PC · 서버 오류 429"));
        assert!(error_text(&RsaclError::Offline, "PC").ends_with("PC · 서버 응답 없음"));
    }
}
