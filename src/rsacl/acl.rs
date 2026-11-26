//! The acl side of the employee's request: the PC key, `ra/request` and `ra/withdraw`.
//!
//! Plain functions over paths and a base URL, so the unit tests run them against a stub server and
//! temporary directories; `super` binds them to the real `%ProgramFiles%`, `%LOCALAPPDATA%` and acl.

use hbb_common::{chrono, rand, serde_json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

/// The acl gateway. `RSACL_RA_BASE` replaces it (tests only).
pub const DEFAULT_BASE: &str = "https://acl.rsautomation.co.kr";
const API: &str = "/api/ext/rsacl-guard/ra/";
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The key file under `%ProgramFiles%` (old IT install) and `%LOCALAPPDATA%` (old per-user install, and ours).
const KEY_DIR: &str = "rsacl-ra";
const KEY_FILE: &str = "key";

/// acl's answer to `ra/request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The PC name acl holds for this key.
    pub target: String,
    /// The 4-digit 접수 번호.
    pub number: String,
    pub expires_in_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RsaclError {
    /// No answer from acl (no network, DNS, TLS, timeout).
    Offline,
    /// 403: unknown key, or the PC's address does not match its registration.
    Refused,
    /// Any other answer that is not a success (400 malformed, 429 too many, 5xx).
    Server(u16),
}

pub fn base_url() -> String {
    std::env::var("RSACL_RA_BASE")
        .ok()
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_owned())
        .trim()
        .trim_end_matches('/')
        .to_owned()
}

/// A RustDesk ID as the server registers it: 9 or 10 digits, nothing else.
pub fn valid_id(id: &str) -> bool {
    (9..=10).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

pub fn request_body(key: &str, name: &str, rustdesk_id: &str) -> String {
    serde_json::json!({ "key": key, "name": name, "rustdeskId": rustdesk_id }).to_string()
}

pub fn withdraw_body(key: &str) -> String {
    serde_json::json!({ "key": key }).to_string()
}

pub fn parse_reply(body: &str) -> Option<Reply> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let number = v.get("number")?.as_str()?.to_owned();
    if number.len() != 4 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Reply {
        target: v.get("target")?.as_str()?.to_owned(),
        number,
        expires_in_minutes: u32::try_from(v.get("expiresInMinutes")?.as_u64()?).ok()?,
    })
}

fn post(base: &str, path: &str, body: String, timeout: Duration) -> Result<(u16, String), RsaclError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|_| RsaclError::Offline)?;
    let resp = client
        .post(format!("{base}{API}{path}"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|_| RsaclError::Offline)?;
    let status = resp.status().as_u16();
    Ok((status, resp.text().unwrap_or_default()))
}

fn check(status: u16) -> Result<(), RsaclError> {
    match status {
        200..=299 => Ok(()),
        403 => Err(RsaclError::Refused),
        s => Err(RsaclError::Server(s)),
    }
}

/// `POST ra/request {key, name, rustdeskId}`: opens (or replaces) this PC's request.
pub fn request_at(base: &str, key: &str, name: &str, id: &str) -> Result<Reply, RsaclError> {
    let (status, body) = post(base, "request", request_body(key, name, id), TIMEOUT)?;
    check(status)?;
    parse_reply(&body).ok_or(RsaclError::Server(status))
}

/// `POST ra/withdraw {key}`: closes this PC's open request. `timeout` is short at logoff, where Windows
/// waits only a few seconds for the process.
pub fn withdraw_at(base: &str, key: &str, timeout: Duration) -> Result<(), RsaclError> {
    let (status, _) = post(base, "withdraw", withdraw_body(key), timeout)?;
    check(status)
}

fn read_key(dir: &Path) -> Option<String> {
    let k = fs::read_to_string(dir.join(KEY_DIR).join(KEY_FILE)).ok()?;
    let k = k.trim();
    (!k.is_empty()).then(|| k.to_owned())
}

pub fn new_key() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The PC key: the first of `<program_files>\rsacl-ra\key` and `<local_app_data>\rsacl-ra\key`, else a new
/// 32-byte hex key written to the latter, so a PC registered by an old install keeps its registration.
pub fn pc_key_in(program_files: Option<&Path>, local_app_data: &Path) -> io::Result<String> {
    if let Some(k) = program_files.and_then(read_key) {
        return Ok(k);
    }
    if let Some(k) = read_key(local_app_data) {
        return Ok(k);
    }
    let dir: PathBuf = local_app_data.join(KEY_DIR);
    fs::create_dir_all(&dir)?;
    let k = new_key();
    fs::write(dir.join(KEY_FILE), &k)?;
    Ok(k)
}

