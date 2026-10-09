use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::identity::{cap_device_name, Fingerprint, Os};
use crate::peers::MAX_ADDRESSES;
use crate::{CoreError, Result};

pub const SERVICE_TYPE: &str = "_wisp._udp.local.";

#[derive(Debug, Clone, PartialEq)]
pub struct Advert {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub port: u16,
    pub fingerprint: Fingerprint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Discovered {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
    pub addresses: Vec<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryEvent {
    Found(Discovered),
    Lost(Uuid),
}

fn os_code(os: Os) -> &'static str {
    match os {
        Os::Windows => "win",
        Os::MacOs => "mac",
        Os::Linux => "linux",
    }
}

fn os_from_code(code: &str) -> Option<Os> {
    match code {
        "win" => Some(Os::Windows),
        "mac" => Some(Os::MacOs),
        "linux" => Some(Os::Linux),
        _ => None,
    }
}

pub fn txt_properties(a: &Advert) -> HashMap<String, String> {
    HashMap::from([
        ("v".to_string(), "1".to_string()),
        ("id".to_string(), a.device_id.to_string()),
        ("name".to_string(), cap_device_name(&a.name)),
        ("os".to_string(), os_code(a.os).to_string()),
        ("fp".to_string(), a.fingerprint.to_hex()),
    ])
}

fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

pub fn parse_txt(props: &HashMap<String, String>, ips: &[IpAddr], port: u16) -> Option<Discovered> {
    let device_id = Uuid::parse_str(props.get("id")?).ok()?;
    let name = cap_device_name(props.get("name")?);
    let os = os_from_code(props.get("os")?)?;
    let fingerprint = Fingerprint::from_hex(props.get("fp")?)?;
    let mut ips: Vec<IpAddr> = ips.iter().copied().filter(usable).collect();
    ips.sort_by_key(|ip| (ip.is_ipv6(), *ip));
    let addresses = ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect();
    Some(Discovered { device_id, name, os, fingerprint, addresses })
}

#[derive(Default)]
pub struct DiscoveredSet {
    map: HashMap<Uuid, Discovered>,
}

impl DiscoveredSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the device or one of its addresses is new. Addresses are capped at
    /// `MAX_ADDRESSES` with the latest advert first, so spoofed adverts cannot grow the dial
    /// fan-out without bound.
    pub fn merge(&mut self, mut d: Discovered) -> bool {
        match self.map.get_mut(&d.device_id) {
            None => {
                d.addresses.truncate(MAX_ADDRESSES);
                self.map.insert(d.device_id, d);
                true
            }
            Some(existing) => {
                existing.name = d.name;
                let changed = d.addresses.iter().any(|a| !existing.addresses.contains(a));
                let older: Vec<SocketAddr> =
                    existing.addresses.iter().copied().filter(|a| !d.addresses.contains(a)).collect();
                let mut addresses = d.addresses;
                addresses.extend(older);
                addresses.truncate(MAX_ADDRESSES);
                existing.addresses = addresses;
                changed
            }
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.map.remove(&id);
    }

    pub fn get(&self, id: Uuid) -> Option<&Discovered> {
        self.map.get(&id)
    }

    pub fn all(&self) -> Vec<&Discovered> {
        self.map.values().collect()
    }
}

pub struct Discovery {
    daemon: ServiceDaemon,
}

fn mdns_err(e: mdns_sd::Error) -> CoreError {
    CoreError::Transport(format!("mdns: {e}"))
}

fn service_info(advert: &Advert) -> std::result::Result<ServiceInfo, mdns_sd::Error> {
    let instance = advert.device_id.to_string();
    let host = format!("{instance}.local.");
    ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", advert.port, txt_properties(advert))
        .map(ServiceInfo::enable_addr_auto)
}

impl Discovery {
    pub fn start(advert: &Advert) -> Result<(Self, mpsc::UnboundedReceiver<DiscoveryEvent>)> {
        let daemon = ServiceDaemon::new().map_err(mdns_err)?;
        let registered = service_info(advert)
            .and_then(|info| daemon.register(info))
            .and_then(|()| daemon.browse(SERVICE_TYPE));
        let browse = match registered {
            Ok(browse) => browse,
            Err(e) => {
                // The daemon runs its own thread; stop it so a failed start leaves nothing behind.
                let _ = daemon.shutdown();
                return Err(mdns_err(e));
            }
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let own_id = advert.device_id;
        tokio::spawn(async move {
            while let Ok(event) = browse.recv_async().await {
                let out = match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let props: HashMap<String, String> = info
                            .get_properties()
                            .iter()
                            .map(|p| (p.key().to_string(), p.val_str().to_string()))
                            .collect();
                        let ips: Vec<IpAddr> = info.get_addresses().iter().copied().collect();
                        parse_txt(&props, &ips, info.get_port())
                            .filter(|d| d.device_id != own_id)
                            .map(DiscoveryEvent::Found)
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => fullname
                        .split('.')
                        .next()
                        .and_then(|id| Uuid::parse_str(id).ok())
                        .filter(|id| *id != own_id)
                        .map(DiscoveryEvent::Lost),
                    _ => None,
                };
                if let Some(e) = out {
                    if tx.send(e).is_err() {
                        break;
                    }
                }
            }
        });
        Ok((Self { daemon }, rx))
    }

