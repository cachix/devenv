//! Conservative analysis of declared configuration, not a reachability probe.
use std::collections::{BTreeMap, BTreeSet};

use miette::{IntoDiagnostic, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Facts {
    pub version: u64,
    pub hostname: String,
    pub ssh: Ssh,
    pub firewall: Firewall,
    pub recovery_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Ssh {
    pub enabled: bool,
    pub ports: Vec<u16>,
    pub root_login: String,
    pub admin_keys: BTreeMap<String, Vec<String>>,
    pub dynamic_keys: bool,
    pub custom_config: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Firewall {
    pub enabled: bool,
    #[serde(rename = "allowedTCPPorts")]
    pub allowed_tcp_ports: Vec<u16>,
    #[serde(rename = "allowedTCPPortRanges")]
    pub allowed_tcp_port_ranges: Vec<PortRange>,
    pub custom_rules: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortRange {
    pub from: u16,
    pub to: u16,
}

impl Facts {
    pub fn parse(input: &str) -> Result<Self> {
        let facts: Self = serde_json::from_str(input).into_diagnostic()?;
        facts.validate()?;
        Ok(facts)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported machine facts version: {}", self.version);
        }
        if self.ssh.ports.contains(&0)
            || self.firewall.allowed_tcp_ports.contains(&0)
            || self
                .firewall
                .allowed_tcp_port_ranges
                .iter()
                .any(|r| r.from == 0 || r.from > r.to)
        {
            bail!("Invalid port in machine facts");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub before: Value,
    pub after: Value,
}

pub(super) fn analyze(current: Option<&Facts>, requested: &Facts) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut add = |code: &str, severity, message: &str, before, after| {
        findings.push(Finding {
            code: code.into(),
            severity,
            message: message.into(),
            before,
            after,
        });
    };
    if !requested.ssh.enabled {
        add(
            "ssh-disabled",
            Severity::Error,
            "Requested configuration disables SSH",
            current.map_or(Value::Null, |c| json!(c.ssh.enabled)),
            json!(false),
        );
    }
    if requested.ssh.root_login == "no" {
        add(
            "root-login-disabled",
            Severity::Error,
            "Requested configuration disables root SSH login required by transactional deployment",
            current.map_or(Value::Null, |c| json!(c.ssh.root_login)),
            json!("no"),
        );
    }
    if !requested.recovery_enabled {
        add(
            "recovery-disabled",
            Severity::Warning,
            "Requested configuration does not enable reboot recovery",
            current.map_or(Value::Null, |c| json!(c.recovery_enabled)),
            json!(false),
        );
    }
    if let Some(current) = current {
        let old_ports: BTreeSet<_> = current.ssh.ports.iter().collect();
        let new_ports: BTreeSet<_> = requested.ssh.ports.iter().collect();
        if old_ports != new_ports {
            add(
                "ssh-ports-changed",
                Severity::Warning,
                "SSH listening ports change; the controller still uses the planned SSH destination",
                json!(old_ports),
                json!(new_ports),
            );
        }
        // Compare per user: moving a root key to a different account is removal.
        let removed: BTreeMap<_, Vec<_>> = current
            .ssh
            .admin_keys
            .iter()
            .filter_map(|(user, keys)| {
                let new = requested.ssh.admin_keys.get(user);
                let missing: Vec<_> = keys
                    .iter()
                    .filter(|key| !new.is_some_and(|keys| keys.contains(key)))
                    .collect();
                (!missing.is_empty()).then_some((user, missing))
            })
            .collect();
        if !removed.is_empty() {
            add(
                "admin-keys-removed",
                Severity::Warning,
                "Declared administrator SSH keys are removed; other key sources may still grant access",
                json!(removed),
                Value::Null,
            );
        }
        if current.hostname != requested.hostname {
            add(
                "hostname-changed",
                Severity::Warning,
                "Declared hostname changes; this alone does not establish target identity",
                json!(current.hostname),
                json!(requested.hostname),
            );
        }
    } else {
        add(
            "current-facts-unavailable",
            Severity::Warning,
            "Current configuration facts are unavailable; before/after access comparison is incomplete",
            Value::Null,
            Value::Null,
        );
    }
    let ports: BTreeSet<_> = requested
        .ssh
        .ports
        .iter()
        .chain(current.into_iter().flat_map(|c| c.ssh.ports.iter()))
        .copied()
        .collect();
    let blocked: Vec<_> = ports
        .into_iter()
        .filter(|port| {
            requested.firewall.enabled
                && !requested.firewall.allowed_tcp_ports.contains(port)
                && !requested
                    .firewall
                    .allowed_tcp_port_ranges
                    .iter()
                    .any(|r| r.from <= *port && *port <= r.to)
        })
        .collect();
    if !blocked.is_empty() {
        add(
            "ssh-firewall-risk",
            Severity::Warning,
            "Declared global firewall allowances omit SSH ports; interface rules, custom rules, and external firewalls are not proven by this analysis",
            json!(blocked),
            json!(requested.firewall),
        );
    }
    if requested.ssh.dynamic_keys
        || requested.ssh.custom_config
        || requested.firewall.custom_rules
        || current
            .is_some_and(|c| c.ssh.dynamic_keys || c.ssh.custom_config || c.firewall.custom_rules)
    {
        add(
            "access-analysis-incomplete",
            Severity::Warning,
            "Dynamic key sources or custom SSH/firewall configuration require manual review; declared facts do not prove reachability",
            Value::Null,
            Value::Null,
        );
    }
    findings
}

pub(super) fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}

pub(super) fn summary(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            format!(
                "  {} [{}]: {}\n",
                if f.severity == Severity::Error {
                    "ERROR"
                } else {
                    "Warning"
                },
                f.code,
                f.message
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn facts() -> Facts {
        Facts::parse(r#"{"version":1,"hostname":"server","ssh":{"enabled":true,"ports":[22],"rootLogin":"prohibit-password","adminKeys":{"root":["public-key"]},"dynamicKeys":false,"customConfig":false},"firewall":{"enabled":true,"allowedTCPPorts":[],"allowedTCPPortRanges":[{"from":20,"to":25}],"customRules":false},"recoveryEnabled":true}"#).unwrap()
    }
    #[test]
    fn unchanged_facts_and_allowed_ranges_are_clean() {
        let f = facts();
        assert!(analyze(Some(&f), &f).is_empty());
    }
    #[test]
    fn missing_current_facts_does_not_hide_requested_errors() {
        let mut f = facts();
        f.ssh.enabled = false;
        f.ssh.root_login = "no".into();
        let findings = analyze(None, &f);
        assert!(has_errors(&findings));
        assert_eq!(
            findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(),
            [
                "ssh-disabled",
                "root-login-disabled",
                "current-facts-unavailable"
            ]
        );
    }
    #[test]
    fn reports_changes_and_uncertainty_without_claiming_identity_or_reachability() {
        let old = facts();
        let mut new = old.clone();
        new.hostname = "renamed".into();
        new.ssh.ports = vec![2222];
        new.ssh.admin_keys = BTreeMap::from([("other".into(), vec!["public-key".into()])]);
        new.ssh.dynamic_keys = true;
        new.firewall.custom_rules = true;
        new.recovery_enabled = false;
        let findings = analyze(Some(&old), &new);
        assert!(!has_errors(&findings));
        for code in [
            "ssh-ports-changed",
            "admin-keys-removed",
            "hostname-changed",
            "ssh-firewall-risk",
            "recovery-disabled",
            "access-analysis-incomplete",
        ] {
            assert!(findings.iter().any(|f| f.code == code), "{code}");
        }
    }
    #[test]
    fn rejects_unknown_schema_and_invalid_ranges() {
        let mut f = facts();
        f.version = 2;
        assert!(f.validate().is_err());
        f.version = 1;
        f.firewall.allowed_tcp_port_ranges[0].from = 26;
        assert!(f.validate().is_err());
    }
}
