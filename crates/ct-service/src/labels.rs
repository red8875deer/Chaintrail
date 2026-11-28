//! Label loading and editing, shared by the CLI, the desktop app and `chaintrail serve`.

use std::path::{Path, PathBuf};

use ct_core::{Address, Confidence, EntityCategory};
use ct_labels::ofac::{OFAC_SDN_URL, OfacStats, pack_from_sdn_xml};
use ct_labels::{Group, LabelIndex, PackFile, PackMeta, USER_PACK_ID};

use crate::error::{Result, ServiceError};

/// Folder for the user's own packs and downloaded updates.
pub fn user_labels_dir() -> Option<PathBuf> {
    Some(ct_store::app_data_dir()?.join("labels"))
}

/// Built-in packs, then the user's folder, then an optional extra folder. A pack that
/// cannot be read is skipped and reported in the returned problems (the app still starts).
pub fn load_index(extra: Option<&Path>) -> Result<(LabelIndex, Vec<String>)> {
    let mut index = LabelIndex::with_builtin()?;
    let mut problems = Vec::new();
    if let Some(dir) = user_labels_dir() {
        problems.extend(index.load_dir_lenient(&dir).1);
    }
    if let Some(dir) = extra {
        problems.extend(index.load_dir_lenient(dir).1);
    }
    Ok((index, problems))
}

/// Write `contents` to `path` so that readers see the old file or the new one, never half
/// of one: a temporary file in the same folder, synced, then renamed over it. The previous
/// version is kept as `<name>.bak`.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("labels");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        if let Err(e) = f.write_all(contents.as_bytes()).and_then(|_| f.sync_all()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if path.exists() {
        let _ = std::fs::copy(path, dir.join(format!("{name}.bak")));
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Holds `<path>.lock` while a label file is read, changed and written, so two writers (two
/// windows, the app and the CLI) never lose each other's labels. A lock left by a crash is
/// taken over after 30 seconds.
struct FileLock(PathBuf);

impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        let lock = path.with_extension("toml.lock");
        for _ in 0..100 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(_) => return Ok(FileLock(lock)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&lock)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(30));
                    if stale {
                        let _ = std::fs::remove_file(&lock);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(ServiceError::internal(
            "the labels file is busy (another ChainTrail window is saving); try again",
        ))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Add a label to the user's own pack in `dir` and return the file written.
#[allow(clippy::too_many_arguments)]
pub fn add_user_label(
    dir: &Path,
    address: &Address,
    entity: &str,
    category: EntityCategory,
    confidence: Confidence,
    note: Option<String>,
    source: Option<String>,
) -> Result<PathBuf> {
    if confidence == Confidence::Certain {
        return Err(ServiceError::invalid(
            "labels are attributions; use low, medium or high confidence",
        ));
    }
    if entity.trim().is_empty() {
        return Err(ServiceError::invalid("entity name is required"));
    }
    // Plain, bounded text: labels are shown and exported to others. Direction-override
    // characters can make a name read differently from what it is.
    let bidi = |s: &str| {
        s.chars()
            .any(|c| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'))
    };
    for (name, text, max) in [
        ("entity name", Some(entity), 200),
        ("note", note.as_deref(), 1_000),
        ("source", source.as_deref(), 1_000),
    ] {
        if let Some(t) = text {
            if t.chars().count() > max {
                return Err(ServiceError::invalid(format!(
                    "the {name} is too long (at most {max} characters)"
                )));
            }
            if bidi(t) || t.chars().any(|c| c.is_control() && c != '\n') {
                return Err(ServiceError::invalid(format!(
                    "the {name} contains invisible or direction-changing characters"
                )));
            }
        }
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{USER_PACK_ID}.toml"));
    let _lock = FileLock::acquire(&path)?;
    let mut pack = match std::fs::read_to_string(&path) {
        Ok(text) => PackFile::parse(&text, &path.display().to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PackFile {
            pack: PackMeta {
                id: USER_PACK_ID.into(),
                title: "My labels".into(),
                source: "added by the user".into(),
                publisher: None,
                licence: "user's own data".into(),
                as_of: None,
                via: None,
                description: None,
                category: EntityCategory::UnknownService,
                confidence: Confidence::Medium,
            },
            groups: Vec::new(),
        },
        Err(e) => return Err(e.into()),
    };
    let mut group = Group {
        entity: entity.trim().to_string(),
        category: Some(category),
        confidence: Some(confidence),
        source,
        note,
        ..Default::default()
    };
    group.push(address);
    pack.groups.push(group);
    pack.labels()?; // validate before writing
    write_atomic(&path, &pack.to_toml())?;
    Ok(path)
}

/// Build the OFAC pack from `xml` and write it to `out`.
pub fn write_ofac_pack(xml: &str, out: &Path) -> Result<(PackFile, OfacStats)> {
    let (pack, stats) = pack_from_sdn_xml(xml)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(out, &pack.to_toml())?;
    Ok((pack, stats))
}

/// Download the current OFAC SDN list.
pub async fn download_ofac() -> Result<String> {
    let http = ct_providers::HttpClient::new(ct_providers::HttpConfig {
        timeout: std::time::Duration::from_secs(120),
        ..Default::default()
    })?;
    Ok(http.get_text("OFAC", OFAC_SDN_URL).await?)
}
