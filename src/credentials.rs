use crate::{Result, Session};
use serde::{Deserialize, Serialize};

// Only this payload enters the system keystore, never TOML or log output.
// No Debug: even malformed credential errors must not expose the token.
#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    pub token: String,
    pub server_guid: Option<String>,
    pub user_guid: String,
}

impl SavedSession {
    pub fn from_session(session: &Session, server_guid: Option<String>) -> Self {
        let user = session.user.get("user").unwrap_or(&session.user);
        Self {
            token: session.token.clone(),
            server_guid,
            user_guid: user["guid"].as_str().unwrap_or_default().to_owned(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        crate::cookie(false, Some(&self.token))?;
        if self.user_guid.is_empty() {
            return Err("保存的登录信息格式不正确，请重新登录".into());
        }
        Ok(())
    }
}

pub trait TokenStore {
    fn load(&self) -> Result<Option<SavedSession>>;
    fn save(&self, session: &SavedSession) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub fn account_key(target: &str, username: &str) -> String {
    let target = target.trim();
    let identity = if target.contains("://") {
        // The connection input may retain an explicit :80/:443. Keep the
        // canonical address as the identity for existing local data.
        let parsed = crate::connection::Target::parse(target).ok();
        let target = parsed.as_ref().map_or(target, |address| address.as_str());
        serde_json::json!(["manual", target, username.trim()])
    } else {
        serde_json::json!([target.to_ascii_lowercase(), username.trim()])
    };
    crate::hash_password(&identity.to_string())
}

pub struct KeyringStore {
    entry: keyring::Entry,
}
impl KeyringStore {
    pub fn new(target: &str, username: &str) -> Result<Self> {
        let entry = keyring::Entry::new("termune.music-session", &account_key(target, username))
            .map_err(|_| "无法打开系统凭据存储".to_owned())?;
        Ok(Self { entry })
    }
}
impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<SavedSession>> {
        let payload = match self.entry.get_password() {
            Ok(payload) => payload,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(_) => return Err("系统凭据存储不可用或已锁定，请解锁后重试，或输入密码登录".into()),
        };
        let saved: SavedSession =
            serde_json::from_str(&payload).map_err(|_| "保存的登录信息损坏，请重新登录")?;
        saved.validate()?;
        Ok(Some(saved))
    }
    fn save(&self, saved: &SavedSession) -> Result<()> {
        saved.validate()?;
        let payload = serde_json::to_string(saved).map_err(|_| "登录信息编码失败")?;
        self.entry.set_password(&payload).map_err(|_| {
            "系统凭据存储不可用或已锁定，无法保存自动登录；本次仍可使用，未明文保存 Token".into()
        })
    }
    fn clear(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("清除系统凭据失败，请解锁密钥环后重试；不会明文保存凭据".into()),
        }
    }
}

pub fn persist(
    store: &dyn TokenStore,
    session: &Session,
    server_guid: Option<String>,
    enabled: bool,
) -> Result<()> {
    if enabled {
        store.save(&SavedSession::from_session(session, server_guid))
    } else {
        store.clear()
    }
}

pub fn restore_saved(
    store: &dyn TokenStore,
    candidate: &crate::Candidate,
    client: &reqwest::blocking::Client,
    saved: SavedSession,
    server_guid: Option<&str>,
) -> Result<Session> {
    if saved
        .server_guid
        .as_deref()
        .is_some_and(|expected| Some(expected) != server_guid)
    {
        let clear = store.clear().err().unwrap_or_default();
        return Err(format!(
            "NAS 身份与保存的登录状态不一致，请重新输入密码。{clear}"
        ));
    }
    let authenticated = match candidate.restore(client, saved.token) {
        Ok(session) => session,
        Err(error) => {
            if crate::is_auth_error(&error) {
                let clear = store.clear().err().unwrap_or_default();
                return Err(format!("自动登录已失效，请重新输入密码。{clear}"));
            }
            // Network, server and protocol failures are not proof of expiry.
            return Err(error);
        }
    };
    let user = authenticated
        .user
        .get("user")
        .unwrap_or(&authenticated.user);
    if user["guid"].as_str() != Some(saved.user_guid.as_str()) {
        let clear = store.clear().err().unwrap_or_default();
        return Err(format!("登录状态的账号身份不匹配，请重新输入密码。{clear}"));
    }
    Ok(authenticated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Memory(RefCell<Option<SavedSession>>);
    impl TokenStore for Memory {
        fn load(&self) -> Result<Option<SavedSession>> {
            Ok(self.0.borrow().as_ref().map(|s| SavedSession {
                token: s.token.clone(),
                server_guid: s.server_guid.clone(),
                user_guid: s.user_guid.clone(),
            }))
        }
        fn save(&self, s: &SavedSession) -> Result<()> {
            self.0.replace(Some(SavedSession {
                token: s.token.clone(),
                server_guid: s.server_guid.clone(),
                user_guid: s.user_guid.clone(),
            }));
            Ok(())
        }
        fn clear(&self) -> Result<()> {
            self.0.replace(None);
            Ok(())
        }
    }
    #[test]
    fn opt_in_persists_token_only_and_opt_out_deletes_it() {
        let session = Session {
            candidate: crate::Candidate::new("https://example.com".parse().unwrap(), true, "test"),
            token: "test-token".into(),
            user: serde_json::json!({"guid":"user-id","name":"tester"}),
        };
        let store = Memory(RefCell::new(None));
        persist(&store, &session, Some("nas-id".into()), true).unwrap();
        let saved = store.load().unwrap().unwrap();
        assert_eq!(saved.token, "test-token");
        assert_eq!(saved.user_guid, "user-id");
        persist(&store, &session, None, false).unwrap();
        assert!(store.load().unwrap().is_none());
    }
    #[test]
    fn credential_keys_isolate_accounts_and_devices() {
        assert_eq!(account_key(" NAS ", "tester"), account_key("nas", "tester"));
        assert_ne!(account_key("nas", "tester"), account_key("other", "tester"));
        assert_ne!(account_key("nas", "tester"), account_key("nas", "other"));
    }

    fn saved() -> SavedSession {
        SavedSession {
            token: "test-token".into(),
            server_guid: Some("nas-id".into()),
            user_guid: "user-id".into(),
        }
    }
    #[test]
    fn restore_validates_token_and_clears_only_confirmed_invalid_sessions() {
        use std::io::{Read, Write};
        for (status, body, success, cleared) in [
            (
                "200 OK",
                r#"{"code":0,"data":{"name":"tester","guid":"user-id"}}"#,
                true,
                false,
            ),
            ("401 Unauthorized", r#"{"code":401}"#, false, true),
            (
                "500 Internal Server Error",
                "server unavailable",
                false,
                false,
            ),
            (
                "200 OK",
                r#"{"code":0,"data":{"name":"other","guid":"wrong-user"}}"#,
                false,
                true,
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let size = socket.read(&mut buffer).unwrap();
                    assert!(size > 0);
                    request.extend_from_slice(&buffer[..size]);
                }
                let text = String::from_utf8(request).unwrap();
                assert!(text.starts_with("GET /music/api/v1/user/me "));
                assert!(
                    text.to_ascii_lowercase()
                        .contains("cookie: mode=relay; music-token=test-token")
                );
                write!(socket,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            let store = Memory(RefCell::new(Some(saved())));
            let candidate = crate::Candidate::new(base, true, "test");
            let result = restore_saved(
                &store,
                &candidate,
                &crate::client(3).unwrap(),
                store.load().unwrap().unwrap(),
                Some("nas-id"),
            );
            assert_eq!(result.is_ok(), success);
            assert_eq!(store.load().unwrap().is_none(), cleared);
            server.join().unwrap();
        }
    }
    #[test]
    fn changed_nas_is_rejected_before_sending_token() {
        let store = Memory(RefCell::new(Some(saved())));
        let candidate =
            crate::Candidate::new("https://example.invalid".parse().unwrap(), true, "test");
        let result = restore_saved(
            &store,
            &candidate,
            &crate::client(3).unwrap(),
            store.load().unwrap().unwrap(),
            Some("other-nas"),
        );
        assert!(result.is_err());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn connection_failure_preserves_saved_credentials() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        drop(listener);
        let store = Memory(RefCell::new(Some(saved())));
        let candidate = crate::Candidate::new(base, false, "test");
        assert!(
            restore_saved(
                &store,
                &candidate,
                &crate::client(1).unwrap(),
                store.load().unwrap().unwrap(),
                Some("nas-id")
            )
            .is_err()
        );
        assert!(store.load().unwrap().is_some());
    }
}
