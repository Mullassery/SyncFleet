//! mDNS peer discovery (Phase 5.1).
//!
//! Advertises this node as a `_syncfleet._tcp.local.` service and browses the
//! local network for other SyncFleet nodes. Discovery is purely an *address*
//! hint: proven identity still comes from the mTLS handshake and certificate
//! pinning, so a spoofed advertisement cannot impersonate a peer.
//!
//! The module is compiled only when the `mdns` feature is enabled.

use crate::config::DiscoveryConfig;
use crate::error::{DiscoveryError, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use std::thread::JoinHandle;
use tokio::sync::mpsc;

/// TXT record key holding the node id (`node_<uuid>`).
pub const TXT_NODE_ID: &str = "node_id";
/// TXT record key holding the human-readable node name.
pub const TXT_NODE_NAME: &str = "node_name";
/// TXT record key holding the protocol version.
pub const TXT_VERSION: &str = "ver";

/// A peer discovered via mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub node_id: String,
    pub node_name: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
}

impl DiscoveredPeer {
    /// The best `host:port` to dial: the first advertised IP if any, otherwise
    /// the resolved `.local.` hostname.
    pub fn address(&self) -> String {
        match self.addresses.first() {
            Some(ip) => format!("{ip}:{}", self.port),
            None => format!("{}:{}", self.host, self.port),
        }
    }
}

/// Stable mDNS instance name for a node (`syncfleet-<short-id>`).
pub fn instance_name(node_id: &str) -> String {
    let short = node_id.strip_prefix("node_").unwrap_or(node_id);
    let short: String = short
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect();
    format!("syncfleet-{short}")
}

/// Advertise this node and own the mDNS daemon for its lifetime.
pub struct Discovery {
    daemon: ServiceDaemon,
    service_type: String,
    browse_task: Option<JoinHandle<()>>,
}

impl Discovery {
    /// Start advertising `this` node on `service_type` at `port`.
    ///
    /// `versions`/`datasets` are not advertised: peers learn authorization
    /// through the normal handshake, keeping the TXT record small and stable.
    pub fn start(
        cfg: &DiscoveryConfig,
        node_id: &str,
        node_name: &str,
        port: u16,
        version: &str,
    ) -> Result<Self> {
        if !cfg.enabled {
            return Err(DiscoveryError::Disabled.into());
        }

        let daemon = ServiceDaemon::new().map_err(DiscoveryError::from)?;
        let instance = instance_name(node_id);
        let host = format!("{}.local.", instance);

        let mut props: HashMap<String, String> = HashMap::new();
        props.insert(TXT_NODE_ID.to_string(), node_id.to_string());
        props.insert(TXT_NODE_NAME.to_string(), node_name.to_string());
        props.insert(TXT_VERSION.to_string(), version.to_string());

        let service = ServiceInfo::new(&cfg.service_name, &instance, &host, (), port, props)
            .map_err(DiscoveryError::from)?
            .enable_addr_auto();
        daemon
            .register(service)
            .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        Ok(Self {
            daemon,
            service_type: cfg.service_name.clone(),
            browse_task: None,
        })
    }

    /// Begin browsing for peers. Returns a channel that yields a
    /// [`DiscoveredPeer`] for every other SyncFleet node that resolves.
    pub fn browse(
        &mut self,
        local_node_id: String,
    ) -> Result<mpsc::UnboundedReceiver<DiscoveredPeer>> {
        let events = self
            .daemon
            .browse(&self.service_type)
            .map_err(DiscoveryError::from)?;
        let (tx, rx) = mpsc::unbounded_channel();

        let task = std::thread::spawn(move || {
            for event in events {
                if let ServiceEvent::ServiceResolved(info) = event {
                    let node_id = info
                        .get_property_val_str(TXT_NODE_ID)
                        .unwrap_or_default()
                        .to_string();
                    if node_id.is_empty() || node_id == local_node_id {
                        continue;
                    }
                    let peer = DiscoveredPeer {
                        node_id,
                        node_name: info
                            .get_property_val_str(TXT_NODE_NAME)
                            .unwrap_or_default()
                            .to_string(),
                        host: info.get_hostname().to_string(),
                        port: info.get_port(),
                        addresses: info.get_addresses().iter().copied().collect(),
                    };
                    if tx.send(peer).is_err() {
                        break;
                    }
                }
            }
        });
        self.browse_task = Some(task);

        Ok(rx)
    }

    /// Stop advertising and shut the mDNS daemon down.
    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.daemon.stop_browse(&self.service_type);
        let _ = self.daemon.shutdown();
        if let Some(task) = self.browse_task.take() {
            let _ = task.join();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_name_is_stable_and_compact() {
        assert_eq!(instance_name("node_abc123"), "syncfleet-abc123");
        assert_eq!(
            instance_name("node_018f-6a7b-zzzz"),
            "syncfleet-018f6a7bzzzz"
        );
        // Long ids are truncated to keep the DNS label within limits.
        assert_eq!(
            instance_name(&"x".repeat(40)).len(),
            "syncfleet-".len() + 12
        );
    }

    #[test]
    fn address_prefers_ip_then_hostname() {
        let with_ip = DiscoveredPeer {
            node_id: "node_a".into(),
            node_name: "a".into(),
            host: "a.local.".into(),
            port: 8443,
            addresses: vec!["10.0.0.5".parse().unwrap()],
        };
        assert_eq!(with_ip.address(), "10.0.0.5:8443");

        let host_only = DiscoveredPeer {
            addresses: vec![],
            ..with_ip
        };
        assert_eq!(host_only.address(), "a.local.:8443");
    }
}