/// `HH:mm` in KST, `minutes` after `now`: the footer's "<HH:mm>까지 유지".
pub fn until_kst(now: chrono::DateTime<chrono::Utc>, minutes: u32) -> String {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("KST offset");
    (now + chrono::Duration::minutes(minutes as i64))
        .with_timezone(&kst)
        .format("%H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    /// One-shot HTTP stub: answers the first request with `status` and `body`, hands back the request.
    fn stub(status: u16, body: &'static str) -> (String, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len || n == 0 {
                        tx.send(text).ok();
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(reply.as_bytes()).ok();
        });
        (base, rx)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rsacl-test-{name}-{}", new_key()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put_key(dir: &Path, k: &str) {
        fs::create_dir_all(dir.join(KEY_DIR)).unwrap();
        fs::write(dir.join(KEY_DIR).join(KEY_FILE), k).unwrap();
    }

    #[test]
    fn valid_id() {
        assert!(super::valid_id("123456789"));
        assert!(super::valid_id("1234567890"));
        assert!(!super::valid_id(""));
        assert!(!super::valid_id("12345678"));
        assert!(!super::valid_id("12345678901"));
        assert!(!super::valid_id("12 3456789"));
        assert!(!super::valid_id("abc123456"));
    }

    #[test]
    fn request_body_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&request_body("k".repeat(64).as_str(), "RS-OP-KPMG", "490209537"))
                .unwrap();
        let o = v.as_object().unwrap();
        let mut keys: Vec<_> = o.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["key", "name", "rustdeskId"]);
        assert_eq!(o["name"], "RS-OP-KPMG");
        assert_eq!(o["rustdeskId"], "490209537");
        let w: serde_json::Value = serde_json::from_str(&withdraw_body("abc")).unwrap();
        assert_eq!(w.as_object().unwrap().len(), 1);
        assert_eq!(w["key"], "abc");
    }

    #[test]
    fn existing_key_order() {
        let (pf, lad) = (tmp("pf"), tmp("lad"));
        // Nothing yet: a new 64-hex key, written under LocalAppData.
        let k = pc_key_in(Some(&pf), &lad).unwrap();
        assert_eq!(k.len(), 64);
        assert!(k.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(read_key(&lad).as_deref(), Some(k.as_str()));
        assert_eq!(pc_key_in(Some(&pf), &lad).unwrap(), k, "the written key is reused");
        // LocalAppData wins over a new key; ProgramFiles wins over LocalAppData.
        put_key(&lad, "local-key\r\n");
        assert_eq!(pc_key_in(Some(&pf), &lad).unwrap(), "local-key");
        put_key(&pf, "machine-key");
        assert_eq!(pc_key_in(Some(&pf), &lad).unwrap(), "machine-key");
        assert_eq!(pc_key_in(None, &lad).unwrap(), "local-key");
        fs::remove_dir_all(pf).ok();
        fs::remove_dir_all(lad).ok();
    }

    #[test]
    fn request_ok() {
        let (base, rx) = stub(200, r#"{"target":"RS-OP-KPMG","number":"4821","expiresInMinutes":120}"#);
        let r = request_at(&base, "kkkk", "RS-OP-KPMG", "490209537").unwrap();
        assert_eq!(
            r,
            Reply { target: "RS-OP-KPMG".into(), number: "4821".into(), expires_in_minutes: 120 }
        );
        let seen = rx.recv().unwrap();
        assert!(seen.starts_with("POST /api/ext/rsacl-guard/ra/request "), "{seen}");
        assert!(seen.contains(r#""rustdeskId":"490209537""#), "{seen}");
    }

    #[test]
    fn request_errors() {
        let (base, _rx) = stub(403, r#"{"message":"rs-acl: request refused"}"#);
        assert_eq!(request_at(&base, "k", "n", "123456789"), Err(RsaclError::Refused));
        let (base, _rx) = stub(429, "{}");
        assert_eq!(request_at(&base, "k", "n", "123456789"), Err(RsaclError::Server(429)));
        let (base, _rx) = stub(400, "{}");
        assert_eq!(request_at(&base, "k", "n", "123456789"), Err(RsaclError::Server(400)));
        let (base, _rx) = stub(200, r#"{"target":"x","number":"12","expiresInMinutes":120}"#);
        assert_eq!(request_at(&base, "k", "n", "123456789"), Err(RsaclError::Server(200)));
        // A port nobody listens on: no answer at all.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        assert_eq!(request_at(&dead, "k", "n", "123456789"), Err(RsaclError::Offline));
    }

    #[test]
    fn withdraw_posts_key() {
        let (base, rx) = stub(204, "");
        assert_eq!(withdraw_at(&base, "the-key", TIMEOUT), Ok(()));
        let seen = rx.recv().unwrap();
        assert!(seen.starts_with("POST /api/ext/rsacl-guard/ra/withdraw "), "{seen}");
        assert!(seen.ends_with(r#"{"key":"the-key"}"#), "{seen}");
    }

    #[test]
    fn base_from_env() {
        std::env::set_var("RSACL_RA_BASE", "http://127.0.0.1:9/ ");
        assert_eq!(base_url(), "http://127.0.0.1:9");
        std::env::remove_var("RSACL_RA_BASE");
        assert_eq!(base_url(), DEFAULT_BASE);
    }

    #[test]
    fn until_is_kst() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-26T03:40:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(until_kst(now, 120), "14:40");
    }
}
