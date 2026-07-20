use devenv::Config;
use miette::{IntoDiagnostic, Result, WrapErr};
use schemars::schema_for;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const JSON_SCHEMA_PATH: &str = "docs/src/devenv.schema.json";
const YAML_OPTIONS_PATH: &str = "docs/src/reference/yaml-options.md";

pub fn default_json_schema_path() -> PathBuf {
    PathBuf::from(JSON_SCHEMA_PATH)
}

pub fn default_yaml_options_path() -> PathBuf {
    PathBuf::from(YAML_OPTIONS_PATH)
}

pub fn generate_json_schema(path: impl AsRef<Path>) -> Result<()> {
    let schema = schema_for!(Config);
    let schema = serde_json::to_string_pretty(&schema)
        .into_diagnostic()
        .wrap_err("Failed to serialize JSON schema")?;
    let path = path.as_ref();
    fs::write(path, &schema)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write JSON schema to {}", path.display()))
}

pub fn generate_yaml_options_doc(path: impl AsRef<Path>) -> Result<()> {
    let schema = schema_for!(Config);
    let json = serde_json::to_value(&schema)
        .into_diagnostic()
        .wrap_err("Failed to serialize JSON schema")?;
    let rendered = render_yaml_options(&json);
    let path = path.as_ref();
    fs::write(path, &rendered)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write YAML options to {}", path.display()))
}

struct OptionSection {
    path: String,
    description: String,
    added_in: Option<String>,
    default: Option<String>,
    type_label: String,
}

struct ParsedMeta {
    body: String,
    added_in: Option<String>,
    default: Option<String>,
    opaque: bool,
}

fn render_yaml_options(schema: &serde_json::Value) -> String {
    let defs = schema
        .get("$defs")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let mut sections: Vec<OptionSection> = Vec::new();
    let visited: HashSet<String> = HashSet::new();
    for (name, prop) in properties.iter() {
        collect_sections(name, prop, &defs, &visited, &mut sections);
    }
    sections.sort_by(|a, b| a.path.cmp(&b.path));

    let mut out = String::from("# devenv.yaml\n\n");
    out.push_str("<!-- This file is auto-generated from devenv-core/src/config.rs doc comments. Do not edit. -->\n\n");
    for section in sections {
        out.push_str(&render_section(&section));
    }
    out
}

/// Extract the `$ref` target name from a schema, considering direct `$ref` and `anyOf` wrappers.
fn ref_target(schema: &serde_json::Value) -> Option<String> {
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str())
        && let Some(name) = r.strip_prefix("#/$defs/")
    {
        return Some(name.to_string());
    }
    if let Some(any) = schema.get("anyOf").and_then(|v| v.as_array()) {
        let non_null: Vec<&serde_json::Value> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .collect();
        if non_null.len() == 1 {
            return ref_target(non_null[0]);
        }
    }
    None
}

fn collect_sections(
    path: &str,
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    visited: &HashSet<String>,
    out: &mut Vec<OptionSection>,
) {
    if schema.get("deprecated").and_then(|v| v.as_bool()) == Some(true) {
        return;
    }

    let resolved = resolve_ref(schema, defs);
    let description = description_of(schema).unwrap_or_default();
    let opaque = parse_description(&description).opaque;

    // Map types (BTreeMap<String, T>) -> emit "<path>.<name>.<sub>" sections via additionalProperties.
    if let Some(additional) = resolved.get("additionalProperties") {
        let wildcard_path = format!("{}.\\<name\\>", path);
        let inner_ref = ref_target(additional);
        let cycle = inner_ref
            .as_ref()
            .map(|name| visited.contains(name))
            .unwrap_or(false);
        let inner_resolved = resolve_ref(additional, defs);

        if !opaque
            && !cycle
            && inner_resolved
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some()
        {
            if !description.is_empty() {
                out.push(make_section(
                    path,
                    &type_label(&resolved, defs),
                    description,
                ));
            }
            let mut next = visited.clone();
            if let Some(name) = inner_ref {
                next.insert(name);
            }
            if let Some(props) = inner_resolved.get("properties").and_then(|v| v.as_object()) {
                for (name, sub) in props {
                    collect_sections(
                        &format!("{}.{}", wildcard_path, name),
                        sub,
                        defs,
                        &next,
                        out,
                    );
                }
            }
            return;
        }
        // Cycle, opaque, scalar value type, or no struct properties -> single section.
        out.push(make_section(
            path,
            &type_label(&resolved, defs),
            description,
        ));
        return;
    }

    // Object with properties (via $ref or inline) -> recurse.
    let inline_ref = ref_target(schema);
    let cycle = inline_ref
        .as_ref()
        .map(|name| visited.contains(name))
        .unwrap_or(false);
    if !opaque
        && !cycle
        && let Some(props) = resolved.get("properties").and_then(|v| v.as_object())
    {
        if !description.is_empty() {
            out.push(make_section(
                path,
                &type_label(&resolved, defs),
                description,
            ));
        }
        let mut next = visited.clone();
        if let Some(name) = inline_ref {
            next.insert(name);
        }
        for (name, sub) in props {
            collect_sections(&format!("{}.{}", path, name), sub, defs, &next, out);
        }
        return;
    }

    // Leaf scalar / enum / cycle / opaque.
    let desc = if description.is_empty() {
        description_of(&resolved).unwrap_or_default()
    } else {
        description
    };
    out.push(make_section(path, &type_label(&resolved, defs), desc));
}

fn make_section(path: &str, type_label: &str, raw_description: String) -> OptionSection {
    let meta = parse_description(&raw_description);
    OptionSection {
        path: path.to_string(),
        description: meta.body,
        added_in: meta.added_in,
        default: meta.default,
        type_label: type_label.to_string(),
    }
}

