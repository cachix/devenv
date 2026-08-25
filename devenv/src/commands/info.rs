//! `devenv info`: render environment metadata for the CLI.

use std::collections::BTreeMap;

use devenv::{Devenv, InputAttribute, InputSource, Metadata};
use miette::{Result, miette};

pub async fn run(devenv: &Devenv) -> Result<String> {
    render(&devenv.metadata().await?)
}

fn render(metadata: &Metadata) -> Result<String> {
    let mut sections = vec![format_inputs(metadata)?];
    sections.extend(
        metadata
            .info_sections
            .iter()
            .filter(|(_, entries)| !entries.is_empty())
            .map(|(name, entries)| {
                format!(
                    "# {name}\n{}",
                    entries
                        .iter()
                        .map(|entry| format!("- {entry}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }),
    );
    Ok(sections.join("\n\n"))
}

fn format_inputs(metadata: &Metadata) -> Result<String> {
    let Some(inputs) = &metadata.inputs else {
        return Ok("Inputs:\n  (no lock file)".to_string());
    };
    if inputs.is_empty() {
        return Ok("Inputs:\n  (no inputs)".to_string());
    }

    let mut lines = Vec::with_capacity(inputs.len() + 1);
    lines.push("Inputs:".to_string());
    for (index, input) in inputs.iter().enumerate() {
        let reference = match &input.source {
            InputSource::Follows(_) => "(follows)".to_string(),
            InputSource::Locked(attributes) => format_locked_ref(attributes)?,
        };
        let prefix = if index + 1 == inputs.len() {
            "└───"
        } else {
            "├───"
        };
        lines.push(format!("{prefix}{}: {reference}", input.name));
    }
    Ok(lines.join("\n"))
}

fn format_locked_ref(attributes: &BTreeMap<String, InputAttribute>) -> Result<String> {
    let input_type = required_str(attributes, "type")?;
    let dir = get_str(attributes, "dir");

    let (base, mut query) = match input_type {
        "github" | "gitlab" | "sourcehut" => {
            let owner = encode_path_segment(required_str(attributes, "owner")?);
            let repo = encode_path_segment(required_str(attributes, "repo")?);
            let mut base = format!("{input_type}:{owner}/{repo}");
            if let Some(revision) =
                get_str(attributes, "rev").or_else(|| get_str(attributes, "ref"))
            {
                base.push('/');
                base.push_str(&encode_path_segment(revision));
            }

            let mut query = BTreeMap::new();
            copy_string_attr(attributes, &mut query, "host");
            copy_string_attr(attributes, &mut query, "narHash");
            (base, query)
        }
        "git" => {
            let url = required_str(attributes, "url")?;
            let base = if url.starts_with("git:") {
                url.to_string()
            } else {
                format!("git+{url}")
            };
            let mut query = BTreeMap::new();
            copy_string_attr(attributes, &mut query, "ref");
            copy_string_attr(attributes, &mut query, "rev");
            copy_true_attr(attributes, &mut query, "exportIgnore");
            copy_true_attr(attributes, &mut query, "lfs");
            copy_true_attr(attributes, &mut query, "shallow");
            copy_true_attr(attributes, &mut query, "submodules");
            copy_true_attr(attributes, &mut query, "verifyCommit");
            copy_string_attr(attributes, &mut query, "keytype");
            copy_string_attr(attributes, &mut query, "publicKey");
            copy_string_attr(attributes, &mut query, "publicKeys");
            (base, query)
        }
        "hg" => {
            let base = format!("hg+{}", required_str(attributes, "url")?);
            let mut query = BTreeMap::new();
            copy_string_attr(attributes, &mut query, "ref");
            copy_string_attr(attributes, &mut query, "rev");
            (base, query)
        }
        "file" | "tarball" => {
            let mut query = BTreeMap::new();
            copy_string_attr(attributes, &mut query, "narHash");
            (required_str(attributes, "url")?.to_string(), query)
        }
        "path" => {
            let base = format!("path:{}", encode_path(required_str(attributes, "path")?));
            let mut query = BTreeMap::new();
            for (name, value) in attributes {
                if !matches!(name.as_str(), "__final" | "dir" | "path" | "type") {
                    query.insert(name.clone(), attr_value_string(value));
                }
            }
            (base, query)
        }
        "indirect" => {
            let mut base = format!(
                "flake:{}",
                encode_path_segment(required_str(attributes, "id")?)
            );
            if let Some(revision) =
                get_str(attributes, "ref").or_else(|| get_str(attributes, "rev"))
            {
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

fn get_str<'a>(attributes: &'a BTreeMap<String, InputAttribute>, name: &str) -> Option<&'a str> {
    attributes.get(name).and_then(InputAttribute::as_str)
}

fn required_str<'a>(
    attributes: &'a BTreeMap<String, InputAttribute>,
    name: &str,
) -> Result<&'a str> {
    get_str(attributes, name)
        .ok_or_else(|| miette!("locked input is missing string attribute {name:?}"))
}

fn copy_string_attr(
    attributes: &BTreeMap<String, InputAttribute>,
    query: &mut BTreeMap<String, String>,
    name: &str,
) {
    if let Some(value) = get_str(attributes, name) {
        query.insert(name.to_string(), value.to_string());
    }
}

fn copy_true_attr(
    attributes: &BTreeMap<String, InputAttribute>,
    query: &mut BTreeMap<String, String>,
    name: &str,
) {
    if attributes
        .get(name)
        .and_then(InputAttribute::as_bool)
        .unwrap_or(false)
    {
        query.insert(name.to_string(), "1".to_string());
    }
}

fn attr_value_string(value: &InputAttribute) -> String {
    match value {
        InputAttribute::String(value) => value.clone(),
        InputAttribute::Integer(value) => value.to_string(),
        InputAttribute::Bool(value) => u8::from(*value).to_string(),
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
    use devenv::{InfoSections, InputMetadata};

    fn string_attributes(values: &[(&str, &str)]) -> BTreeMap<String, InputAttribute> {
        values
            .iter()
            .map(|(name, value)| {
                (
                    (*name).to_string(),
                    InputAttribute::String((*value).to_string()),
                )
            })
            .collect()
    }

    #[test]
    fn renders_inputs_and_environment_sections() {
        let metadata = Metadata {
            inputs: Some(vec![
                InputMetadata {
                    name: "follows".to_string(),
                    source: InputSource::Follows(vec!["github".to_string()]),
                },
                InputMetadata {
                    name: "github".to_string(),
                    source: InputSource::Locked(string_attributes(&[
                        ("narHash", "sha256-a/b+c="),
                        ("owner", "example"),
                        ("repo", "project"),
                        ("rev", "0123456789abcdef0123456789abcdef01234567"),
                        ("type", "github"),
                    ])),
                },
            ]),
            info_sections: InfoSections::from([("packages".to_string(), vec!["jq".to_string()])]),
        };

        assert_eq!(
            render(&metadata).unwrap(),
            concat!(
                "Inputs:\n",
                "├───follows: (follows)\n",
                "└───github: github:example/project/0123456789abcdef0123456789abcdef01234567?narHash=sha256-a/b%2Bc%3D\n\n",
                "# packages\n",
                "- jq"
            )
        );
    }

    #[test]
    fn renders_missing_lock_file() {
        let metadata = Metadata {
            inputs: None,
            info_sections: InfoSections::new(),
        };
        assert_eq!(render(&metadata).unwrap(), "Inputs:\n  (no lock file)");
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
