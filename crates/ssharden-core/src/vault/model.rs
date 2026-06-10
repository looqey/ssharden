//! Host data model and parsing from Bitwarden Login items.
//!
//! Convention (see `DESIGN.md`): a host is a normal Bitwarden Login item.
//! The URI **scheme** selects the launcher (`ssh` / `sftp` / `rdp`); host / port /
//! user come from the URI; protocol-specific extras live in known-named custom fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A connectable host parsed from a Bitwarden Login item.
#[derive(Debug, Clone, Serialize)]
pub struct Host {
    /// Bitwarden item id.
    pub id: String,
    /// Display name (the item's `name`).
    pub name: String,
    /// Folder id, for UI grouping (and collection id later for orgs).
    pub folder_id: Option<String>,
    /// Resolved folder name (filled by `list_hosts`), for a friendly group label.
    pub folder_name: Option<String>,
    /// Login username, if present.
    pub username: Option<String>,
    /// Parsed protocol URIs (one host may expose several protocols).
    pub uris: Vec<HostUri>,
    /// Known-named custom fields (e.g. `jump`, `domain`, `sshkey`).
    pub fields: BTreeMap<String, String>,
}

/// A single parsed connection URI from a Login item.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HostUri {
    /// URI scheme: selects the launcher (`ssh`, `sftp`, `rdp`, `ftp`).
    pub scheme: String,
    /// Hostname or IP.
    pub host: String,
    /// Port (filled with the scheme default when the URI omits it).
    pub port: Option<u16>,
    /// Optional user component (`ssh://user@host`).
    pub user: Option<String>,
    /// The original raw URI string.
    pub raw: String,
}

/// Authentication/lock state of the `bw` CLI account (from `bw status`).
#[derive(Debug, Clone, Serialize)]
pub struct AccountStatus {
    /// Configured server URL, if any.
    pub server_url: Option<String>,
    /// Logged-in account email, if authenticated.
    pub user_email: Option<String>,
    /// `unauthenticated` | `locked` | `unlocked`.
    pub status: String,
}

/// Default port for a recognized scheme, or `None` if the scheme is not a host protocol.
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "ssh" | "sftp" => Some(22),
        "rdp" => Some(3389),
        "ftp" => Some(21),
        _ => None,
    }
}

/// Parse a single raw URI string into a [`HostUri`].
///
/// Recognizes the `ssh`, `sftp`, `rdp`, and `ftp` schemes. Returns `None` for any
/// other scheme or for a string without a usable `scheme://host` shape. A missing
/// port is filled with the scheme default.
pub fn parse_host_uri(raw: &str) -> Option<HostUri> {
    let raw = raw.trim();
    let (scheme, rest) = raw.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let dport = default_port(&scheme)?; // also rejects unknown schemes

    // Authority is everything up to the first path/query/fragment delimiter.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }

    // Optional `user@` prefix (take everything before the last '@' as the user).
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((u, hp)) if !u.is_empty() => (Some(u.to_string()), hp),
        _ => (None, authority),
    };

    // Split host and optional port. Bracketed IPv6 (`[::1]:22`) is handled minimally.
    let (host, port) = if let Some(stripped) = host_port.strip_prefix('[') {
        // `[ipv6]` or `[ipv6]:port`
        let (h, after) = stripped.split_once(']')?;
        let port = after.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
        (h.to_string(), port)
    } else if let Some((h, p)) = host_port.rsplit_once(':') {
        match p.parse::<u16>() {
            Ok(port) => (h.to_string(), Some(port)),
            // A colon that isn't a valid port: treat the whole thing as the host.
            Err(_) => (host_port.to_string(), None),
        }
    } else {
        (host_port.to_string(), None)
    };

    if host.is_empty() {
        return None;
    }

    Some(HostUri {
        scheme,
        host,
        port: Some(port.unwrap_or(dport)),
        user,
        raw: raw.to_string(),
    })
}

/// Read an optional string field from a JSON object (`null`/missing → `None`).
fn opt_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

