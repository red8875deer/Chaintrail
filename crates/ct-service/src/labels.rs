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

/// Built-in packs, then the user's folder, then an optional extra folder.
pub fn load_index(extra: Option<&Path>) -> Result<LabelIndex> {
    let mut index = LabelIndex::with_builtin()?;
    if let Some(dir) = user_labels_dir() {
        index.load_dir(&dir)?;
    }
    if let Some(dir) = extra {
        index.load_dir(dir)?;
    }
    Ok(index)
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
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{USER_PACK_ID}.toml"));
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
    std::fs::write(&path, pack.to_toml())?;
    Ok(path)
}

/// Build the OFAC pack from `xml` and write it to `out`.
pub fn write_ofac_pack(xml: &str, out: &Path) -> Result<(PackFile, OfacStats)> {
    let (pack, stats) = pack_from_sdn_xml(xml)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, pack.to_toml())?;
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
