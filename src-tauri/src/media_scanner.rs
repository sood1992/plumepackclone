//! Media scanning and inventory management
//!
//! Scans project for all media files, checks their status,
//! and provides inventory information.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use xxhash_rust::xxh3::xxh3_64;

use crate::project_parser::{get_sidecar_files, MediaFile, MediaType, PremiereProject};

/// Complete media inventory for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInventory {
    pub items: Vec<MediaInventoryItem>,
    pub total_size: u64,
    pub total_count: usize,
    pub online_count: usize,
    pub offline_count: usize,
    pub duplicate_groups: Vec<DuplicateGroup>,
}

/// A media item with full inventory information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInventoryItem {
    pub object_id: String,
    pub file_path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub is_online: bool,
    pub media_type: MediaType,
    pub has_proxy: bool,
    pub proxy_path: Option<PathBuf>,
    pub proxy_size: Option<u64>,
    pub sidecar_files: Vec<PathBuf>,
    pub sidecar_total_size: u64,
    pub hash: Option<String>,
    pub bin_path: Option<String>,
}

/// Group of duplicate files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub file_size: u64,
    pub items: Vec<String>, // Object IDs
}

/// Scanner for media files
pub struct MediaScanner<'a> {
    project: &'a PremiereProject,
}

impl<'a> MediaScanner<'a> {
    pub fn new(project: &'a PremiereProject) -> Self {
        Self { project }
    }

    /// Scan all media and build inventory
    pub fn scan(&self) -> Result<MediaInventory> {
        let mut items = Vec::new();
        let mut total_size = 0u64;
        let mut online_count = 0;
        let mut offline_count = 0;

        for (id, media) in &self.project.media_files {
            let item = self.scan_media_file(id, media)?;

            if item.is_online {
                total_size += item.file_size + item.sidecar_total_size;
                if let Some(proxy_size) = item.proxy_size {
                    total_size += proxy_size;
                }
                online_count += 1;
            } else {
                offline_count += 1;
            }

            items.push(item);
        }

        let duplicate_groups = self.find_duplicates(&items);

        Ok(MediaInventory {
            total_count: items.len(),
            items,
            total_size,
            online_count,
            offline_count,
            duplicate_groups,
        })
    }

    fn scan_media_file(&self, id: &str, media: &MediaFile) -> Result<MediaInventoryItem> {
        let file_path = &media.file_path;
        let is_online = file_path.exists();

        let file_size = if is_online {
            fs::metadata(file_path)
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        let sidecar_files = if is_online {
            get_sidecar_files(file_path)
        } else {
            Vec::new()
        };

        let sidecar_total_size: u64 = sidecar_files
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();

        let proxy_size = media
            .proxy_path
            .as_ref()
            .filter(|p| p.exists())
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len());

        let has_proxy = proxy_size.is_some();

        // Find bin path for this media
        let bin_path = self
            .project
            .project_items
            .values()
            .find(|item| item.media_ref.as_ref() == Some(id))
            .and_then(|item| item.bin_id.as_ref())
            .and_then(|bin_id| {
                self.project
                    .bins
                    .iter()
                    .find(|b| &b.object_id == bin_id)
                    .map(|b| b.path.clone())
            });

        Ok(MediaInventoryItem {
            object_id: id.to_string(),
            file_path: file_path.clone(),
            file_name: file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            file_size,
            is_online,
            media_type: media.media_type.clone(),
            has_proxy,
            proxy_path: media.proxy_path.clone(),
            proxy_size,
            sidecar_files,
            sidecar_total_size,
            hash: None, // Computed lazily
            bin_path,
        })
    }

    /// Find duplicate files using size + partial hash
    fn find_duplicates(&self, items: &[MediaInventoryItem]) -> Vec<DuplicateGroup> {
        // Group by file size first
        let mut size_groups: HashMap<u64, Vec<&MediaInventoryItem>> = HashMap::new();
        for item in items {
            if item.is_online && item.file_size > 0 {
                size_groups
                    .entry(item.file_size)
                    .or_default()
                    .push(item);
            }
        }

        let mut duplicate_groups = Vec::new();

        // Only check files with same size
        for (size, group) in size_groups {
            if group.len() < 2 {
                continue;
            }

            // Compute partial hashes for these files
            let mut hash_groups: HashMap<String, Vec<String>> = HashMap::new();
            for item in group {
                if let Ok(hash) = compute_partial_hash(&item.file_path) {
                    hash_groups.entry(hash).or_default().push(item.object_id.clone());
                }
            }

            // Report duplicates
            for (hash, ids) in hash_groups {
                if ids.len() > 1 {
                    duplicate_groups.push(DuplicateGroup {
                        hash,
                        file_size: size,
                        items: ids,
                    });
                }
            }
        }

        duplicate_groups
    }
}

