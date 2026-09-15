use crate::TlsConfig;
#[cfg(feature = "server")]
use crate::certificates::Certificate;
use anyhow::{Result, bail};
#[cfg(feature = "server")]
use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
#[cfg(feature = "server")]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub hostname: String,
    pub upstream: SocketAddr,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsConfig>,
}

#[derive(Clone)]
#[cfg(feature = "server")]
struct RegisteredRoute {
    route: Route,
    certificate: Option<Arc<Certificate>>,
}

#[cfg(feature = "server")]
impl RegisteredRoute {
    fn new(mut route: Route) -> Result<Self> {
        route.hostname = normalize_hostname(&route.hostname)?;
        if route.owner.trim().is_empty() {
            bail!("route owner cannot be empty");
        }
        if !route.upstream.ip().is_loopback() {
            bail!("route upstream must use a loopback address");
        }
        let certificate = route.tls.as_ref().map(Certificate::load).transpose()?;
        if certificate
            .as_ref()
            .is_some_and(|certificate| !certificate.covers(&route.hostname))
        {
            bail!("proxy certificate does not cover {}", route.hostname);
        }
        Ok(Self {
            route,
            certificate: certificate.map(Arc::new),
        })
    }
}

#[derive(Clone, Default)]
#[cfg(feature = "server")]
pub struct RouteTable {
    routes: Arc<ArcSwap<BTreeMap<String, RegisteredRoute>>>,
    updates: Arc<Mutex<()>>,
    pub(crate) https_listen: Option<SocketAddr>,
}

#[cfg(feature = "server")]
impl RouteTable {
    pub fn register(&self, route: Route) -> Result<()> {
        let registered = RegisteredRoute::new(route)?;
        let route = &registered.route;

        let _update = self
            .updates
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = self.routes.load();
        if current
            .get(&route.hostname)
            .is_some_and(|existing| existing.route.owner != route.owner)
        {
            bail!(
                "hostname {} is already owned by another project",
                route.hostname
            );
        }
        let mut next = (**current).clone();
        next.insert(route.hostname.clone(), registered);
        self.routes.store(Arc::new(next));
        Ok(())
    }

    pub fn unregister(&self, hostname: &str, owner: &str) -> Result<bool> {
        let hostname = normalize_hostname(hostname)?;
        let _update = self
            .updates
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = self.routes.load();
        let Some(existing) = current.get(&hostname) else {
            return Ok(false);
        };
        if existing.route.owner != owner {
            bail!("hostname {hostname} is owned by another project");
        }
        let mut next = (**current).clone();
        next.remove(&hostname);
        self.routes.store(Arc::new(next));
        Ok(true)
    }

    /// Atomically replace every route belonging to `owner`.
    ///
    /// `devenv up` uses this to reconcile the shared proxy with the project's
    /// current process configuration, including removing routes that vanished
    /// since the previous evaluation.
    pub fn replace_owner(&self, owner: &str, routes: Vec<Route>) -> Result<()> {
        if owner.trim().is_empty() {
            bail!("route owner cannot be empty");
        }

        let mut replacement = BTreeMap::new();
        for route in routes {
            if route.owner != owner {
                bail!("replacement route has a different owner");
            }
            let registered = RegisteredRoute::new(route)?;
            if replacement
                .insert(registered.route.hostname.clone(), registered)
                .is_some()
            {
                bail!("replacement contains a duplicate hostname");
            }
        }

        let _update = self
            .updates
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = self.routes.load();
        for hostname in replacement.keys() {
            if current
                .get(hostname)
                .is_some_and(|existing| existing.route.owner != owner)
            {
                bail!("hostname {hostname} is already owned by another project");
            }
        }

        let mut next = (**current).clone();
        next.retain(|_, route| route.route.owner != owner);
        next.extend(replacement);
        self.routes.store(Arc::new(next));
        Ok(())
    }

    pub fn resolve(&self, hostname: &str) -> Option<SocketAddr> {
        let hostname = normalize_hostname(hostname).ok()?;
        self.routes
            .load()
            .get(&hostname)
            .map(|route| route.route.upstream)
    }

    pub fn list(&self) -> Vec<Route> {
        self.routes
            .load()
            .values()
            .map(|route| route.route.clone())
            .collect()
    }

    pub(crate) fn certificate(&self, hostname: &str) -> Option<Arc<Certificate>> {
        let hostname = normalize_hostname(hostname).ok()?;
        self.routes.load().get(&hostname)?.certificate.clone()
    }
}

pub fn normalize_hostname(hostname: &str) -> Result<String> {
    let hostname = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    if hostname != "localhost" && !hostname.ends_with(".localhost") {
        bail!("route hostname must be localhost or end in .localhost");
    }
    if hostname
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.'))
    {
        bail!("route hostname contains invalid characters");
    }
    if hostname
        .split('.')
        .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
    {
        bail!("route hostname contains an invalid label");
    }
    Ok(hostname)
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;

    fn route(hostname: &str, port: u16, owner: &str) -> Route {
        Route {
            hostname: hostname.to_owned(),
            upstream: SocketAddr::from(([127, 0, 0, 1], port)),
            owner: owner.to_owned(),
            tls: None,
        }
    }

    #[test]
    fn registration_is_normalized_and_idempotent() {
        let table = RouteTable::default();
        table
            .register(route("WEB.Demo.Localhost.", 3000, "demo"))
            .unwrap();
        table
            .register(route("web.demo.localhost", 3001, "demo"))
            .unwrap();

        assert_eq!(
            table.resolve("WEB.demo.localhost"),
            Some(SocketAddr::from(([127, 0, 0, 1], 3001)))
        );
        assert_eq!(table.list().len(), 1);
    }

    #[test]
    fn replacement_reconciles_only_the_owners_routes() {
        let table = RouteTable::default();
        table
            .register(route("api.other.localhost", 9000, "other"))
            .unwrap();
        table
            .register(route("old.demo.localhost", 8000, "demo"))
            .unwrap();

        table
            .replace_owner("demo", vec![route("new.demo.localhost", 8001, "demo")])
            .unwrap();

        assert_eq!(table.resolve("old.demo.localhost"), None);
        assert_eq!(
            table.resolve("new.demo.localhost"),
            Some(SocketAddr::from(([127, 0, 0, 1], 8001)))
        );
        assert_eq!(
            table.resolve("api.other.localhost"),
            Some(SocketAddr::from(([127, 0, 0, 1], 9000)))
        );
    }

    #[test]
    fn replacement_rejects_routes_owned_by_someone_else() {
        let table = RouteTable::default();
        let error = table
            .replace_owner("demo", vec![route("web.demo.localhost", 8000, "other")])
            .unwrap_err();
        assert!(error.to_string().contains("different owner"));
    }

    #[test]
    fn owners_cannot_replace_or_remove_each_others_routes() {
        let table = RouteTable::default();
        table
            .register(route("web.demo.localhost", 3000, "demo"))
            .unwrap();

        assert!(
            table
                .register(route("web.demo.localhost", 4000, "other"))
                .is_err()
        );
        assert!(table.unregister("web.demo.localhost", "other").is_err());
        assert_eq!(
            table.resolve("web.demo.localhost"),
            Some(SocketAddr::from(([127, 0, 0, 1], 3000)))
        );
    }

    #[test]
    fn only_local_routes_are_allowed() {
        let table = RouteTable::default();
        assert!(table.register(route("example.com", 3000, "demo")).is_err());
        let mut public = route("web.demo.localhost", 3000, "demo");
        public.upstream = SocketAddr::from(([192, 0, 2, 1], 3000));
        assert!(table.register(public).is_err());
    }
}
