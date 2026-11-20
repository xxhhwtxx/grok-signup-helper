//! Uses the mokeyun share-link response contract from the iCloud ZPOOL adapter.
use serde::Serialize;
use serde_json::Value;
use std::{io::Read, sync::LazyLock, time::Duration};
use url::Url;

pub fn base(email: &str) -> String {
    let lower = email.trim().to_ascii_lowercase();
    match lower.rsplit_once('@') {
        Some((local, "icloud.com")) => {
            format!("{}@icloud.com", local.split('+').next().unwrap_or(local))
        }
        _ => lower,
    }
}
pub fn validate_url(uri: &str, email: &str) -> Result<Url, String> {
    let err = || "取信链接格式错误或邮箱不匹配。".to_owned();
    let url = Url::parse(uri).map_err(|_| err())?;
    if url.host_str() == Some("icsms.top") {
        let pairs: Vec<_> =
            url::form_urlencoded::parse(url.fragment().unwrap_or("").as_bytes()).collect();
        let addresses: Vec<_> = pairs.iter().filter(|(k, _)| k == "email").collect();
        let keys: Vec<_> = pairs.iter().filter(|(k, _)| k == "key").collect();
        if uri.len() > 8192
            || url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || url.path() != "/pickup"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || pairs.len() != 2
            || addresses.len() != 1
            || keys.len() != 1
            || !keys[0].1.starts_with("tok_")
            || base(&addresses[0].1) != base(email)
        {
            return Err(err());
        }
        return Ok(url);
    }
    let params: Vec<_> = url.query_pairs().collect();
    let addresses: Vec<_> = params.iter().filter(|(k, _)| k == "email").collect();
    let tokens: Vec<_> = params.iter().filter(|(k, _)| k == "token").collect();
    if uri.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("mokeyun.icu")
        || url.port_or_known_default() != Some(443)
        || url.path() != "/share/v1/latest"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || addresses.len() != 1
        || tokens.len() != 1
        || params.len() != 2
        || tokens[0].1.trim().is_empty()
        || !base(email).ends_with("@icloud.com")
        || base(&addresses[0].1) != base(email)
    {
        return Err(err());
    }
    Ok(url)
}
pub fn trusted_service(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .host_str()
            .is_some_and(|h| ["accounts.x.ai", "grok.com", "x.ai"].contains(&h))
}

pub fn allowed_navigation(url: &Url) -> bool {
    if trusted_service(url) {
        return true;
    }
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("auth.x.ai" | "auth.grok.com" | "auth.grokusercontent.com" | "www.grok.com") => true,
        Some("auth.cursor.com") => url.path() == "/set-cookie",
        _ => false,
    }
}