fn parse_description(input: &str) -> ParsedMeta {
    let mut lines: Vec<String> = input.lines().map(|l| l.to_string()).collect();
    let mut added_in: Option<String> = None;
    let mut default: Option<String> = None;
    let mut opaque = false;
    // Walk lines from the end, pulling off trailing metadata markers.
    while let Some(last) = lines.last().cloned() {
        let trimmed = last.trim();
        if trimmed.is_empty() {
            lines.pop();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Added in ") {
            added_in = Some(rest.trim_end_matches('.').to_string());
            lines.pop();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Default: ") {
            default = Some(rest.trim_end_matches('.').to_string());
            lines.pop();
            continue;
        }
        if trimmed == "Opaque." {
            opaque = true;
            lines.pop();
            continue;
        }
        break;
    }
    ParsedMeta {
        body: lines.join("\n").trim().to_string(),
        added_in,
        default,
        opaque,
    }
}

fn render_section(s: &OptionSection) -> String {
    let mut out = format!("## {}\n\n", s.path);
    if let Some(version) = &s.added_in {
        // The MkDocs hook moves this marker into reference headings after
        // generating their IDs and TOC entries (see hooks/added_in.py).
        out.push_str(&format!("[added-in:{}]\n\n", version));
    }
    if !s.description.is_empty() {
        out.push_str(&s.description);
        out.push_str("\n\n");
    }
    let mut meta = vec![format!("*Type:* {}", s.type_label)];
    if let Some(default) = &s.default {
        meta.push(format!("*Default:* {}", default));
    }
    out.push_str(&meta.join(" · "));
    out.push_str("\n\n");
    out
}

fn description_of(schema: &serde_json::Value) -> Option<String> {
    schema
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn resolve_ref(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    // Direct $ref
    if let Some(reference) = schema.get("$ref").and_then(|v| v.as_str())
        && let Some(name) = reference.strip_prefix("#/$defs/")
        && let Some(target) = defs.get(name)
    {
        return target.clone();
    }
    // anyOf with one $ref + null -> resolve the $ref.
    if let Some(any) = schema.get("anyOf").and_then(|v| v.as_array()) {
        let non_null: Vec<&serde_json::Value> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .collect();
        if non_null.len() == 1 {
            return resolve_ref(non_null[0], defs);
        }
    }
    schema.clone()
}

/// Returns a markdown-ready type expression including outer backticks.
fn type_label(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> String {
    format!("`{}`", type_label_inner(schema, defs))
}

fn type_label_inner(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let ref_name = ref_target(schema);
    let resolved = resolve_ref(schema, defs);

    if let Some(enum_values) = resolved.get("enum").and_then(|v| v.as_array()) {
        let values: Vec<String> = enum_values
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        return values.join(" | ");
    }

    if let Some(any) = resolved.get("anyOf").and_then(|v| v.as_array()) {
        let parts: Vec<String> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .map(|v| type_label_inner(v, defs))
            .collect();
        if !parts.is_empty() {
            return parts.join(" | ");
        }
    }

    if let Some(types) = resolved.get("type").and_then(|v| v.as_array()) {
        let non_null: Vec<&str> = types
            .iter()
            .filter_map(|t| t.as_str())
            .filter(|s| *s != "null")
            .collect();
        if non_null.len() == 1 {
            return scalar_label_inner(non_null[0], &resolved, defs, ref_name.as_deref());
        }
    }

    if let Some(ty) = resolved.get("type").and_then(|v| v.as_str()) {
        return scalar_label_inner(ty, &resolved, defs, ref_name.as_deref());
    }

    ref_name.unwrap_or_else(|| "unknown".to_string())
}

fn scalar_label_inner(
    ty: &str,
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    ref_name: Option<&str>,
) -> String {
    match ty {
        "boolean" => "boolean".to_string(),
        "string" => "string".to_string(),
        "integer" => "integer".to_string(),
        "number" => "number".to_string(),
        "array" => {
            let item_label = schema
                .get("items")
                .map(|i| type_label_inner(i, defs))
                .unwrap_or_else(|| "any".to_string());
            format!("list of {}", item_label)
        }
        "object" => {
            if let Some(additional) = schema.get("additionalProperties") {
                let inner_ref = ref_target(additional);
                let inner = inner_ref
                    .map(|n| humanize_ref_name(&n))
                    .unwrap_or_else(|| type_label_inner(additional, defs));
                format!("attribute set of {}", inner)
            } else if let Some(name) = ref_name {
                humanize_ref_name(name)
            } else {
                "attribute set".to_string()
            }
        }
        other => other.to_string(),
    }
}

/// `NixpkgsConfig` -> `nixpkgs config`, `Input` -> `input`.
fn humanize_ref_name(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        for low in ch.to_lowercase() {
            out.push(low);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trailing_description_metadata() {
        let parsed =
            parse_description("Visible description.\n\nDefault: true.\n\nAdded in 2.0.\n\nOpaque.");

        assert_eq!(parsed.body, "Visible description.");
        assert_eq!(parsed.default.as_deref(), Some("true"));
        assert_eq!(parsed.added_in.as_deref(), Some("2.0"));
        assert!(parsed.opaque);
    }

    #[test]
    fn renders_config_schema_as_option_reference() {
        let schema = serde_json::to_value(schema_for!(Config)).unwrap();
        let rendered = render_yaml_options(&schema);

        assert!(rendered.contains("## nixpkgs.allow_unfree\n"));
        assert!(rendered.contains("## inputs.\\<name\\>.url\n"));
        assert!(rendered.contains("## require_version\n"));
        assert!(rendered.contains("*Type:* `boolean | string`"));
        assert!(rendered.contains("[added-in:1.7]"));
        assert!(!rendered.contains("## allow_unfree\n"));
    }
}
