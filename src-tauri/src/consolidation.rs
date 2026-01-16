//! Consolidation engine for project processing
//!
//! Handles the complete consolidation workflow: scanning, processing,
//! and rewriting project files.

use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::ffmpeg::{FFmpeg, TranscodePreset};
use crate::media_scanner::{MediaInventory, MediaScanner};
use crate::project_parser::{get_sidecar_files, MediaFile, PremiereProject, ProjectParser};
use crate::sequence_analyzer::{
    find_common_ancestor, optimize_time_ranges, MediaUsageAnalysis, SequenceAnalyzer, TimeRange,
};

/// Consolidation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    pub project_path: PathBuf,
    pub output_path: PathBuf,
    pub sequences: Vec<String>,
    pub processing_mode: ProcessingModeConfig,
    pub optimization_mode: OptimizationMode,
    pub folder_structure: FolderStructure,
    pub proxy_mode: ProxyMode,
    pub handle_frames: i64,
    pub include_unused_multicam_angles: bool,
    pub generate_unique_filenames: bool,
    pub use_project_item_names: bool,
    pub add_frame_range_to_filename: bool,
    pub copy_sidecar_files: bool,
    pub skip_offline_media: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingModeConfig {
    Trim,
    Transcode { preset: TranscodePreset },
    Copy,
    NoProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationMode {
    /// Minimize disk space - split files when used non-contiguously
    MinimizeDiskSpace,
    /// Keep same number of files - one output per input
    KeepSameNumberOfFiles,
    /// Each timeline clip as unique file - for VFX roundtrips
    EachClipUnique,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FolderStructure {
    /// All media in single folder
    Flat,
    /// Mirror project panel bin structure
    BinStructure,
    /// Recreate original disk structure using common ancestor
    OriginalDiskStructure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyMode {
    /// Copy both main and proxy
    CopyBoth,
    /// Copy only proxy (fallback to main if no proxy)
    ProxyOnly,
    /// Copy only main media
    MainOnly,
    /// Keep proxy references but don't copy proxy files
    PreserveReferences,
}

/// Progress tracking for consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationProgress {
    pub job_id: String,
    pub status: ConsolidationStatus,
    pub current_file: String,
    pub current_operation: String,
    pub files_processed: u64,
    pub files_total: u64,
    pub bytes_processed: u64,
    pub bytes_total: u64,
    pub errors: Vec<ProcessingError>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsolidationStatus {
    Pending,
    Analyzing,
    Processing,
    WritingProject,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingError {
    pub file_path: String,
    pub error_message: String,
    pub is_fatal: bool,
}

/// Result of consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    pub job_id: String,
    pub success: bool,
    pub output_project_path: PathBuf,
    pub files_processed: u64,
    pub bytes_saved: i64,
    pub original_size: u64,
    pub final_size: u64,
    pub duration_seconds: f64,
    pub path_mapping: HashMap<PathBuf, PathBuf>,
    pub errors: Vec<ProcessingError>,
    pub warnings: Vec<String>,
}

/// Mapping from original to new paths
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMapping {
    pub original: PathBuf,
    pub new_path: PathBuf,
    pub time_range: Option<(f64, f64)>,
}

/// Main consolidation engine
pub struct ConsolidationEngine {
    config: ConsolidationConfig,
    project: PremiereProject,
    ffmpeg: FFmpeg,
    progress: Arc<Mutex<ConsolidationProgress>>,
    cancel_flag: Arc<AtomicBool>,
    bytes_processed: Arc<AtomicU64>,
}

impl ConsolidationEngine {
    pub fn new(config: ConsolidationConfig) -> Result<Self> {
        let parser = ProjectParser::new(&config.project_path);
        let project = parser.parse().context("Failed to parse project file")?;
        let ffmpeg = FFmpeg::new().context("Failed to initialize FFmpeg")?;

        let job_id = Uuid::new_v4().to_string();

        let progress = ConsolidationProgress {
            job_id: job_id.clone(),
            status: ConsolidationStatus::Pending,
            current_file: String::new(),
            current_operation: String::new(),
            files_processed: 0,
            files_total: 0,
            bytes_processed: 0,
            bytes_total: 0,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        Ok(Self {
            config,
            project,
            ffmpeg,
            progress: Arc::new(Mutex::new(progress)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            bytes_processed: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Get current progress
    pub fn get_progress(&self) -> ConsolidationProgress {
        self.progress.lock().unwrap().clone()
    }

    /// Cancel the consolidation
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Get the cancel flag for external tracking
    pub fn get_cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel_flag.clone()
    }

    /// Run the consolidation process
    pub fn run(&self) -> Result<ConsolidationResult> {
        let start_time = std::time::Instant::now();

        // Phase 1: Analyze media usage
        self.update_status(ConsolidationStatus::Analyzing, "Analyzing sequences...");

        let analyzer = SequenceAnalyzer::new(&self.project)
            .with_handles(self.config.handle_frames)
            .include_all_multicam_angles(self.config.include_unused_multicam_angles);

        let usage = if self.config.sequences.is_empty() {
            analyzer.analyze_all()
        } else {
            analyzer.analyze_sequences(&self.config.sequences)
        };

        self.check_cancelled()?;

        // Phase 2: Scan media inventory
        let scanner = MediaScanner::new(&self.project);
        let inventory = scanner.scan()?;

        // Calculate totals
        let files_total = usage.used_media.len() as u64;
        let bytes_total = self.calculate_total_bytes(&usage, &inventory);

        {
            let mut progress = self.progress.lock().unwrap();
            progress.files_total = files_total;
            progress.bytes_total = bytes_total;
        }

        self.check_cancelled()?;

        // Phase 3: Create output directory structure
        self.update_status(ConsolidationStatus::Processing, "Creating output directories...");
        self.create_output_structure()?;

        self.check_cancelled()?;

        // Phase 4: Process each media file
        let mut path_mapping: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut original_size = 0u64;
        let mut final_size = 0u64;

        for (media_id, usage_info) in &usage.used_media {
            self.check_cancelled()?;

            let media = match self.project.media_files.get(media_id) {
                Some(m) => m,
                None => {
                    self.add_warning(format!("Media not found: {}", media_id));
                    continue;
                }
            };

            if !media.file_path.exists() {
                if self.config.skip_offline_media {
                    self.add_warning(format!("Skipping offline media: {:?}", media.file_path));
                    continue;
                } else {
                    self.add_error(ProcessingError {
                        file_path: media.file_path.to_string_lossy().to_string(),
                        error_message: "File is offline".to_string(),
                        is_fatal: false,
                    });
                    continue;
                }
            }

            self.update_current_file(&media.file_path);

            let file_size = fs::metadata(&media.file_path)
                .map(|m| m.len())
                .unwrap_or(0);
            original_size += file_size;

            // Calculate output path
            let output_path = self.calculate_output_path(media, &usage_info.used_in_sequences)?;

            // Ensure parent directory exists
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Process the file based on mode
            match &self.config.processing_mode {
                ProcessingModeConfig::Trim => {
                    let ranges = optimize_time_ranges(&usage_info.time_ranges, 0);
                    self.process_trim(media, &output_path, &ranges)?;
                }
                ProcessingModeConfig::Transcode { preset } => {
                    let ranges = optimize_time_ranges(&usage_info.time_ranges, 0);
                    self.process_transcode(media, &output_path, &ranges, preset)?;
                }
                ProcessingModeConfig::Copy => {
                    self.process_copy(media, &output_path)?;
                }
                ProcessingModeConfig::NoProcess => {
                    // Just track the mapping, don't process
                }
            }

            // Handle sidecars
            if self.config.copy_sidecar_files {
                self.copy_sidecars(media, &output_path)?;
            }

            // Handle proxy
            self.process_proxy(media, &output_path)?;

            // Update mapping
            path_mapping.insert(media.file_path.clone(), output_path.clone());

            // Update progress
            let output_size = fs::metadata(&output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            final_size += output_size;

            self.increment_processed(file_size);
        }

        self.check_cancelled()?;

        // Phase 5: Write new project file
        self.update_status(ConsolidationStatus::WritingProject, "Creating new project file...");
        let output_project_path = self.write_project_file(&path_mapping)?;

        // Complete
        self.update_status(ConsolidationStatus::Completed, "Consolidation complete");

        let progress = self.progress.lock().unwrap();

        Ok(ConsolidationResult {
            job_id: progress.job_id.clone(),
            success: progress.errors.iter().all(|e| !e.is_fatal),
            output_project_path,
            files_processed: progress.files_processed,
            bytes_saved: original_size as i64 - final_size as i64,
            original_size,
            final_size,
            duration_seconds: start_time.elapsed().as_secs_f64(),
            path_mapping,
            errors: progress.errors.clone(),
            warnings: progress.warnings.clone(),
        })
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancel_flag.load(Ordering::Relaxed) {
            self.update_status(ConsolidationStatus::Cancelled, "Cancelled by user");
            anyhow::bail!("Consolidation cancelled");
        }
        Ok(())
    }

    fn update_status(&self, status: ConsolidationStatus, operation: &str) {
        let mut progress = self.progress.lock().unwrap();
        progress.status = status;
        progress.current_operation = operation.to_string();
    }

    fn update_current_file(&self, path: &Path) {
        let mut progress = self.progress.lock().unwrap();
        progress.current_file = path.to_string_lossy().to_string();
    }

    fn increment_processed(&self, bytes: u64) {
        let mut progress = self.progress.lock().unwrap();
        progress.files_processed += 1;
        progress.bytes_processed += bytes;
    }

    fn add_error(&self, error: ProcessingError) {
        let mut progress = self.progress.lock().unwrap();
        progress.errors.push(error);
    }

    fn add_warning(&self, warning: String) {
        let mut progress = self.progress.lock().unwrap();
        progress.warnings.push(warning);
    }

    fn calculate_total_bytes(
        &self,
        usage: &MediaUsageAnalysis,
        inventory: &MediaInventory,
    ) -> u64 {
        usage
            .used_media
            .keys()
            .filter_map(|id| {
                inventory
                    .items
                    .iter()
                    .find(|item| item.object_id == *id)
                    .map(|item| item.file_size)
            })
            .sum()
    }

    fn create_output_structure(&self) -> Result<()> {
        fs::create_dir_all(&self.config.output_path)?;

        // Create media subfolder
        let media_folder = self.config.output_path.join("Media");
        fs::create_dir_all(&media_folder)?;

        // Create proxy folder if needed
        if matches!(self.config.proxy_mode, ProxyMode::CopyBoth | ProxyMode::ProxyOnly) {
            let proxy_folder = self.config.output_path.join("Proxy");
            fs::create_dir_all(&proxy_folder)?;
        }

        Ok(())
    }

    fn calculate_output_path(
        &self,
        media: &MediaFile,
        used_in_sequences: &[String],
    ) -> Result<PathBuf> {
        let media_folder = self.config.output_path.join("Media");
        let file_name = media
            .file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let base_path = match &self.config.folder_structure {
            FolderStructure::Flat => media_folder,

            FolderStructure::BinStructure => {
                // Find bin path for this media
                let bin_path = self
                    .project
                    .project_items
                    .values()
                    .find(|item| item.media_ref.as_ref() == Some(&media.object_id))
                    .and_then(|item| item.bin_id.as_ref())
                    .and_then(|bin_id| {
                        self.project
                            .bins
                            .iter()
                            .find(|b| &b.object_id == bin_id)
                    })
                    .map(|bin| bin.path.clone())
                    .unwrap_or_default();

                if bin_path.is_empty() {
                    media_folder
                } else {
                    media_folder.join(bin_path)
                }
            }

            FolderStructure::OriginalDiskStructure => {
                // Collect all media paths to find common ancestor
                let all_paths: Vec<PathBuf> = self
                    .project
                    .media_files
                    .values()
                    .map(|m| m.file_path.clone())
                    .collect();

                if let Some(common) = find_common_ancestor(&all_paths) {
                    // Get relative path from common ancestor
                    if let Ok(relative) = media.file_path.strip_prefix(&common) {
                        if let Some(parent) = relative.parent() {
                            media_folder.join(parent)
                        } else {
                            media_folder
                        }
                    } else {
                        media_folder
                    }
                } else {
                    media_folder
                }
            }
        };

        // Generate final filename
        let mut final_name = if self.config.use_project_item_names {
            // Try to find project item name
            self.project
                .project_items
                .values()
                .find(|item| item.media_ref.as_ref() == Some(&media.object_id))
                .map(|item| item.name.clone())
                .unwrap_or(file_name.clone())
        } else {
            file_name.clone()
        };

        // Handle duplicate filenames
        if self.config.generate_unique_filenames {
            let mut counter = 1;
            let extension = Path::new(&final_name)
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = Path::new(&final_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(final_name.clone());

            let mut candidate = base_path.join(&final_name);
            while candidate.exists() {
                final_name = if extension.is_empty() {
                    format!("{}_pp{:03}", stem, counter)
                } else {
                    format!("{}_pp{:03}.{}", stem, counter, extension)
                };
                candidate = base_path.join(&final_name);
                counter += 1;
            }
        }

        Ok(base_path.join(final_name))
    }

    fn process_trim(
        &self,
        media: &MediaFile,
        output_path: &Path,
        ranges: &[TimeRange],
    ) -> Result<()> {
        if ranges.is_empty() {
            return self.process_copy(media, output_path);
        }

        // Get metadata
        let metadata = self.ffmpeg.probe(&media.file_path)?;

        // For single range, just trim
        if ranges.len() == 1 {
            let range = &ranges[0];
            let (start_sec, end_sec) = range.to_seconds();

            self.ffmpeg.trim_lossless(
                &media.file_path,
                output_path,
                start_sec,
                end_sec,
                Some(self.cancel_flag.clone()),
            )?;
        } else {
            // Multiple ranges - need to handle based on optimization mode
            match self.config.optimization_mode {
                OptimizationMode::MinimizeDiskSpace => {
                    // Create separate output for each range
                    for (i, range) in ranges.iter().enumerate() {
                        let (start_sec, end_sec) = range.to_seconds();
                        let range_output = self.add_range_suffix(output_path, i, start_sec, end_sec);

                        self.ffmpeg.trim_lossless(
                            &media.file_path,
                            &range_output,
                            start_sec,
                            end_sec,
                            Some(self.cancel_flag.clone()),
                        )?;
                    }
                }
                OptimizationMode::KeepSameNumberOfFiles => {
                    // Merge all ranges and trim to encompassing range
                    let merged = TimeRange::new(
                        ranges.iter().map(|r| r.start_ticks).min().unwrap_or(0),
                        ranges.iter().map(|r| r.end_ticks).max().unwrap_or(0),
                    );
                    let (start_sec, end_sec) = merged.to_seconds();

                    self.ffmpeg.trim_lossless(
                        &media.file_path,
                        output_path,
                        start_sec,
                        end_sec,
                        Some(self.cancel_flag.clone()),
                    )?;
                }
                OptimizationMode::EachClipUnique => {
                    // Same as MinimizeDiskSpace for trim
                    for (i, range) in ranges.iter().enumerate() {
                        let (start_sec, end_sec) = range.to_seconds();
                        let range_output = self.add_range_suffix(output_path, i, start_sec, end_sec);

                        self.ffmpeg.trim_lossless(
                            &media.file_path,
                            &range_output,
                            start_sec,
                            end_sec,
                            Some(self.cancel_flag.clone()),
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    fn process_transcode(
        &self,
        media: &MediaFile,
        output_path: &Path,
        ranges: &[TimeRange],
        preset: &TranscodePreset,
    ) -> Result<()> {
        if ranges.is_empty() {
            self.ffmpeg.transcode(
                &media.file_path,
                output_path,
                None,
                None,
                preset,
                Some(self.cancel_flag.clone()),
                None,
            )?;
            return Ok(());
        }

        // Get metadata for progress
        let merged = TimeRange::new(
            ranges.iter().map(|r| r.start_ticks).min().unwrap_or(0),
            ranges.iter().map(|r| r.end_ticks).max().unwrap_or(0),
        );
        let (start_sec, end_sec) = merged.to_seconds();

        self.ffmpeg.transcode(
            &media.file_path,
            output_path,
            Some(start_sec),
            Some(end_sec),
            preset,
            Some(self.cancel_flag.clone()),
            None,
        )?;

        Ok(())
    }

    fn process_copy(&self, media: &MediaFile, output_path: &Path) -> Result<()> {
        fs::copy(&media.file_path, output_path)
            .with_context(|| format!("Failed to copy {:?}", media.file_path))?;
        Ok(())
    }

    fn copy_sidecars(&self, media: &MediaFile, output_path: &Path) -> Result<()> {
        let sidecars = get_sidecar_files(&media.file_path);
        let output_dir = output_path.parent().unwrap_or(Path::new("."));

        for sidecar in sidecars {
            if let Some(sidecar_name) = sidecar.file_name() {
                let output_sidecar = output_dir.join(sidecar_name);
                fs::copy(&sidecar, &output_sidecar)
                    .with_context(|| format!("Failed to copy sidecar {:?}", sidecar))?;
            }
        }

        Ok(())
    }

    fn process_proxy(&self, media: &MediaFile, output_path: &Path) -> Result<()> {
        match self.config.proxy_mode {
            ProxyMode::CopyBoth | ProxyMode::ProxyOnly => {
                if let Some(ref proxy_path) = media.proxy_path {
                    if proxy_path.exists() {
                        let proxy_folder = self.config.output_path.join("Proxy");
                        if let Some(proxy_name) = proxy_path.file_name() {
                            let output_proxy = proxy_folder.join(proxy_name);
                            fs::copy(proxy_path, &output_proxy)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn add_range_suffix(&self, path: &Path, index: usize, start: f64, end: f64) -> PathBuf {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let suffix = if self.config.add_frame_range_to_filename {
            format!("_{}_to_{}", start as i64, end as i64)
        } else {
            format!("_{:03}", index + 1)
        };

        let new_name = if ext.is_empty() {
            format!("{}{}", stem, suffix)
        } else {
            format!("{}{}.{}", stem, suffix, ext)
        };

        path.parent()
            .unwrap_or(Path::new("."))
            .join(new_name)
    }

    fn write_project_file(&self, path_mapping: &HashMap<PathBuf, PathBuf>) -> Result<PathBuf> {
        // Read original project
        let mut original_content = String::new();
        {
            let file = File::open(&self.config.project_path)?;
            let mut decoder = flate2::read::GzDecoder::new(file);
            decoder.read_to_string(&mut original_content)?;
        }

        // Replace paths in XML
        let mut updated_content = original_content.clone();
        for (original, new_path) in path_mapping {
            let original_str = original.to_string_lossy();
            let new_str = new_path.to_string_lossy();

            // Replace both forward and back slashes versions
            updated_content = updated_content.replace(&original_str.to_string(), &new_str);
            updated_content = updated_content.replace(
                &original_str.replace('/', "\\"),
                &new_str.replace('/', "\\"),
            );
            updated_content = updated_content.replace(
                &original_str.replace('\\', "/"),
                &new_str.replace('\\', "/"),
            );
        }

        // Write new project file
        let project_name = self
            .config
            .project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project.prproj".to_string());

        let output_project_path = self.config.output_path.join(project_name);

        let file = File::create(&output_project_path)?;
        let buf_writer = BufWriter::new(file);
        let mut encoder = GzEncoder::new(buf_writer, Compression::default());
        encoder.write_all(updated_content.as_bytes())?;
        encoder.finish()?;

        // Also write a manifest file
        self.write_manifest(path_mapping)?;

        Ok(output_project_path)
    }

    fn write_manifest(&self, path_mapping: &HashMap<PathBuf, PathBuf>) -> Result<()> {
        let manifest_path = self.config.output_path.join("consolidation_manifest.json");

        let manifest = serde_json::json!({
            "version": "1.0",
            "created": chrono::Utc::now().to_rfc3339(),
            "original_project": self.config.project_path,
            "path_mappings": path_mapping.iter().map(|(k, v)| {
                serde_json::json!({
                    "original": k,
                    "new": v
                })
            }).collect::<Vec<_>>(),
            "config": {
                "processing_mode": format!("{:?}", self.config.processing_mode),
                "optimization_mode": format!("{:?}", self.config.optimization_mode),
                "folder_structure": format!("{:?}", self.config.folder_structure),
                "handle_frames": self.config.handle_frames
            }
        });

        let file = File::create(&manifest_path)?;
        serde_json::to_writer_pretty(file, &manifest)?;

        Ok(())
    }
}

/// Estimate output size for a consolidation job
pub fn estimate_output_size(
    project: &PremiereProject,
    config: &ConsolidationConfig,
) -> Result<u64> {
    let analyzer = SequenceAnalyzer::new(project)
        .with_handles(config.handle_frames)
        .include_all_multicam_angles(config.include_unused_multicam_angles);

    let usage = if config.sequences.is_empty() {
        analyzer.analyze_all()
    } else {
        analyzer.analyze_sequences(&config.sequences)
    };

    let mut total_size = 0u64;

    for (media_id, usage_info) in &usage.used_media {
        let media = match project.media_files.get(media_id) {
            Some(m) => m,
            None => continue,
        };

        if !media.file_path.exists() {
            continue;
        }

        let file_size = fs::metadata(&media.file_path)
            .map(|m| m.len())
            .unwrap_or(0);

        match &config.processing_mode {
            ProcessingModeConfig::Trim => {
                // Estimate based on usage ratio
                let total_duration = media.duration_ticks as f64;
                let used_duration: i64 = usage_info.time_ranges.iter().map(|r| r.duration()).sum();
                let ratio = if total_duration > 0.0 {
                    (used_duration as f64) / total_duration
                } else {
                    1.0
                };
                total_size += (file_size as f64 * ratio) as u64;
            }
            ProcessingModeConfig::Transcode { .. } => {
                // Rough estimate - transcoding can vary widely
                total_size += file_size; // Assume similar size
            }
            ProcessingModeConfig::Copy | ProcessingModeConfig::NoProcess => {
                total_size += file_size;
            }
        }
    }

    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimization_modes() {
        // Ensure all modes serialize correctly
        let modes = vec![
            OptimizationMode::MinimizeDiskSpace,
            OptimizationMode::KeepSameNumberOfFiles,
            OptimizationMode::EachClipUnique,
        ];

        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let _: OptimizationMode = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_folder_structures() {
        let structures = vec![
            FolderStructure::Flat,
            FolderStructure::BinStructure,
            FolderStructure::OriginalDiskStructure,
        ];

        for structure in structures {
            let json = serde_json::to_string(&structure).unwrap();
            let _: FolderStructure = serde_json::from_str(&json).unwrap();
        }
    }
}