#[derive(Clone, Default, Serialize)]
pub struct Mail {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub text: String,
    pub received_at: Option<String>,
    pub codes: Vec<String>,
    pub links: Vec<String>,
    pub eligible: bool,
    pub reason: String,
}
fn timestamp_ms(value: &Value) -> Option<i64> {
    if let Some(n) = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    {
        if n <= 0 {
            return None;
        }
        return if n < 100_000_000_000 {
            n.checked_mul(1000)
        } else {
            Some(n)
        };
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
}
fn plain_html(html: &str) -> String {
    static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?is)<(?:script|style)\b[^>]*>.*?</(?:script|style)>|<[^>]*>").unwrap()
    });
    TAGS.replace_all(html, " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
fn links_in(html: &str, email: &str) -> Vec<String> {
    static URLS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"https://[^\s<>"'\\]+"#).unwrap());
    let decoded = html.replace("&amp;", "&").replace("&#38;", "&");
    let mut links = Vec::new();
    for m in URLS.find_iter(&decoded) {
        let raw = m.as_str().trim_end_matches(['.', ',', ')', ']', ';']);
        let Ok(url) = Url::parse(raw) else {
            continue;
        };
        if !trusted_service(&url) {
            continue;
        }
        let path = url.path().to_ascii_lowercase();
        if !["verify", "verification", "confirm", "activate"]
            .iter()
            .any(|k| path.contains(k))
        {
            continue;
        }
        if url
            .query_pairs()
            .any(|(k, v)| k.eq_ignore_ascii_case("email") && !v.eq_ignore_ascii_case(email))
        {
            continue;
        }
        if !links.contains(&raw.to_owned()) {
            links.push(raw.to_owned());
        }
        if links.len() >= 5 {
            break;
        }
    }
    links
}
pub fn parse(payload: &Value, email: &str, since: Option<i64>) -> Result<Option<Mail>, String> {
    if payload.get("ok").and_then(Value::as_bool) == Some(false) && no_message_error(payload) {
        return Ok(None);
    }
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("取信接口返回失败，请更新取信链接。".into());
    }
    let data = payload
        .get("data")
        .filter(|v| v.is_object())
        .ok_or("取信响应格式不正确。")?;
    if data
        .get("alias")
        .and_then(Value::as_str)
        .is_some_and(|s| base(s) != base(email))
    {
        return Err("取信接口返回了其他邮箱的邮件。".into());
    }
    let msg = data.get("message").ok_or("取信响应缺少邮件字段。")?;
    if msg.is_null() {
        return Ok(None);
    }
    if !msg.is_object() {
        return Err("取信响应格式不正确。".into());
    }
    let get = |key: &str| {
        msg.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let subject = get("subject");
    let from = get("from");
    let html = get("html");
    let raw_text = get("text");
    let text = if raw_text.trim().is_empty() {
        plain_html(&html)
    } else {
        raw_text
    };
    let at = msg.get("date").and_then(timestamp_ms);
    let received_at = at
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|d| d.to_rfc3339());
    let mut codes = Vec::new();
    let numeric = |s: &str| {
        (4..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
            || s.len() == 7
                && s.as_bytes()[3] == b'-'
                && s.bytes()
                    .enumerate()
                    .all(|(i, b)| i == 3 || b.is_ascii_alphanumeric())
    };
    if let Some(values) = msg.get("codes").and_then(Value::as_array) {
        for value in values {
            let value = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_u64().map(|v| v.to_string()))
                .unwrap_or_default();
            if numeric(&value) && !codes.contains(&value) {
                codes.push(value);
            }
        }
    }
    if codes.is_empty() {
        static CODES: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"\b[0-9]{6}\b").unwrap());
        for m in CODES.find_iter(&text) {
            if !codes.contains(&m.as_str().to_owned()) {
                codes.push(m.as_str().into());
            }
            if codes.len() >= 5 {
                break;
            }
        }
    }
    if codes.is_empty() {
        static ALPHA: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"(?i)\b[A-Z0-9]{3}-[A-Z0-9]{3}\b|(?:verification code|confirmation code|验证码|验证代码)[:：\s]+([A-Z0-9]{6})\b").unwrap()
        });
        for cap in ALPHA.captures_iter(&text) {
            let code = cap
                .get(1)
                .or_else(|| cap.get(0))
                .unwrap()
                .as_str()
                .to_owned();
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    codes.truncate(5);
    let links = links_in(&format!("{text}\n{html}"), email);
    let combined = format!("{from} {subject} {text}").to_ascii_lowercase();
    static SENDER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)@(?:[a-z0-9-]+\.)*(?:x\.ai|grok\.com)(?:>|\s|$)").unwrap()
    });
    let trae = SENDER.is_match(&from) || !links.is_empty();
    let verification = ["verif", "code", "confirm", "验证码", "验证", "activate"]
        .iter()
        .any(|w| combined.contains(w));
    // Recipient headers, when present, must match the complete plus-address.
    let recipients: Vec<String> = ["to", "recipient", "email"]
        .iter()
        .filter_map(|key| msg.get(*key).and_then(Value::as_str))
        .filter(|s| s.contains('@'))
        .map(str::to_owned)
        .collect();
    let wrong_recipient = recipients.iter().any(|s| {
        !s.eq_ignore_ascii_case(email)
            && !s
                .to_ascii_lowercase()
                .contains(&format!("<{email}>").to_ascii_lowercase())
    });
    let reason = if wrong_recipient {
        "邮件收件人与当前邮箱不匹配。"
    } else if !trae || !verification {
        "最新邮件不是 Grok 验证邮件，继续等待。"
    } else if at.is_none() {
        "邮件没有有效时间，仅展示内容，请手动核对。"
    } else if since.is_none() {
        "请先打开本次注册页；这里展示的是最新邮件。"
    } else if at.unwrap() < since.unwrap() {
        "这是本次注册开始前的旧邮件，继续等待。"
    } else if codes.is_empty() && links.is_empty() {
        "已收到 Grok 邮件，但没有识别到验证码或验证链接。"
    } else {
        "已收到本次注册的 Grok 验证邮件。"
    };
    Ok(Some(Mail {
        id: msg
            .get("uid")
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_default(),
        subject,
        from,
        text: text.chars().take(12000).collect(),
        received_at,
        codes,
        links,
        eligible: reason == "已收到本次注册的 Grok 验证邮件。",
        reason: reason.into(),
    }))
}
pub fn fetch(account: &crate::store::Account, since: Option<i64>) -> Result<Option<Mail>, String> {
    if is_graph(account) {
        return fetch_graph(account, since);
    }
    let uri = validate_url(&account.mail_api_url, &account.email)?;
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirects(0)
        .build()
        .get(uri.as_str())
        .set("Accept", "application/json")
        .call();
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(404, response)) => {
            if read_payload(response).is_ok_and(|payload| no_message_error(&payload)) {
                return Ok(None);
            }
            return Err("取信失败：HTTP 404".into());
        }
        Err(ureq::Error::Status(401 | 403, _)) => {
            return Err("取信链接已失效，请重新导入有效链接。".into())
        }
        Err(ureq::Error::Status(status, _)) => return Err(format!("取信失败：HTTP {status}")),
        Err(_) => return Err("取信网络连接失败，可稍后重试。".into()),
    };
    let payload = read_payload(response)?;
    parse(&payload, &account.email, since)
}