/// Compute a partial hash (first and last 1MB) for quick duplicate detection
pub fn compute_partial_hash(path: &Path) -> Result<String> {
    let file = fs::File::open(path).context("Failed to open file for hashing")?;
    let metadata = file.metadata()?;
    let file_size = metadata.len();

    const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB

    let mut hasher_data = Vec::new();

    // Read first chunk
    let first_chunk = std::io::Read::by_ref(&mut std::io::BufReader::new(&file))
        .take(CHUNK_SIZE)
        .bytes()
        .collect::<Result<Vec<_>, _>>()?;
    hasher_data.extend_from_slice(&first_chunk);

    // Read last chunk if file is large enough
    if file_size > CHUNK_SIZE * 2 {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = fs::File::open(path)?;
        file.seek(SeekFrom::End(-(CHUNK_SIZE as i64)))?;
        let mut last_chunk = vec![0u8; CHUNK_SIZE as usize];
        file.read_exact(&mut last_chunk)?;
        hasher_data.extend_from_slice(&last_chunk);
    }

    // Include file size in hash
    hasher_data.extend_from_slice(&file_size.to_le_bytes());

    let hash = xxh3_64(&hasher_data);
    Ok(format!("{:016x}", hash))
}

/// Compute full file hash using xxHash
pub fn compute_full_hash(path: &Path) -> Result<String> {
    let data = fs::read(path).context("Failed to read file for hashing")?;
    let hash = xxh3_64(&data);
    Ok(format!("{:016x}", hash))
}

/// Check if a path points to an image sequence
pub fn is_image_sequence(path: &Path) -> bool {
    let file_name = match path.file_stem() {
        Some(n) => n.to_string_lossy(),
        None => return false,
    };

    // Check for common sequence patterns: name.0001.ext, name_0001.ext, name0001.ext
    let patterns = [
        regex::Regex::new(r"\d{3,}$").unwrap(),
        regex::Regex::new(r"[._]\d{3,}$").unwrap(),
    ];

    patterns.iter().any(|p| p.is_match(&file_name))
}

/// Get all files in an image sequence
pub fn get_image_sequence_files(path: &Path) -> Result<Vec<PathBuf>> {
    let parent = path.parent().context("No parent directory")?;
    let file_name = path.file_stem().context("No file stem")?.to_string_lossy();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());

    // Extract the base name (remove trailing numbers)
    let base_pattern = regex::Regex::new(r"^(.+?)[._]?\d+$").unwrap();
    let base_name = base_pattern
        .captures(&file_name)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .unwrap_or(&file_name);

    let mut sequence_files = Vec::new();

    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let entry_path = entry.path();

        if !entry_path.is_file() {
            continue;
        }

        // Check extension matches
        if let Some(ref expected_ext) = ext {
            if entry_path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                != Some(expected_ext.to_lowercase())
            {
                continue;
            }
        }

        // Check if file belongs to same sequence
        if let Some(entry_stem) = entry_path.file_stem() {
            let entry_stem = entry_stem.to_string_lossy();
            if entry_stem.starts_with(base_name) {
                sequence_files.push(entry_path);
            }
        }
    }

    sequence_files.sort();
    Ok(sequence_files)
}

/// Path mapping for cross-platform projects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMapping {
    pub windows_path: String,
    pub mac_path: String,
}

/// Normalize paths for cross-platform compatibility
pub fn normalize_path(path: &Path, mappings: &[PathMapping]) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Apply mappings
    for mapping in mappings {
        if path_str.starts_with(&mapping.windows_path) {
            return PathBuf::from(path_str.replacen(&mapping.windows_path, &mapping.mac_path, 1));
        }
        if path_str.starts_with(&mapping.mac_path) {
            return PathBuf::from(path_str.replacen(&mapping.mac_path, &mapping.windows_path, 1));
        }
    }

    // Normalize slashes
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(path_str.replace("/", "\\"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from(path_str.replace("\\", "/"))
    }
}

use std::io::Read;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_sequence() {
        assert!(is_image_sequence(Path::new("footage/shot_0001.dpx")));
        assert!(is_image_sequence(Path::new("footage/shot.0001.exr")));
        assert!(is_image_sequence(Path::new("footage/shot0001.png")));
        assert!(!is_image_sequence(Path::new("footage/shot.mov")));
    }

    #[test]
    fn test_path_mapping() {
        let mappings = vec![PathMapping {
            windows_path: "P:\\Projects".to_string(),
            mac_path: "/Volumes/Projects".to_string(),
        }];

        let win_path = Path::new("P:\\Projects\\MyProject\\footage.mov");
        let normalized = normalize_path(win_path, &mappings);
        assert!(normalized.to_string_lossy().contains("/Volumes/Projects"));
    }
}