/// Build a [`Host`] from a single Bitwarden cipher (`item`) JSON value.
///
/// Returns `None` for ciphers that are not Login items (`type != 1`) or that carry
/// no URI parseable into a recognized host protocol.
pub fn host_from_cipher(v: &serde_json::Value) -> Option<Host> {
    // Bitwarden item type 1 == Login.
    if v.get("type").and_then(|t| t.as_u64()) != Some(1) {
        return None;
    }
    let login = v.get("login")?;

    let uris: Vec<HostUri> = login
        .get("uris")
        .and_then(|u| u.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| entry.get("uri").and_then(|s| s.as_str()))
                .filter_map(parse_host_uri)
                .collect()
        })
        .unwrap_or_default();

    if uris.is_empty() {
        return None;
    }

    let mut fields = BTreeMap::new();
    if let Some(arr) = v.get("fields").and_then(|f| f.as_array()) {
        for f in arr {
            if let Some(name) = f.get("name").and_then(|n| n.as_str()) {
                let value = f
                    .get("value")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                fields.insert(name.to_string(), value);
            }
        }
    }

    Some(Host {
        id: opt_str(v, "id").unwrap_or_default(),
        name: opt_str(v, "name").unwrap_or_default(),
        folder_id: opt_str(v, "folderId"),
        folder_name: None, // resolved by VaultClient::list_hosts
        username: opt_str(login, "username"),
        uris,
        fields,
    })
}

/// User-supplied fields for creating or editing a host (a Bitwarden Login item).
#[derive(Debug, Clone, Deserialize)]
pub struct HostInput {
    /// Display name.
    pub name: String,
    /// Folder id, or `None` to leave unchanged (edit) / unfiled (create).
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Login username.
    #[serde(default)]
    pub username: Option<String>,
    /// Password. Empty/`None` on edit means "keep the existing password".
    #[serde(default)]
    pub password: Option<String>,
    /// Raw connection URIs, e.g. `ssh://host:22`.
    #[serde(default)]
    pub uris: Vec<String>,
    /// Custom fields (e.g. `jump`, `domain`).
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// Build a Bitwarden Login cipher JSON from a [`HostInput`].
///
/// `base` is the existing cipher when editing: unspecified bits (password when the
/// input leaves it blank, folder when `folder_id` is `None`, notes/totp/reprompt) are
/// carried over so an edit doesn't silently wipe them.
pub fn login_cipher_json(input: &HostInput, base: Option<&serde_json::Value>) -> serde_json::Value {
    use serde_json::json;

    let existing_pw = base
        .and_then(|b| b.get("login"))
        .and_then(|l| l.get("password"))
        .and_then(|p| p.as_str());
    let password = match input.password.as_deref() {
        Some(p) if !p.is_empty() => Some(p),
        _ => existing_pw,
    };

    let folder_id = match &input.folder_id {
        // Explicit folder selected in the form.
        Some(f) if !f.is_empty() => json!(f),
        // Explicit "No folder" (empty string from the picker).
        Some(_) => json!(null),
        // Unspecified by the caller: keep whatever the item already had.
        None => base
            .and_then(|b| b.get("folderId"))
            .cloned()
            .unwrap_or(json!(null)),
    };

    let uris: Vec<_> = input
        .uris
        .iter()
        .filter(|u| !u.trim().is_empty())
        .map(|u| json!({ "match": null, "uri": u }))
        .collect();

    let fields: Vec<_> = input
        .fields
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| json!({ "name": k, "value": v, "type": 0, "linkedId": null }))
        .collect();

    let carry = |k: &str, default: serde_json::Value| {
        base.and_then(|b| b.get(k)).cloned().unwrap_or(default)
    };