/// Outlook / Microsoft 365 四段格式的账号走 Microsoft Graph 取信。
pub fn is_graph(account: &crate::store::Account) -> bool {
    !account.mail_client_id.trim().is_empty() && !account.mail_refresh_token.trim().is_empty()
}

const GRAPH_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const GRAPH_API: &str = "https://graph.microsoft.com/v1.0";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirects(0)
        .build()
}

fn exchange_token(client_id: &str, refresh_token: &str) -> Result<String, String> {
    const SCOPES: [&str; 3] = [
        "https://graph.microsoft.com/.default",
        "https://graph.microsoft.com/Mail.Read offline_access",
        "https://outlook.office.com/Mail.Read offline_access",
    ];
    let mut last_error = String::new();
    for scope in SCOPES {
        let response = agent().post(GRAPH_TOKEN).send_form(&[
            ("client_id", client_id.trim()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.trim()),
            ("scope", scope),
        ]);
        match response {
            Ok(response) => {
                let payload: Value = response.into_json().unwrap_or(Value::Null);
                if let Some(token) = payload.get("access_token").and_then(Value::as_str) {
                    if !token.is_empty() {
                        return Ok(token.to_owned());
                    }
                }
                last_error = payload
                    .get("error_description")
                    .and_then(Value::as_str)
                    .or_else(|| payload.get("error").and_then(Value::as_str))
                    .unwrap_or("授权服务器没有返回 access_token")
                    .chars()
                    .take(200)
                    .collect();
            }
            Err(ureq::Error::Status(code, response)) => {
                let payload: Value = response.into_json().unwrap_or(Value::Null);
                last_error = payload
                    .get("error_description")
                    .and_then(Value::as_str)
                    .or_else(|| payload.get("error").and_then(Value::as_str))
                    .map(|s| s.chars().take(200).collect())
                    .unwrap_or_else(|| format!("HTTP {code}"));
            }
            Err(_) => last_error = "网络连接失败".into(),
        }
    }
    if last_error.is_empty() {
        last_error = "refresh_token 可能已失效，请更新后再试。".into();
    }
    Err(format!("Outlook 取信授权失败：{last_error}"))
}