    /// Re-announces this device with new details (for example a new name).
    pub fn update(&self, advert: &Advert) -> Result<()> {
        let info = service_info(advert).map_err(mdns_err)?;
        let fullname = info.get_fullname().to_string();
        // Commands run in order on the daemon thread, so the old record is gone before the new
        // one is announced.
        self.daemon.unregister(&fullname).map_err(mdns_err)?;
        self.daemon.register(info).map_err(mdns_err)
    }

    pub fn stop(self) {
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advert() -> Advert {
        Advert {
            device_id: Uuid::new_v4(),
            name: "Wolfgang's PC".into(),
            os: Os::Windows,
            port: 47800,
            fingerprint: Fingerprint([3; 32]),
        }
    }

    #[test]
    fn txt_round_trip() {
        let a = advert();
        let ips: Vec<IpAddr> = vec!["192.168.1.20".parse().unwrap()];
        let d = parse_txt(&txt_properties(&a), &ips, a.port).unwrap();
        assert_eq!(d.device_id, a.device_id);
        assert_eq!(d.name, a.name);
        assert_eq!(d.os, Os::Windows);
        assert_eq!(d.fingerprint, a.fingerprint);
        assert_eq!(d.addresses, vec!["192.168.1.20:47800".parse::<SocketAddr>().unwrap()]);
    }

    #[test]
    fn long_names_fit_in_one_txt_string() {
        let a = Advert { name: "\u{1f600}".repeat(200), ..advert() };
        for (key, value) in txt_properties(&a) {
            assert!(key.len() + 1 + value.len() <= 255, "{key} is {} bytes", value.len());
        }
        let mut props = txt_properties(&advert());
        props.insert("name".into(), "x".repeat(250));
        assert!(parse_txt(&props, &[], 1).unwrap().name.chars().count() <= 64);
    }

    #[test]
    fn parse_rejects_missing_or_bad_fields() {
        let a = advert();
        let mut props = txt_properties(&a);
        props.insert("fp".into(), "zz".into());
        assert!(parse_txt(&props, &[], a.port).is_none());
        let mut props = txt_properties(&a);
        props.remove("id");
        assert!(parse_txt(&props, &[], a.port).is_none());
    }

    #[test]
    fn parse_filters_loopback_and_link_local_and_puts_ipv4_first() {
        let a = advert();
        let ips: Vec<IpAddr> = vec![
            "fe80::1".parse().unwrap(),
            "127.0.0.1".parse().unwrap(),
            "fd00::20".parse().unwrap(),
            "10.0.0.5".parse().unwrap(),
        ];
        let d = parse_txt(&txt_properties(&a), &ips, a.port).unwrap();
        let ips: Vec<String> = d.addresses.iter().map(|s| s.ip().to_string()).collect();
        assert_eq!(ips, vec!["10.0.0.5", "fd00::20"]);
    }

    #[test]
    fn same_device_on_two_interfaces_is_one_entry() {
        let a = advert();
        let wifi = parse_txt(&txt_properties(&a), &["192.168.1.20".parse().unwrap()], a.port).unwrap();
        let cable = parse_txt(&txt_properties(&a), &["192.168.1.30".parse().unwrap()], a.port).unwrap();
        let mut set = DiscoveredSet::new();
        assert!(set.merge(wifi.clone()));
        assert!(set.merge(cable));
        assert!(!set.merge(wifi));
        assert_eq!(set.all().len(), 1);
        assert_eq!(set.get(a.device_id).unwrap().addresses.len(), 2);
        set.remove(a.device_id);
        assert!(set.all().is_empty());
    }

    #[test]
    fn merged_addresses_are_capped_with_the_latest_first() {
        let a = advert();
        let at = |i: u16| -> SocketAddr { format!("192.168.1.{i}:47800").parse().unwrap() };
        let mut set = DiscoveredSet::new();
        let many: Vec<IpAddr> = (1..=12).map(|i| format!("10.0.0.{i}").parse().unwrap()).collect();
        set.merge(parse_txt(&txt_properties(&a), &many, a.port).unwrap());
        assert_eq!(set.get(a.device_id).unwrap().addresses.len(), MAX_ADDRESSES);

        for i in 1..=20u16 {
            set.merge(Discovered { addresses: vec![at(i)], ..parse_txt(&txt_properties(&a), &[], a.port).unwrap() });
        }
        let addrs = &set.get(a.device_id).unwrap().addresses;
        assert_eq!(addrs.len(), MAX_ADDRESSES);
        assert_eq!(addrs[0], at(20));
        assert_eq!(addrs[MAX_ADDRESSES - 1], at(13));
    }

    #[tokio::test]
    #[ignore = "needs a real network with multicast"]
    async fn two_daemons_find_each_other() {
        let a = advert();
        let b = Advert { device_id: Uuid::new_v4(), name: "Mac".into(), os: Os::MacOs, port: 47801, ..advert() };
        let (da, mut rx_a) = Discovery::start(&a).unwrap();
        let (db, _rx_b) = Discovery::start(&b).unwrap();
        let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(DiscoveryEvent::Found(d)) = rx_a.recv().await {
                    if d.device_id == b.device_id {
                        return d;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(found.name, "Mac");
        da.stop();
        db.stop();
    }
}
