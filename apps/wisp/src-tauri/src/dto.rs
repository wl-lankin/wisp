use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use wisp_core::engine::Event;
use wisp_core::item::Preview;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SmartKind {
    Plain,
    Link { domain: String },
    Color { value: String },
    Code,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PreviewDto {
    Text { text: String, smart: SmartKind },
    Image { data_url: String, width: u32, height: u32 },
    File { name: String },
}

const CODE_MARKERS: [&str; 14] = [
    ";\n", "{\n", "}", "=>", "fn ", "function ", "def ", "class ", "import ", "const ", "let ", "</", "#include", "<?php",
];

pub fn classify(text: &str) -> SmartKind {
    let t = text.trim();
    if let Some(domain) = link_domain(t) {
        return SmartKind::Link { domain };
    }
    if is_color(t) {
        return SmartKind::Color { value: t.to_string() };
    }
    if t.lines().count() >= 2 && CODE_MARKERS.iter().filter(|m| t.contains(*m)).count() >= 2 {
        return SmartKind::Code;
    }
    SmartKind::Plain
}

fn link_domain(t: &str) -> Option<String> {
    if t.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // Anything before the last '@' is userinfo, which can be dressed up to look like a host.
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, _)| h);
    (!host.is_empty()).then(|| host.to_lowercase())
}

fn is_color(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = t.to_ascii_lowercase();
    let Some(inner) = ["rgb(", "rgba(", "hsl(", "hsla("]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    // Only numbers and their separators, so nothing like url(...) or var(...) can ride along.
    let plain = inner.replace("deg", "");
    plain.chars().any(|c| c.is_ascii_digit())
        && plain.chars().all(|c| c.is_ascii_digit() || matches!(c, ' ' | '.' | ',' | '%' | '/'))
}

pub fn preview_dto(p: &Preview) -> PreviewDto {
    match p {
        Preview::Text(text) => PreviewDto::Text { text: text.clone(), smart: classify(text) },
        Preview::Thumbnail { png, width, height } => PreviewDto::Image {
            data_url: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
            width: *width,
            height: *height,
        },
        Preview::File { name } => PreviewDto::File { name: name.clone() },
    }
}

/// Serializes `value` and swaps its raw `preview` field for the UI preview.
pub fn with_ui_preview<T: Serialize>(value: &T, preview: &Preview) -> Value {
    let mut v = serde_json::to_value(value).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut v {
        map.insert("preview".into(), serde_json::to_value(preview_dto(preview)).unwrap_or(Value::Null));
    }
    v
}

pub fn event_to_ui(e: &Event) -> Value {
    match e {
        Event::OfferReceived(info) => with_ui_preview(e, &info.preview),
        other => serde_json::to_value(other).unwrap_or(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use wisp_core::engine::OfferInfo;
    use wisp_core::item::ItemKind;

    #[test]
    fn classifies_links_colors_code_and_plain_text() {
        assert_eq!(
            classify("https://www.example.org/path?q=1"),
            SmartKind::Link { domain: "www.example.org".into() }
        );
        assert_eq!(classify("  http://192.168.1.20:8080/x  "), SmartKind::Link { domain: "192.168.1.20".into() });
        assert_eq!(classify("#2DE2C5"), SmartKind::Color { value: "#2DE2C5".into() });
        assert_eq!(classify("#abc"), SmartKind::Color { value: "#abc".into() });
        assert_eq!(classify("rgb(45, 226, 197)"), SmartKind::Color { value: "rgb(45, 226, 197)".into() });
        assert_eq!(classify("fn main() {\n    println!(\"hi\");\n}"), SmartKind::Code);
        assert_eq!(classify("const a = 1;\nconst b = 2;"), SmartKind::Code);
        assert_eq!(classify("Meeting at 3, bring snacks"), SmartKind::Plain);
        assert_eq!(classify("see https://example.org for details"), SmartKind::Plain);
        assert_eq!(classify("#hashtag"), SmartKind::Plain);
    }

    #[test]
    fn link_domains_ignore_userinfo() {
        assert_eq!(classify("https://good.com:x@evil.com/"), SmartKind::Link { domain: "evil.com".into() });
        assert_eq!(classify("https://user@example.org:8443/a"), SmartKind::Link { domain: "example.org".into() });
        assert_eq!(classify("https://a@b@host.net"), SmartKind::Link { domain: "host.net".into() });
    }

    #[test]
    fn colors_only_accept_plain_numeric_functions() {
        assert!(is_color("rgba(45, 226, 197, 0.5)"));
        assert!(is_color("rgb(45 226 197 / 50%)"));
        assert!(is_color("hsl(170deg, 76%, 53%)"));
        assert!(is_color("HSLA(170, 76%, 53%, .4)"));
        assert!(!is_color("rgb(url(https://evil.example/x))"));
        assert!(!is_color("rgb(var(--x))"));
        assert!(!is_color("rgb()"));
        assert!(!is_color("rgb(1,2,3);background:red)"));
    }

    #[test]
    fn thumbnails_become_data_urls() {
        let p = Preview::Thumbnail { png: vec![1, 2, 3], width: 10, height: 5 };
        assert_eq!(
            preview_dto(&p),
            PreviewDto::Image { data_url: "data:image/png;base64,AQID".into(), width: 10, height: 5 }
        );
    }

    #[test]
    fn offer_events_carry_ui_previews_and_no_raw_bytes() {
        let info = OfferInfo {
            peer: Uuid::new_v4(),
            item_id: Uuid::new_v4(),
            kind: ItemKind::Image,
            size: 50_000_000,
            preview: Preview::Thumbnail { png: vec![9; 64], width: 256, height: 128 },
            age_ms: 0,
        };
        let v = event_to_ui(&Event::OfferReceived(info.clone()));
        assert_eq!(v["type"], "offerReceived");
        assert_eq!(v["itemId"], info.item_id.to_string());
        assert_eq!(v["preview"]["kind"], "image");
        assert!(v["preview"]["dataUrl"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert!(!v.to_string().contains("\"png\""));
    }

    #[test]
    fn other_events_pass_through_unchanged() {
        let peer = Uuid::new_v4();
        let v = event_to_ui(&Event::PeerOffline { peer });
        assert_eq!(v, serde_json::json!({ "type": "peerOffline", "peer": peer.to_string() }));
    }
}