fn graph_payload(message: &Value, email: &str) -> Value {
    let subject = message
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or("");
    let address = message
        .pointer("/from/emailAddress/address")
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = message
        .pointer("/from/emailAddress/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let from = if name.is_empty() {
        address.to_owned()
    } else {
        format!("{name} <{address}>")
    };
    let html = message
        .pointer("/body/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let preview = message
        .get("bodyPreview")
        .and_then(Value::as_str)
        .unwrap_or("");
    let uid = message.get("id").and_then(Value::as_str).unwrap_or("");
    let date = message
        .get("receivedDateTime")
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis());
    let text = if html.trim().is_empty() { preview } else { "" };
    serde_json::json!({
        "ok": true,
        "data": {
            "alias": email,
            "message": {
                "uid": uid,
                "subject": subject,
                "from": from,
                "html": html,
                "text": text,
                "date": date,
            }
        }
    })
}

fn fetch_graph(
    account: &crate::store::Account,
    since: Option<i64>,
) -> Result<Option<Mail>, String> {
    let token = exchange_token(&account.mail_client_id, &account.mail_refresh_token)?;
    let url = format!(
        "{GRAPH_API}/me/messages?$top=10&$select=id,subject,from,receivedDateTime,body,bodyPreview&$orderby=receivedDateTime%20desc"
    );
    let response = agent()
        .get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .call();
    let value: Value = match response {
        Ok(response) => response.into_json().unwrap_or(Value::Null),
        Err(ureq::Error::Status(401 | 403, _)) => {
            return Err("Outlook 凭据已失效，请重新导入 refresh_token。".into())
        }
        Err(ureq::Error::Status(code, _)) => return Err(format!("Outlook 取信失败：HTTP {code}")),
        Err(_) => return Err("Outlook 取信网络连接失败，可稍后重试。".into()),
    };
    let items = value
        .get("value")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok(None);
    }
    let mut fallback: Option<Mail> = None;
    for message in &items {
        let payload = graph_payload(message, &account.email);
        if let Ok(Some(mail)) = parse(&payload, &account.email, since) {
            if mail.eligible {
                return Ok(Some(mail));
            }
            if fallback.is_none() {
                fallback = Some(mail);
            }
        }
    }
    Ok(fallback)
}

fn no_message_error(value: &Value) -> bool {
    static EMPTY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(\bmail[ _-]*not[ _-]*found\b|\bmessage[ _-]*not[ _-]*found\b|\bno[ _-]*message\b|未找到.{0,16}邮件|暂无.{0,16}邮件|邮件不存在)").unwrap()
    });
    if let Some(text) = value.as_str() {
        return EMPTY.is_match(text);
    }
    if let Some(obj) = value.as_object() {
        return [
            "data", "error", "message", "msg", "detail", "code", "reason",
        ]
        .iter()
        .filter_map(|key| obj.get(*key))
        .any(no_message_error);
    }
    false
}