    json!({
        "organizationId": null,
        "folderId": folder_id,
        "type": 1,
        "name": input.name,
        "notes": carry("notes", json!(null)),
        "favorite": carry("favorite", json!(false)),
        "fields": fields,
        "login": {
            "uris": uris,
            "username": input.username,
            "password": password,
            "totp": base
                .and_then(|b| b.get("login"))
                .and_then(|l| l.get("totp"))
                .cloned()
                .unwrap_or(json!(null)),
        },
        "reprompt": carry("reprompt", json!(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_ssh_with_user_and_port() {
        let u = parse_host_uri("ssh://admin@10.0.0.5:2222").unwrap();
        assert_eq!(u.scheme, "ssh");
        assert_eq!(u.host, "10.0.0.5");
        assert_eq!(u.port, Some(2222));
        assert_eq!(u.user.as_deref(), Some("admin"));
    }

    #[test]
    fn fills_default_ports_per_scheme() {
        assert_eq!(parse_host_uri("ssh://host").unwrap().port, Some(22));
        assert_eq!(parse_host_uri("sftp://host").unwrap().port, Some(22));
        assert_eq!(parse_host_uri("rdp://host").unwrap().port, Some(3389));
        assert_eq!(parse_host_uri("ftp://host").unwrap().port, Some(21));
    }

    #[test]
    fn strips_path_and_handles_no_user() {
        let u = parse_host_uri("rdp://10.0.0.5:3389/some/path").unwrap();
        assert_eq!(u.host, "10.0.0.5");
        assert_eq!(u.port, Some(3389));
        assert_eq!(u.user, None);
    }

    #[test]
    fn rejects_unknown_scheme_and_garbage() {
        assert!(parse_host_uri("https://example.com").is_none());
        assert!(parse_host_uri("not-a-uri").is_none());
        assert!(parse_host_uri("ssh://").is_none());
    }

    #[test]
    fn host_from_login_cipher_with_ssh_uri() {
        let cipher = json!({
            "id": "abc-123",
            "name": "prod-db-01",
            "type": 1,
            "folderId": "folder-9",
            "login": {
                "username": "admin",
                "uris": [
                    { "uri": "ssh://admin@10.0.0.5:22" },
                    { "uri": "https://ignore.me" }
                ]
            },
            "fields": [
                { "name": "jump", "value": "bastion.corp" },
                { "name": "domain", "value": null }
            ]
        });
        let h = host_from_cipher(&cipher).unwrap();
        assert_eq!(h.id, "abc-123");
        assert_eq!(h.name, "prod-db-01");
        assert_eq!(h.folder_id.as_deref(), Some("folder-9"));
        assert_eq!(h.username.as_deref(), Some("admin"));
        assert_eq!(h.uris.len(), 1); // the https uri is filtered out
        assert_eq!(h.uris[0].scheme, "ssh");
        assert_eq!(h.fields.get("jump").map(String::as_str), Some("bastion.corp"));
        assert_eq!(h.fields.get("domain").map(String::as_str), Some(""));
    }

    #[test]
    fn login_without_host_uri_is_not_a_host() {
        let cipher = json!({
            "id": "x", "name": "gmail", "type": 1,
            "login": { "username": "me", "uris": [ { "uri": "https://mail.google.com" } ] }
        });
        assert!(host_from_cipher(&cipher).is_none());
    }

    #[test]
    fn non_login_item_is_not_a_host() {
        let note = json!({ "id": "n", "name": "a note", "type": 2 });
        assert!(host_from_cipher(&note).is_none());
    }

    #[test]
    fn builds_create_cipher_from_input() {
        let input = HostInput {
            name: "web-01".into(),
            folder_id: None,
            username: Some("root".into()),
            password: Some("s3cret".into()),
            uris: vec!["ssh://web-01.lan:22".into(), "".into()],
            fields: {
                let mut m = BTreeMap::new();
                m.insert("jump".to_string(), "bastion".to_string());
                m.insert("blank".to_string(), "".to_string());
                m
            },
        };
        let c = login_cipher_json(&input, None);
        assert_eq!(c["type"], 1);
        assert_eq!(c["name"], "web-01");
        assert_eq!(c["folderId"], json!(null));
        assert_eq!(c["login"]["username"], "root");
        assert_eq!(c["login"]["password"], "s3cret");
        assert_eq!(c["login"]["uris"].as_array().unwrap().len(), 1); // empty uri dropped
        assert_eq!(c["login"]["uris"][0]["uri"], "ssh://web-01.lan:22");
        let fields = c["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 1); // empty-valued field dropped
        assert_eq!(fields[0]["name"], "jump");
    }

    #[test]
    fn edit_keeps_password_and_folder_when_blank() {
        let base = json!({
            "folderId": "folder-7",
            "notes": "keep me",
            "login": { "password": "OLDPASS", "totp": "seed" }
        });
        let input = HostInput {
            name: "renamed".into(),
            folder_id: None,           // keep existing folder
            username: Some("admin".into()),
            password: None,            // keep existing password
            uris: vec!["rdp://h:3389".into()],
            fields: BTreeMap::new(),
        };
        let c = login_cipher_json(&input, Some(&base));
        assert_eq!(c["login"]["password"], "OLDPASS");
        assert_eq!(c["folderId"], "folder-7");
        assert_eq!(c["notes"], "keep me");
        assert_eq!(c["login"]["totp"], "seed");
        assert_eq!(c["name"], "renamed");
    }
}
