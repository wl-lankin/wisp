use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_CLIP_BYTES: u64 = 50 * 1024 * 1024;
pub const THUMB_MAX: u32 = 256;
const TEXT_PREVIEW_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Html,
    Image,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipPayload {
    Text(String),
    Html { html: String, plain: String },
    Image { png: Vec<u8>, width: u32, height: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Preview {
    Text(String),
    Thumbnail { png: Vec<u8>, width: u32, height: u32 },
    File { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipItem {
    pub id: Uuid,
    pub source: Uuid,
    pub created_ms: i64,
    pub payload: ClipPayload,
}

impl ClipItem {
    pub fn new(source: Uuid, payload: ClipPayload) -> Self {
        let created_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self { id: Uuid::new_v4(), source, created_ms, payload }
    }
}

impl ClipPayload {
    pub fn kind(&self) -> ItemKind {
        match self {
            ClipPayload::Text(_) => ItemKind::Text,
            ClipPayload::Html { .. } => ItemKind::Html,
            ClipPayload::Image { .. } => ItemKind::Image,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            ClipPayload::Text(t) => t.len() as u64,
            ClipPayload::Html { html, plain } => (html.len() + plain.len()) as u64,
            ClipPayload::Image { png, .. } => png.len() as u64,
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        match self {
            ClipPayload::Text(t) => {
                h.update(b"text\0");
                h.update(t.as_bytes());
            }
            ClipPayload::Html { html, .. } => {
                h.update(b"html\0");
                h.update(html.as_bytes());
            }
            ClipPayload::Image { png, .. } => {
                h.update(b"image\0");
                h.update(png);
            }
        }
        *h.finalize().as_bytes()
    }

    pub fn preview(&self) -> Preview {
        match self {
            ClipPayload::Text(t) => Preview::Text(t.chars().take(TEXT_PREVIEW_CHARS).collect()),
            ClipPayload::Html { plain, .. } => {
                Preview::Text(plain.chars().take(TEXT_PREVIEW_CHARS).collect())
            }
            ClipPayload::Image { png, width, height } => thumbnail(png, *width, *height),
        }
    }
}

fn thumbnail(png: &[u8], width: u32, height: u32) -> Preview {
    if width <= THUMB_MAX && height <= THUMB_MAX {
        return Preview::Thumbnail { png: png.to_vec(), width, height };
    }
    let Ok(img) = image::load_from_memory(png) else {
        return Preview::Text(format!("Image {width} x {height}"));
    };
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX);
    let mut out = std::io::Cursor::new(Vec::new());
    if thumb.write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Preview::Text(format!("Image {width} x {height}"));
    }
    Preview::Thumbnail { png: out.into_inner(), width: thumb.width(), height: thumb.height() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([45, 226, 197, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn text_preview_is_truncated_to_200_chars() {
        let p = ClipPayload::Text("a".repeat(500)).preview();
        match p {
            Preview::Text(t) => assert_eq!(t.chars().count(), 200),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn image_preview_fits_256() {
        let payload = ClipPayload::Image { png: png(1024, 512), width: 1024, height: 512 };
        match payload.preview() {
            Preview::Thumbnail { width, height, .. } => {
                assert_eq!(width, 256);
                assert_eq!(height, 128);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn small_image_preview_is_not_upscaled() {
        let payload = ClipPayload::Image { png: png(40, 20), width: 40, height: 20 };
        match payload.preview() {
            Preview::Thumbnail { width, height, .. } => assert_eq!((width, height), (40, 20)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hash_differs_by_content_and_kind() {
        let a = ClipPayload::Text("hi".into());
        let b = ClipPayload::Text("ho".into());
        let c = ClipPayload::Html { html: "hi".into(), plain: "hi".into() };
        assert_ne!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        assert_eq!(a.hash(), ClipPayload::Text("hi".into()).hash());
    }
}