fn read_payload(response: ureq::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取取信响应。")?;
    if bytes.len() > 1024 * 1024 {
        return Err("取信响应过大。".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "取信接口未返回有效 JSON。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pickup_requires_matching_email_and_exact_https_host() {
        let uri = "https://icsms.top/pickup#email=demo%40icloud.com&key=tok_fixture";
        assert!(validate_url(uri, "demo+tag@icloud.com").is_ok());
        assert!(validate_url(uri, "other@icloud.com").is_err());
        assert!(validate_url(
            &uri.replace("icsms.top", "icsms.top.evil.test"),
            "demo@icloud.com"
        )
        .is_err());
        assert!(validate_url(&uri.replace("https:", "http:"), "demo@icloud.com").is_err());
    }
    #[test]
    fn alphanumeric_grok_codes_and_foreign_mail() {
        let mut p = fixture();
        p["data"]["message"]["codes"] = json!([]);
        p["data"]["message"]["text"] = json!("Your verification code is ABC-123");
        let m = parse(&p, "demo@icloud.com", Some(1_700_000_000_000))
            .unwrap()
            .unwrap();
        assert!(m.eligible);
        assert_eq!(m.codes, vec!["ABC-123"]);
        p["data"]["message"]["from"] = json!("Grok <mail@evilx.ai>");
        assert!(
            !parse(&p, "demo@icloud.com", Some(1_700_000_000_000))
                .unwrap()
                .unwrap()
                .eligible
        );
        p["data"]["message"]["from"] = json!("Meta <security@account.meta.com>");
        assert!(
            !parse(&p, "demo@icloud.com", Some(1_700_000_000_000))
                .unwrap()
                .unwrap()
                .eligible
        );
    }
    use serde_json::json;
    fn fixture() -> Value {
        json!({"ok":true,"data":{"alias":"demo@icloud.com","message":{"uid":"1","from":"Grok <no-reply@x.ai>","subject":"Grok verification code","date":1700000001,"text":"Your verification code is 123456.","codes":["123456"]}}})
    }
    #[test]
    fn provider_empty_mail_is_not_bad_credentials() {
        let empty = json!({"ok":false,"data":{"message":"Mail not found"}});
        assert!(no_message_error(&empty));
        assert!(parse(&empty, "demo@icloud.com", Some(0)).unwrap().is_none());
        assert!(!no_message_error(
            &json!({"ok":false,"error":"Invalid token"})
        ));
        assert!(!no_message_error(
            &json!({"ok":false,"error":"Email not found"})
        ));
        assert!(!no_message_error(
            &json!({"data":{"alias":"no_message@icloud.com"}})
        ));
    }
    #[test]
    fn fresh_mail_only() {
        let p = fixture();
        assert!(
            parse(&p, "demo+tag@icloud.com", Some(1700000000000))
                .unwrap()
                .unwrap()
                .eligible
        );
        assert!(
            !parse(&p, "demo@icloud.com", Some(1700000002000))
                .unwrap()
                .unwrap()
                .eligible
        );
        assert!(
            !parse(&p, "demo@icloud.com", None)
                .unwrap()
                .unwrap()
                .eligible
        );
    }
    #[test]
    fn unrelated_and_undated_mail() {
        let mut p = fixture();
        p["data"]["message"]["from"] = json!("Z.ai");
        p["data"]["message"]["subject"] = json!("Z.ai code");
        assert!(
            !parse(&p, "demo@icloud.com", Some(0))
                .unwrap()
                .unwrap()
                .eligible
        );
        let mut p = fixture();
        p["data"]["message"]["date"] = Value::Null;
        assert!(
            !parse(&p, "demo@icloud.com", Some(0))
                .unwrap()
                .unwrap()
                .eligible
        );
    }
    #[test]
    fn recipient_and_links_do_not_cross_aliases() {
        let mut p = fixture();
        p["data"]["message"]["to"] = json!("demo+other@icloud.com");
        assert!(
            !parse(&p, "demo+tag@icloud.com", Some(0))
                .unwrap()
                .unwrap()
                .eligible
        );
        let links = links_in("https://grok.com/users/verify?email=demo%2Bother%40icloud.com&token=fixture https://grok.com.evil.test/verify https://grok.com/users/verify?email=demo%2Btag%40icloud.com&amp;token=fixture", "demo+tag@icloud.com");
        assert_eq!(links.len(), 1);
        assert!(links[0].contains("&token="));
    }
    #[test]
    fn urls_are_bounded_and_secrets_redacted() {
        let uri = "https://mokeyun.icu/share/v1/latest?email=demo%40icloud.com&token=fixture";
        assert!(validate_url(uri, "demo+tag@icloud.com").is_ok());
        for bad in [
            uri.replace("https://", "http://"),
            uri.replace("mokeyun.icu", "mokeyun.icu.evil.test"),
            format!("{uri}&email=demo%40icloud.com"),
            uri.replace("demo%40", "other%40"),
        ] {
            assert!(!validate_url(&bad, "demo@icloud.com")
                .unwrap_err()
                .contains("fixture"));
        }
        assert!(!trusted_service(
            &Url::parse("https://grok.com@evil.test/verify").unwrap()
        ));
    }
    #[test]
    fn html_and_numeric_codes() {
        let mut p = fixture();
        p["data"]["message"]["text"] = json!("");
        p["data"]["message"]["html"] =
            json!("<p>Verification code <b>654321</b></p><script>steal()</script>");
        p["data"]["message"]["codes"] = json!([]);
        let m = parse(&p, "demo@icloud.com", Some(0)).unwrap().unwrap();
        assert_eq!(m.codes, vec!["654321"]);
        assert!(!m.text.contains("steal"));
        assert!(parse(
            &json!({"ok":true,"data":{"message":null}}),
            "demo@icloud.com",
            Some(0)
        )
        .unwrap()
        .is_none());
    }
    #[test]
    fn cookie_chain_navigation_and_sender_variants() {
        for ok in [
            "https://accounts.x.ai/cookie-chain?q=1",
            "https://auth.x.ai/set-cookie?q=1",
            "https://auth.grok.com/set-cookie?q=1",
            "https://auth.grokusercontent.com/set-cookie?q=1",
            "https://auth.cursor.com/set-cookie?q=1",
            "https://www.grok.com/",
        ] {
            let url = Url::parse(ok).unwrap();
            assert!(allowed_navigation(&url), "expected allowed: {ok}");
        }
        for bad in [
            "https://auth.cursor.com/",
            "https://auth.cursor.com/login",
            "http://auth.x.ai/set-cookie?q=1",
            "https://auth.grok.com.evil.test/set-cookie",
            "https://user@auth.grok.com/set-cookie",
            "https://evil.example/",
        ] {
            let url = Url::parse(bad).unwrap();
            assert!(!allowed_navigation(&url), "expected blocked: {bad}");
        }
        assert!(!trusted_service(
            &Url::parse("https://auth.x.ai/set-cookie?q=1").unwrap()
        ));
        assert!(!trusted_service(
            &Url::parse("https://auth.cursor.com/set-cookie?q=1").unwrap()
        ));

        let mut p = fixture();
        p["data"]["message"]["from"] = json!("Grok <noreply@grok.com>");
        p["data"]["message"]["subject"] = json!("Login link");
        p["data"]["message"]["text"] = json!("Code 654321");
        p["data"]["message"]["codes"] = json!(["654321"]);
        assert!(parse(&p, "demo@icloud.com", Some(0)).unwrap().unwrap().eligible);

        let mut p_evil = fixture();
        p_evil["data"]["message"]["from"] = json!("Spoof <noreply@evilx.ai>");
        p_evil["data"]["message"]["subject"] = json!("Other");
        p_evil["data"]["message"]["text"] = json!("Code 654321");
        assert!(!parse(&p_evil, "demo@icloud.com", Some(0)).unwrap().unwrap().eligible);
    }
    #[test]
    #[ignore = "Read-only test of user-supplied mailbox links; requires local credential files."]
    fn live_import_and_read() {
        let file = std::env::var("Grok_TEST_MAIL_FILE").expect("set local mailbox text path");
        let defaults =
            std::env::var("Grok_TEST_DEFAULTS_FILE").expect("set local password config path");
        let config: Value = serde_json::from_slice(&std::fs::read(defaults).unwrap()).unwrap();
        let password = config["signup_password"]
            .as_str()
            .expect("configured password");
        let mut db = crate::store::Database {
            default_password: password.into(),
            ..Default::default()
        };
        let text = crate::store::decode_text(&std::fs::read(file).unwrap()).unwrap();
        let r = crate::store::import(&mut db, &text, None);
        assert!(r.errors.is_empty());
        assert_eq!(db.accounts.len(), 10);
        assert!(db.accounts.iter().all(|a| a.password == password));
        let duplicate = crate::store::import(&mut db, &text, None);
        assert_eq!(duplicate.skipped, 10);
        let _mail = fetch(&db.accounts[0], None).expect("live mailbox response must parse");
        // Deliberately do not print message content, email addresses or credentials.
    }
}
