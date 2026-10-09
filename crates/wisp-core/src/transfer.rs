use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::protocol::{read_frame, write_frame, Message};
use crate::{CoreError, Result};

pub const CHUNK: usize = 256 * 1024;
pub const RESUME_WINDOW: Duration = Duration::from_secs(600);
pub const PARTIAL_DIR: &str = ".wisp-partial";
const MAX_COMPONENT_BYTES: usize = 255;
const RESERVED:[&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn sanitize_component(c: &str) -> Option<String> {
    let cleaned: String = c
        .chars()
        .filter(|ch| !ch.is_control() && !"<>:\"|?*".contains(*ch))
        .collect();
    let trimmed = cleaned.trim_matches(|ch: char| ch == '.' || ch == ' ').to_string();
    if trimmed.is_empty() {
        return None;
    }
    let stem = trimmed.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let trimmed = truncate_component(&trimmed);
    let safe = if RESERVED.contains(&stem.as_str()) {
        truncate_component(&format!("_{trimmed}"))
    } else {
        trimmed
    };
    if safe.is_empty() {
        None
    } else {
        Some(safe)
    }
}

/// Cuts a name to at most 255 UTF-8 bytes on a char boundary, then drops trailing
/// dots and spaces again because Windows strips them.
fn truncate_component(name: &str) -> String {
    let mut end = name.len().min(MAX_COMPONENT_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].trim_end_matches(['.', ' ']).to_string()
}

pub fn sanitize_relative_path(name: &str) -> PathBuf {
    let parts: Vec<String> = name
        .split(['/', '\\'])
        .filter(|p| *p != "..")
        .filter_map(sanitize_component)
        .collect();
    if parts.is_empty() {
        return PathBuf::from("file");
    }
    parts.iter().collect()
}

fn numbered_candidate(path: &Path, stem: &str, ext: Option<&str>, n: u32) -> PathBuf {
    let name = match ext {
        Some(ext) => format!("{stem} ({n}).{ext}"),
        None => format!("{stem} ({n})"),
    };
    path.with_file_name(name)
}

/// Atomically claims a free file name by creating an empty placeholder with
/// `create_new`, so concurrent transfers can never pick the same target.
async fn reserve_path(path: &Path) -> Result<PathBuf> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    let mut candidate = path.to_path_buf();
    for n in 1u32.. {
        match tokio::fs::OpenOptions::new().write(true).create_new(true).open(&candidate).await {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                candidate = numbered_candidate(path, stem, ext, n);
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1.. {
        let candidate = numbered_candidate(path, stem, ext, n);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn expand_paths(paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), &name, out)?;
            } else {
                out.push((entry.path(), name));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for path in paths {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
        if path.is_dir() {
            walk(path, &name, &mut out)?;
        } else {
            out.push((path.clone(), name));
        }
    }
    Ok(out)
}

async fn hash_file(path: &Path) -> Result<([u8; 32], u64)> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((*hasher.finalize().as_bytes(), total))
}

pub async fn send_file<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    transfer_id: Uuid,
    path: &Path,
    name: &str,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<()> {
    let (hash, size) = hash_file(path).await?;
    write_frame(stream, &Message::FileStart { transfer_id, name: name.to_string(), size, hash }).await?;
    let mut offset = match read_frame(stream).await? {
        Message::FileAck { offset, .. } if offset <= size => offset,
        other => return Err(CoreError::Transfer(format!("unexpected reply {other:?}"))),
    };

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buf = vec![0u8; CHUNK];
    while offset < size {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        write_frame(stream, &Message::FileChunk { transfer_id, offset, bytes: buf[..n].to_vec() }).await?;
        offset += n as u64;
        progress(offset);
    }
    write_frame(stream, &Message::FileDone { transfer_id }).await?;
    match read_frame(stream).await? {
        Message::FileAck { offset, .. } if offset == size => Ok(()),
        _ => Err(CoreError::Transfer("checksum mismatch on the receiving side".into())),
    }
}

pub async fn receive_file<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    save_dir: &Path,
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<PathBuf> {
    let (transfer_id, name, size, hash) = match read_frame(stream).await? {
        Message::FileStart { transfer_id, name, size, hash } => (transfer_id, name, size, hash),
        other => return Err(CoreError::Transfer(format!("expected FileStart, got {other:?}"))),
    };
    let partial_dir = save_dir.join(PARTIAL_DIR);
    tokio::fs::create_dir_all(&partial_dir).await?;
    let partial = partial_dir.join(format!("{transfer_id}.part"));
    let mut offset = match tokio::fs::metadata(&partial).await {
        Ok(m) if m.len() <= size => m.len(),
        _ => 0,
    };
    let mut file = tokio::fs::OpenOptions::new().create(true).write(true).open(&partial).await?;
    file.set_len(offset).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    write_frame(stream, &Message::FileAck { transfer_id, offset }).await?;

    let mut oversized = false;
    loop {
        match read_frame(stream).await? {
            Message::FileChunk { offset: at, bytes, .. } if at == offset => {
                let end = if bytes.len() <= CHUNK {
                    offset.checked_add(bytes.len() as u64).filter(|end| *end <= size)
                } else {
                    None
                };
                let Some(end) = end else {
                    oversized = true;
                    break;
                };
                file.write_all(&bytes).await?;
                // tokio buffers file writes on the blocking pool; flush so received
                // bytes are handed to the OS before a dropped connection aborts this loop.
                file.flush().await?;
                offset = end;
                progress(offset, size);
            }
            Message::FileDone { .. } => break,
            other => return Err(CoreError::Transfer(format!("unexpected message {other:?}"))),
        }
    }
    file.flush().await?;
    drop(file);
    if oversized {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(CoreError::Transfer("received more data than announced".into()));
    }

    let (actual, _) = hash_file(&partial).await?;
    if actual != hash {
        let _ = tokio::fs::remove_file(&partial).await;
        write_frame(stream, &Message::FileAck { transfer_id, offset: 0 }).await?;
        return Err(CoreError::Transfer("checksum mismatch".into()));
    }
    let wanted = save_dir.join(sanitize_relative_path(&name));
    if let Some(parent) = wanted.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let target = reserve_path(&wanted).await?;
    tokio::fs::rename(&partial, &target).await?;
    write_frame(stream, &Message::FileAck { transfer_id, offset: size }).await?;
    Ok(target)
}

pub fn cleanup_partials(save_dir: &Path, max_age: Duration) -> Result<()> {
    let dir = save_dir.join(PARTIAL_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default() >= max_age)
            .unwrap_or(true);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, len: usize) -> PathBuf {
        let path = dir.join(name);
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn hostile_names_stay_inside_the_save_folder() {
        for name in ["../../evil.exe", "C:\\Windows\\x.dll", "/etc/passwd", "..", "a/../../b.txt"] {
            let rel = sanitize_relative_path(name);
            assert!(rel.is_relative(), "{name:?} became {rel:?}");
            assert!(!rel.components().any(|c| matches!(c, std::path::Component::ParentDir)));
            assert!(rel.file_name().is_some());
        }
        assert_eq!(sanitize_relative_path("../../evil.exe"), PathBuf::from("evil.exe"));
        assert_eq!(sanitize_relative_path("Shots/a:b?.png"), PathBuf::from("Shots").join("ab.png"));
        assert_eq!(sanitize_relative_path("CON.txt"), PathBuf::from("_CON.txt"));
        assert_eq!(sanitize_relative_path(""), PathBuf::from("file"));
    }

    #[test]
    fn sanitized_paths_never_escape_the_save_dir() {
        let save_dir = tempfile::tempdir().unwrap();
        let hostile = [
            "C:evil.txt",
            "\\\\server\\share\\x",
            "a/./../b",
            "../../evil.exe",
            "C:\\Windows\\x.dll",
            "/etc/passwd",
            "\\\\?\\C:\\x",
            "//host/share/x",
            "..",
            ".",
            "...",
            "a/..\\..\\b",
            "NUL",
            "aux.tar.gz",
            "dir/COM1",
            "trailing. ",
            "x:y/../../z",
            "",
        ];
        for name in hostile {
            let rel = sanitize_relative_path(name);
            assert!(rel.is_relative(), "{name:?} became {rel:?}");
            assert!(
                rel.components().all(|c| matches!(c, std::path::Component::Normal(_))),
                "{name:?} became {rel:?}"
            );
            let joined = save_dir.path().join(&rel);
            assert!(joined.starts_with(save_dir.path()), "{name:?} escaped: {joined:?}");
            assert!(rel.file_name().is_some(), "{name:?} has no file name");
        }
    }

    #[test]
    fn unique_path_appends_counter() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("shot.png");
        assert_eq!(unique_path(&first), first);
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_path(&first), dir.path().join("shot (1).png"));
        std::fs::write(dir.path().join("shot (1).png"), b"x").unwrap();
        assert_eq!(unique_path(&first), dir.path().join("shot (2).png"));
    }

    #[test]
    fn expand_paths_walks_folders() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Shots");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        write_file(&folder, "a.png", 10);
        write_file(&folder.join("sub"), "b.png", 10);
        let single = write_file(dir.path(), "c.txt", 10);
        let mut names: Vec<String> = expand_paths(&[folder, single]).unwrap().into_iter().map(|(_, n)| n).collect();
        names.sort();
        assert_eq!(names, vec!["Shots/a.png", "Shots/sub/b.png", "c.txt"]);
    }

    #[tokio::test]
    async fn transfer_round_trip_and_collision() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let file = write_file(src.path(), "shot.png", CHUNK * 3 + 17);
        for expected in ["shot.png", "shot (1).png"] {
            let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
            let dst_path = dst.path().to_path_buf();
            let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
            send_file(&mut a, Uuid::new_v4(), &file, "shot.png", &mut |_| {}).await.unwrap();
            let saved = recv.await.unwrap().unwrap();
            assert_eq!(saved, dst.path().join(expected));
            assert_eq!(std::fs::read(&saved).unwrap(), std::fs::read(&file).unwrap());
        }
    }

    #[tokio::test]
    async fn interrupted_transfer_resumes() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let file = write_file(src.path(), "big.bin", CHUNK * 4);
        let data = std::fs::read(&file).unwrap();
        let hash = *blake3::hash(&data).as_bytes();
        let id = Uuid::new_v4();

        {
            let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
            let dst_path = dst.path().to_path_buf();
            let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
            let start = Message::FileStart { transfer_id: id, name: "big.bin".into(), size: data.len() as u64, hash };
            write_frame(&mut a, &start).await.unwrap();
            assert_eq!(read_frame(&mut a).await.unwrap(), Message::FileAck { transfer_id: id, offset: 0 });
            let chunk = Message::FileChunk { transfer_id: id, offset: 0, bytes: data[..CHUNK].to_vec() };
            write_frame(&mut a, &chunk).await.unwrap();
            drop(a);
            assert!(recv.await.unwrap().is_err());
        }

        let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
        let dst_path = dst.path().to_path_buf();
        let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
        let mut first_progress = None;
        send_file(&mut a, id, &file, "big.bin", &mut |sent| {
            first_progress.get_or_insert(sent);
        })
        .await
        .unwrap();
        let saved = recv.await.unwrap().unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), data);
        assert!(first_progress.unwrap() > CHUNK as u64, "did not resume from the partial file");
    }

    #[test]
    fn cleanup_removes_old_partials() {
        let dst = tempfile::tempdir().unwrap();
        let partial_dir = dst.path().join(PARTIAL_DIR);
        std::fs::create_dir_all(&partial_dir).unwrap();
        std::fs::write(partial_dir.join("x.part"), b"x").unwrap();
        cleanup_partials(dst.path(), Duration::ZERO).unwrap();
        assert!(!partial_dir.join("x.part").exists());
    }

    #[tokio::test]
    async fn chunks_beyond_the_announced_size_are_rejected() {
        let dst = tempfile::tempdir().unwrap();
        let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
        let dst_path = dst.path().to_path_buf();
        let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
        let id = Uuid::new_v4();
        let start = Message::FileStart { transfer_id: id, name: "x.bin".into(), size: 10, hash: [0u8; 32] };
        write_frame(&mut a, &start).await.unwrap();
        assert_eq!(read_frame(&mut a).await.unwrap(), Message::FileAck { transfer_id: id, offset: 0 });
        write_frame(&mut a, &Message::FileChunk { transfer_id: id, offset: 0, bytes: vec![1u8; 64] }).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), recv).await.expect("receiver kept waiting").unwrap();
        assert!(matches!(result, Err(CoreError::Transfer(_))), "{result:?}");
        let partial = dst.path().join(PARTIAL_DIR).join(format!("{id}.part"));
        assert!(!partial.exists(), "partial file was kept");
    }

    #[tokio::test]
    async fn concurrent_transfers_of_the_same_name_keep_every_file() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(dst.path().join("shot.png"), b"existing").unwrap();
        let file_a = write_file(src.path(), "a.bin", CHUNK + 5);
        let file_b = write_file(src.path(), "b.bin", CHUNK * 2 + 9);

        let (mut a1, mut b1) = tokio::io::duplex(1024 * 1024);
        let (mut a2, mut b2) = tokio::io::duplex(1024 * 1024);
        let (d1, d2) = (dst.path().to_path_buf(), dst.path().to_path_buf());
        let r1 = tokio::spawn(async move { receive_file(&mut b1, &d1, &mut |_, _| {}).await });
        let r2 = tokio::spawn(async move { receive_file(&mut b2, &d2, &mut |_, _| {}).await });
        let (mut p_a, mut p_b) = (|_: u64| {}, |_: u64| {});
        let (s1, s2) = tokio::join!(
            send_file(&mut a1, Uuid::new_v4(), &file_a, "shot.png", &mut p_a),
            send_file(&mut a2, Uuid::new_v4(), &file_b, "shot.png", &mut p_b),
        );
        s1.unwrap();
        s2.unwrap();
        let p1 = r1.await.unwrap().unwrap();
        let p2 = r2.await.unwrap().unwrap();
        assert_ne!(p1, p2);
        assert_eq!(std::fs::read(dst.path().join("shot.png")).unwrap(), b"existing");
        assert_eq!(std::fs::read(&p1).unwrap(), std::fs::read(&file_a).unwrap());
        assert_eq!(std::fs::read(&p2).unwrap(), std::fs::read(&file_b).unwrap());
    }

    #[test]
    fn long_multibyte_names_are_cut_on_a_char_boundary() {
        let name = format!("{}.", "\u{00e4}".repeat(300) + &" ".repeat(150));
        let rel = sanitize_relative_path(&name);
        let component = rel.file_name().unwrap().to_str().unwrap();
        assert!(component.len() <= 255, "{} bytes", component.len());
        assert!(!component.ends_with('.') && !component.ends_with(' '));
        let spaced = format!("{}{}x", "a".repeat(254), " ");
        let rel = sanitize_relative_path(&spaced);
        let component = rel.file_name().unwrap().to_str().unwrap();
        assert!(component.len() <= 255 && !component.ends_with(' '));
    }
}
