use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub mail_api_url: String,
    /// Outlook / Microsoft 365 四段格式：邮箱密码（仅备份用，不用作注册密码）。
    #[serde(default)]
    pub mail_password: String,
    #[serde(default)]
    pub mail_client_id: String,
    #[serde(default)]
    pub mail_refresh_token: String,
    #[serde(default = "new_status")]
    pub status: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub note: String,
}
fn new_status() -> String {
    "new".into()
}

#[derive(Default, Serialize, Deserialize)]
pub struct Database {
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
    #[serde(default)]
    pub default_password: String,
}

#[derive(Default, Serialize)]
pub struct ImportResult {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

pub fn load(root: &Path) -> Result<Database, String> {
    match fs::read(root.join("accounts.json")) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "账号数据格式损坏，请从备份恢复。".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Database::default()),
        Err(_) => Err("无法读取本地账号文件。".into()),
    }
}

pub fn save(root: &Path, db: &Database) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|_| "无法创建本地数据目录。")?;
    let temp = root.join(format!("accounts-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "无法写入本地账号文件。")?;
        let bytes = serde_json::to_vec_pretty(db).map_err(|_| "无法保存账号数据。")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "无法保存账号数据。")?;
        drop(f);
        fs::rename(&temp, root.join("accounts.json"))
            .map_err(|_| "无法更新账号文件，请检查文件是否被其他程序占用。")
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(str::to_owned)
}

pub fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("导入文件不能超过 1 MB。".into());
    }
    let raw = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err("UTF-16 文件内容不完整。".into());
        }
        let le = bytes[0] == 0xff;
        let words: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|b| {
                if le {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        return String::from_utf16(&words).map_err(|_| "无法识别 UTF-16 文件。".into());
    }
    if let Ok(text) = std::str::from_utf8(raw) {
        return Ok(text.to_owned());
    }
    let (text, _, malformed) = encoding_rs::GBK.decode(raw);
    if malformed {
        Err("无法识别文件编码，请保存为 UTF-8。".into())
    } else {
        Ok(text.into_owned())
    }
}

fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && !domain.is_empty()
        && domain.len() <= 255
        && domain.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && domain.bytes().last().is_some_and(|c| c.is_ascii_alphanumeric())
        && domain.bytes().all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        && domain.contains('.')
        && !domain.contains("..")
        && local
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && !local.contains("---")
}

fn guid_like(value: &str) -> bool {
    value.len() == 36
        && value.as_bytes()[8] == b'-'
        && value.as_bytes()[13] == b'-'
        && value.as_bytes()[18] == b'-'
        && value.as_bytes()[23] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_hexdigit() || (c == b'-' && matches!(i, 8 | 13 | 18 | 23)))
}

/// “邮箱----邮箱密码----client_id----refresh_token” 四段 Outlook / Graph 格式。
fn graph_line(line: &str) -> Option<(&str, &str, &str, &str)> {
    let parts: Vec<&str> = line.split("----").collect();
    if parts.len() != 4 {
        return None;
    }
    let email = parts[0].trim();
    let mail_password = parts[1].trim();
    let client_id = parts[2].trim();
    let refresh_token = parts[3].trim();
    if email.contains('@')
        && !mail_password.is_empty()
        && guid_like(client_id)
        && refresh_token.len() >= 20
        && !refresh_token.starts_with("http")
        && !refresh_token.contains(char::is_whitespace)
    {
        return Some((email, mail_password, client_id, refresh_token));
    }
    None
}

