use std::collections::BTreeMap;
use std::path::Path;

use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix_flake_lock::{AttrSet, AttrValue, Edge, LockFile, LockedNode};

pub(super) fn render(lock_path: &Path, config_info: &str) -> Result<String> {
    let inputs = format_lock_inputs(lock_path)?;
    if config_info.is_empty() {
        Ok(inputs)
    } else {
        Ok(format!("{inputs}\n\n{config_info}"))
    }
}

fn format_lock_inputs(lock_path: &Path) -> Result<String> {
    if !lock_path.exists() {
        return Ok("Inputs:\n  (no lock file)".to_string());
    }

    let bytes = std::fs::read(lock_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {}", lock_path.display()))?;
    let lock = LockFile::parse(&bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to parse {}", lock_path.display()))?;
    lock.validate()
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to validate {}", lock_path.display()))?;

    let root_inputs = lock.root().inputs();
    if root_inputs.is_empty() {
        return Ok("Inputs:\n  (no inputs)".to_string());
    }

    let mut lines = Vec::with_capacity(root_inputs.len() + 1);
    lines.push("Inputs:".to_string());
    for (index, input) in root_inputs.iter().enumerate() {
        let reference = match input.edge() {
            Edge::Follows(_) => "(follows)".to_string(),
            Edge::Node(node_id) => {
                let locked = lock
                    .node(*node_id)
                    .and_then(|node| node.locked())
                    .ok_or_else(|| {
                        miette!("input {:?} does not point to a locked node", input.name())
                    })?;
                format_locked_ref(locked)?
            }
        };
        let prefix = if index + 1 == root_inputs.len() {
            "└───"
        } else {
            "├───"
        };
        lines.push(format!("{prefix}{}: {reference}", input.name()));
    }
    Ok(lines.join("\n"))
}

fn format_locked_ref(node: &LockedNode<'_>) -> Result<String> {
    let attrs = node.locked();
    let input_type = required_str(attrs, "type")?;
    let dir = attrs.get_str("dir");

    let (base, mut query) = match input_type {
        "github" | "gitlab" | "sourcehut" => {
            let owner = encode_path_segment(required_str(attrs, "owner")?);
            let repo = encode_path_segment(required_str(attrs, "repo")?);
            let mut base = format!("{input_type}:{owner}/{repo}");
            if let Some(revision) = attrs.get_str("rev").or_else(|| attrs.get_str("ref")) {
                base.push('/');
                base.push_str(&encode_path_segment(revision));
            }

            let mut query = BTreeMap::new();
            copy_string_attr(attrs, &mut query, "host");
            copy_string_attr(attrs, &mut query, "narHash");
            (base, query)
        }
        "git" => {
            let url = required_str(attrs, "url")?;
            let base = if url.starts_with("git:") {
                url.to_string()
            } else {
                format!("git+{url}")
            };
            let mut query = BTreeMap::new();
            copy_string_attr(attrs, &mut query, "ref");
            copy_string_attr(attrs, &mut query, "rev");
            copy_true_attr(attrs, &mut query, "exportIgnore");
            copy_true_attr(attrs, &mut query, "lfs");
            copy_true_attr(attrs, &mut query, "shallow");
            copy_true_attr(attrs, &mut query, "submodules");
            copy_true_attr(attrs, &mut query, "verifyCommit");
            copy_string_attr(attrs, &mut query, "keytype");
            copy_string_attr(attrs, &mut query, "publicKey");
            copy_string_attr(attrs, &mut query, "publicKeys");
            (base, query)
        }
        "hg" => {
            let base = format!("hg+{}", required_str(attrs, "url")?);
            let mut query = BTreeMap::new();
            copy_string_attr(attrs, &mut query, "ref");
            copy_string_attr(attrs, &mut query, "rev");
            (base, query)
        }
        "file" | "tarball" => {
            let mut query = BTreeMap::new();
            copy_string_attr(attrs, &mut query, "narHash");
            (required_str(attrs, "url")?.to_string(), query)
        }
        "path" => {
            let base = format!("path:{}", encode_path(required_str(attrs, "path")?));
            let mut query = BTreeMap::new();
            for attr in attrs.iter() {
                if !matches!(attr.name(), "__final" | "dir" | "path" | "type") {
                    query.insert(attr.name().to_string(), attr_value_string(attr.value()));
                }
            }
            (base, query)
        }
        "indirect" => {
            let mut base = format!("flake:{}", encode_path_segment(required_str(attrs, "id")?));
            if let Some(revision) = attrs.get_str("ref").or_else(|| attrs.get_str("rev")) {
                base.push('/');
                base.push_str(&encode_path_segment(revision));
            }
            (base, BTreeMap::new())
        }
        other => return Err(miette!("unsupported locked input type {other:?}")),
    };

    if let Some(dir) = dir {
        query.insert("dir".to_string(), dir.to_string());
    }
    Ok(format_brief_ref(&merge_query(&base, query)))
}

fn required_str<'a>(attrs: &'a AttrSet<'_>, name: &str) -> Result<&'a str> {
    attrs
        .get_str(name)
        .ok_or_else(|| miette!("locked input is missing string attribute {name:?}"))
}

fn copy_string_attr(attrs: &AttrSet<'_>, query: &mut BTreeMap<String, String>, name: &str) {
    if let Some(value) = attrs.get_str(name) {
        query.insert(name.to_string(), value.to_string());
    }
}

fn copy_true_attr(attrs: &AttrSet<'_>, query: &mut BTreeMap<String, String>, name: &str) {
    if attrs.get_bool(name).unwrap_or(false) {
        query.insert(name.to_string(), "1".to_string());
    }
}

fn attr_value_string(value: &AttrValue<'_>) -> String {
    match value {
        AttrValue::String(value) => value.to_string(),
        AttrValue::Integer(value) => value.to_string(),
        AttrValue::Bool(value) => u8::from(*value).to_string(),
    }
}

fn merge_query(base: &str, additions: BTreeMap<String, String>) -> String {
    let (base, fragment) = base
        .split_once('#')
        .map_or((base, None), |(base, fragment)| (base, Some(fragment)));
    let (base, existing_query) = base
        .split_once('?')
        .map_or((base, None), |(base, query)| (base, Some(query)));
    let mut query = existing_query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (percent_decode(name), percent_decode(value)))
        .collect::<BTreeMap<_, _>>();
    query.extend(additions);

    let encoded = query
        .into_iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                percent_encode(&name, ":@/?"),
                percent_encode(&value, ":@/?")
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    let mut result = base.to_string();
    if !encoded.is_empty() {
        result.push('?');
        result.push_str(&encoded);
    }
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(fragment);
    }
    result
}

fn format_brief_ref(reference: &str) -> String {
    let Some((prefix, revision)) = reference.rsplit_once('/') else {
        return reference.to_string();
    };
    if revision.len() >= 40 && revision.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("{prefix}/{}", &revision[..7])
    } else {
        reference.to_string()
    }
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_path_segment(segment: &str) -> String {
    percent_encode(segment, ":@")
}

fn percent_encode(value: &str, keep: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || keep.as_bytes().contains(&byte)
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const LOCK: &str = r#"{
      "nodes": {
        "github": {
          "locked": {
            "narHash": "sha256-a/b+c=",
            "owner": "example",
            "repo": "project",
            "rev": "0123456789abcdef0123456789abcdef01234567",
            "type": "github"
          },
          "original": { "owner": "example", "repo": "project", "type": "github" }
        },
        "local": {
          "locked": { "dir": "src/modules", "path": ".", "type": "path" },
          "original": { "dir": "src/modules", "path": ".", "type": "path" },
          "parent": []
        },
        "root": {
          "inputs": {
            "follows": ["github"],
            "github": "github",
            "local": "local"
          }
        }
      },
      "root": "root",
      "version": 7
    }"#;

    #[test]
    fn formats_root_lock_inputs_without_nix() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("devenv.lock");
        std::fs::write(&lock_path, LOCK).unwrap();

        assert_eq!(
            render(&lock_path, "").unwrap(),
            concat!(
                "Inputs:\n",
                "├───follows: (follows)\n",
                "├───github: github:example/project/0123456789abcdef0123456789abcdef01234567?narHash=sha256-a/b%2Bc%3D\n",
                "└───local: path:.?dir=src/modules"
            )
        );
    }

    #[test]
    fn reports_missing_lock_file() {
        let temp = TempDir::new().unwrap();
        assert_eq!(
            render(&temp.path().join("devenv.lock"), "").unwrap(),
            "Inputs:\n  (no lock file)"
        );
    }

    #[test]
    fn appends_config_info_as_a_separate_section() {
        let temp = TempDir::new().unwrap();
        assert_eq!(
            render(&temp.path().join("devenv.lock"), "# packages\n- jq").unwrap(),
            "Inputs:\n  (no lock file)\n\n# packages\n- jq"
        );
    }

    #[test]
    fn merges_and_canonicalizes_existing_url_queries() {
        let additions = BTreeMap::from([("narHash".to_string(), "sha256-a/b+c=".to_string())]);
        assert_eq!(
            merge_query("https://example.com/archive?z=a%2Bb&a=x#source", additions),
            "https://example.com/archive?a=x&narHash=sha256-a/b%2Bc%3D&z=a%2Bb#source"
        );
    }
}