pub fn import(db: &mut Database, text: &str, password: Option<&str>) -> ImportResult {
    use std::sync::LazyLock;
    static PREFIX: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^卡密\s*\d+\s*[:：]\s*").unwrap());
    let mut result = ImportResult::default();
    if text.len() > 1024 * 1024 {
        result.errors.push("导入文本不能超过 1 MB。".into());
        return result;
    }
    let default = password
        .filter(|s| !s.is_empty())
        .unwrap_or(&db.default_password)
        .to_owned();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = PREFIX.replace(line, "");
        let error = |reason: &str| format!("第 {} 行：{reason}", index + 1);
        if let Some((email, mail_password, client_id, refresh_token)) = graph_line(&line) {
            let email = email.trim();
            let mail_password = mail_password.trim();
            let client_id = client_id.trim();
            // 有的邮箱文本用 “$$” 作为行尾标记，粘进来要裁掉。
            let refresh_token = refresh_token.trim().trim_end_matches('$').trim();
            if refresh_token.len() < 20 {
                result.errors.push(error("Outlook 四段格式的 refresh_token 不完整。"));
                continue;
            }
            if !valid_email(email) {
                result
                    .errors
                    .push(error("邮箱地址无效，请填写完整邮箱。"));
                continue;
            }
            if let Some(pos) = db
                .accounts
                .iter()
                .position(|a| a.email.eq_ignore_ascii_case(email))
            {
                let account = &mut db.accounts[pos];
                let changed = account.mail_client_id != client_id
                    || account.mail_refresh_token != refresh_token
                    || account.mail_password != mail_password
                    || !account.mail_api_url.is_empty();
                if changed {
                    account.mail_client_id = client_id.into();
                    account.mail_refresh_token = refresh_token.into();
                    account.mail_password = mail_password.into();
                    account.mail_api_url.clear();
                    result.updated += 1;
                } else {
                    result.skipped += 1;
                }
                continue;
            }
            if default.is_empty() {
                result
                    .errors
                    .push(error("请先填写统一注册密码，或先保存一个注册密码。"));
                continue;
            }
            if db.accounts.len() >= 500 {
                result.errors.push(error("本地邮箱库最多保存 500 条。"));
                break;
            }
            db.accounts.push(Account {
                email: email.into(),
                password: default.clone(),
                mail_api_url: String::new(),
                mail_password: mail_password.into(),
                mail_client_id: client_id.into(),
                mail_refresh_token: refresh_token.into(),
                status: new_status(),
                updated_at: String::new(),
                note: String::new(),
            });
            result.added += 1;
            continue;
        }
        let (email, rest, triple) = if let Some((email, rest)) = line.split_once("---") {
            if let Some(rest) = rest.strip_prefix('-') { (email, rest, false) }
            else { (email, rest, true) }
        } else {
            result.errors.push(error(
                "需要“邮箱----取信链接”或“邮箱---取信密钥---取信链接”格式。",
            ));
            continue;
        };
        let email = email.trim();
        if !valid_email(email) {
            result
                .errors
                .push(error("邮箱地址无效，请使用完整邮箱。"));
            continue;
        }
        let rest = rest.trim();
        let (explicit_password, uri) = if rest.starts_with("https://") {
            (None, rest)
        } else if triple {
            let Some((key, uri)) = rest.split_once("---https://") else {
                result.errors.push(error("取信链接格式不完整。"));
                continue;
            };
            let uri_start = rest.len() - uri.len() - 8;
            let uri = &rest[uri_start..];
            let matching = url::Url::parse(uri)
                .ok()
                .and_then(|u| u.fragment().map(str::to_owned))
                .is_some_and(|f| {
                    url::form_urlencoded::parse(f.as_bytes())
                        .any(|(k, v)| k == "key" && v == key.trim())
                });
            if !key.trim().starts_with("tok_") || !matching {
                result.errors.push(error("取信密钥与链接不匹配。"));
                continue;
            }
            (None, uri)
        } else if let Some((pw, uri)) = rest.split_once("----") {
            (Some(pw.trim()), uri.trim())
        } else {
            result.errors.push(error("取信链接格式不完整。"));
            continue;
        };
        if crate::mailbox::validate_url(uri, email).is_err() {
            result
                .errors
                .push(error("取信链接无效，或链接里的邮箱不匹配。"));
            continue;
        }
        let existing = db
            .accounts
            .iter_mut()
            .find(|a| a.email.eq_ignore_ascii_case(email));
        if let Some(account) = existing {
            let new_password = explicit_password
                .filter(|s| !s.is_empty())
                .unwrap_or(&account.password)
                .to_owned();
            if account.mail_api_url == uri && account.password == new_password {
                result.skipped += 1;
            } else {
                account.mail_api_url = uri.into();
                account.password = new_password;
                result.updated += 1;
            }
            continue;
        }
        let pw = explicit_password
            .filter(|s| !s.is_empty())
            .unwrap_or(&default);
        if pw.is_empty() {
            result
                .errors
                .push(error("请先填写统一注册密码，或使用三段格式指定密码。"));
            continue;
        }
        if db.accounts.len() >= 500 {
            result.errors.push(error("本地邮箱库最多保存 500 条。"));
            break;
        }
        db.accounts.push(Account {
            email: email.into(),
            password: pw.into(),
            mail_api_url: uri.into(),
            mail_password: String::new(),
            mail_client_id: String::new(),
            mail_refresh_token: String::new(),
            status: new_status(),
            updated_at: String::new(),
            note: String::new(),
        });
        result.added += 1;
    }
    if password.is_some_and(|s| !s.is_empty()) && result.added + result.updated > 0 {
        db.default_password = default;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outlook_graph_line_keeps_signup_password() {
        let mut db = Database {
            default_password: "fixture-password".into(),
            ..Default::default()
        };
        let line = "demo@outlook.com----MailPass123----9e5f94bc-e8a4-4e73-b8be-63364c29d753----M.C5xx_BAY.0.U.MsaArtifacts.-abcdefghijklmnopqrstuvwxyz0123456789$$";
        let r = import(&mut db, line, None);
        assert_eq!(r.added, 1, "{:?}", r.errors);
        assert!(r.errors.is_empty());
        let a = &db.accounts[0];
        assert_eq!(a.email, "demo@outlook.com");
        assert_eq!(a.password, "fixture-password");
        assert_eq!(a.mail_password, "MailPass123");
        assert_eq!(a.mail_client_id, "9e5f94bc-e8a4-4e73-b8be-63364c29d753");
        assert!(a.mail_refresh_token.starts_with("M.C5xx"));
        assert!(!a.mail_refresh_token.ends_with('$'));
        assert!(a.mail_api_url.is_empty());
        assert!(crate::mailbox::is_graph(a));
        assert_eq!(import(&mut db, line, None).skipped, 1);
        assert_eq!(db.accounts.len(), 1);
    }
    #[test]
    fn pickup_key_is_not_a_registration_password() {
        let mut db = Database {
            default_password: "fixture-password".into(),
            ..Default::default()
        };
        let line="demo@icloud.com---tok_fixture---https://icsms.top/pickup#email=demo%40icloud.com&key=tok_fixture";
        let r = import(&mut db, line, None);
        assert_eq!(r.added, 1);
        assert!(r.errors.is_empty());
        assert_eq!(db.accounts[0].password, "fixture-password");
        assert_eq!(import(&mut db, line, None).skipped, 1);
        assert_eq!(
            import(
                &mut db,
                &line.replace("---tok_fixture---", "---tok_other---"),
                None
            )
            .added,
            0
        );
    }
    const URI: &str =
        "https://mokeyun.icu/share/v1/latest?email=demo%40icloud.com&token=test----token";
    #[test]
    fn card_text_alias_and_dedup() {
        let mut db = Database {
            default_password: "fixture-only".into(),
            ..Default::default()
        };
        let line = format!("卡密 1: demo+tag@icloud.com----{URI}\u{a0}\n");
        let r = import(&mut db, &line, None);
        assert_eq!(r.added, 1);
        assert!(r.errors.is_empty());
        assert_eq!(db.accounts[0].mail_api_url, URI);
        assert_eq!(db.accounts[0].email, "demo+tag@icloud.com");
        let r = import(&mut db, &line, Some("changed-default"));
        assert_eq!(r.skipped, 1);
        assert_eq!(db.accounts[0].password, "fixture-only");
    }
    #[test]
    fn reports_bad_rows_without_secrets() {
        let mut db = Database::default();
        let r = import(
            &mut db,
            &format!("demo@icloud.com----{URI}\nbad----secret-value\n"),
            None,
        );
        assert_eq!(r.errors.len(), 2);
        assert_eq!(r.added, 0);
        assert!(!r.errors.join("").contains("secret-value"));
        assert!(!r.errors.join("").contains("test----token"));
    }
    #[test]
    fn three_parts_update_preserves_status() {
        let mut db = Database::default();
        import(&mut db, &format!("demo@icloud.com----first----{URI}"), None);
        db.accounts[0].status = "done".into();
        let r = import(
            &mut db,
            &format!("demo@icloud.com----second----{URI}"),
            None,
        );
        assert_eq!(r.updated, 1);
        assert_eq!(db.accounts[0].status, "done");
        assert_eq!(db.accounts[0].password, "second");
    }
    #[test]
    fn encodings() {
        assert_eq!(decode_text(b"\xef\xbb\xbfhello").unwrap(), "hello");
        assert_eq!(decode_text(&[0xff, 0xfe, b'h', 0, b'i', 0]).unwrap(), "hi");
        assert!(decode_text(&[0xff, 0xfe, 1]).is_err());
        let (bytes, _, _) = encoding_rs::GBK.encode("卡密 1:");
        assert_eq!(decode_text(&bytes).unwrap(), "卡密 1:");
    }
    #[test]
    fn corrupt_database_is_not_silently_replaced() {
        let root = std::env::temp_dir().join(format!("trae-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("accounts.json"), b"bad").unwrap();
        assert!(load(&root).is_err());
        let db = Database::default();
        save(&root, &db).unwrap();
        save(&root, &db).unwrap();
        assert!(load(&root).unwrap().accounts.is_empty());
        fs::remove_file(root.join("accounts.json")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}

#[cfg(test)]
mod pickup_compat_tests {
    use super::*;
    #[test]
    fn triple_dash_opaque_key_round_trip() {
        let line = "卡密 12： demo+tag@icloud.com---tok_fixture----opaque---https://icsms.top/pickup#email=demo%40icloud.com&key=tok_fixture----opaque";
        let mut db = Database { default_password: "fixture-password".into(), ..Default::default() };
        let result = import(&mut db, line, None);
        assert!(result.errors.is_empty());
        assert_eq!(result.added, 1);
        assert_eq!(db.accounts[0].email, "demo+tag@icloud.com");
        assert_eq!(db.accounts[0].password, "fixture-password");
        db.accounts[0].status = "done".into();
        let again = import(&mut db, line, None);
        assert_eq!(again.skipped, 1);
        assert_eq!(db.accounts[0].status, "done");
        assert!(!valid_email("demo---tok_fixture@icloud.com"));
        assert_eq!(import(&mut db, &line.replace("---tok_fixture----opaque---", "---tok_other---"), None).errors.len(), 1);
    }
}

#[cfg(test)]
mod private_import_test {
    use super::*;
    #[test]
    #[ignore = "Local private import check; no network or registration submission."]
    fn user_pickup_records_parse_without_keys_becoming_passwords() {
        let raw = std::fs::read_to_string(std::env::var("SIGNUP_TEST_RECORD_FILE").unwrap()).unwrap();
        let expected = raw.lines().filter(|s| !s.trim().is_empty()).count();
        let mut db = Database {default_password:"fixture-password".into(), ..Default::default()};
        let result = import(&mut db, &raw, None);
        assert!(result.errors.is_empty());
        assert_eq!(result.added, expected);
        assert!(db.accounts.iter().all(|a| valid_email(&a.email) && a.password == "fixture-password" && a.mail_api_url.starts_with("https://icsms.top/pickup#")));
    }
}

#[cfg(test)]
mod mixed_private_import_test {
    use super::*;
    #[test]
    #[ignore = "Local private mixed-format import check; no network or registration."]
    fn mixed_desktop_mailbox_formats_parse() {
        let raw = std::fs::read_to_string(std::env::var("SIGNUP_TEST_MIXED_RECORD_FILE").unwrap()).unwrap();
        let expected = raw.lines().filter(|s| !s.trim().is_empty()).count();

        let mut db = Database {default_password:"fixture-password".into(), ..Default::default()};
        let result = import(&mut db, &raw, None);
        assert!(result.errors.is_empty());
        assert_eq!(result.added, expected);
        assert!(db.accounts.iter().all(|a| valid_email(&a.email) && a.password == "fixture-password"));
    }
}
