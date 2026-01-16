//! Tauri commands for frontend communication
//!
//! These commands are exposed to the React frontend via Tauri's IPC.

use crate::consolidation::{
    ConsolidationConfig, ConsolidationEngine, ConsolidationProgress, ConsolidationStatus,
    FolderStructure, OptimizationMode, ProcessingModeConfig, ProxyMode,
};
use crate::ffmpeg::{FFmpeg, MediaMetadata, TranscodePreset};
use crate::media_scanner::MediaScanner;
use crate::project_parser::{PremiereProject, ProjectParser};
use crate::sequence_analyzer::SequenceAnalyzer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::State;

/// Premiere ticks per second constant
const TICKS_PER_SECOND: f64 = 254016000000.0;

/// Application state for storing loaded projects and running jobs
pub struct AppState {
    /// Cache of parsed projects (path -> project)
    pub project_cache: RwLock<HashMap<String, Arc<PremiereProject>>>,
    /// Running consolidation jobs (job_id -> job info)
    pub running_jobs: RwLock<HashMap<String, JobInfo>>,
}

/// Information about a running job
pub struct JobInfo {
    pub engine: Arc<ConsolidationEngine>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            project_cache: RwLock::new(HashMap::new()),
            running_jobs: RwLock::new(HashMap::new()),
        }
    }
}

impl AppState {
    /// Get or parse a project, using cache if available
    fn get_project(&self, path: &str) -> Result<Arc<PremiereProject>, String> {
        // Check cache first
        {
            let cache = self.project_cache.read().unwrap();
            if let Some(project) = cache.get(path) {
                return Ok(project.clone());
            }
        }

        // Parse and cache
        let parser = ProjectParser::new(path);
        let project = parser.parse().map_err(|e| e.to_string())?;
        let project = Arc::new(project);

        {
            let mut cache = self.project_cache.write().unwrap();
            cache.insert(path.to_string(), project.clone());
        }

        Ok(project)
    }

    /// Clear project from cache (e.g., when it might have changed)
    fn invalidate_project(&self, path: &str) {
        let mut cache = self.project_cache.write().unwrap();
        cache.remove(path);
    }
}

/// Project info returned to frontend
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub file_path: String,
    pub version: u32,
    pub sequence_count: usize,
    pub media_count: usize,
    pub bin_count: usize,
}

/// Sequence info for frontend
#[derive(Debug, Clone, Serialize)]
pub struct SequenceInfo {
    pub object_id: String,
    pub name: String,
    pub duration_seconds: f64,
    pub frame_rate: f64,
    pub video_track_count: usize,
    pub audio_track_count: usize,
    pub nested_count: usize,
}

/// Media item info for frontend
#[derive(Debug, Clone, Serialize)]
pub struct MediaItemInfo {
    pub object_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub file_size_formatted: String,
    pub is_online: bool,
    pub media_type: String,
    pub has_proxy: bool,
    pub bin_path: Option<String>,
}

/// Media usage info for frontend
#[derive(Debug, Clone, Serialize)]
pub struct MediaUsageResult {
    pub used_count: usize,
    pub unused_count: usize,
    pub used_size: u64,
    pub unused_size: u64,
    pub used_media: Vec<UsedMediaItem>,
    pub unused_media: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsedMediaItem {
    pub object_id: String,
    pub file_name: String,
    pub usage_count: usize,
    pub time_range_seconds: (f64, f64),
    pub sequences: Vec<String>,
}

/// Consolidation options from frontend
#[derive(Debug, Clone, Deserialize)]
pub struct ConsolidationOptions {
    pub output_path: String,
    pub sequences: Vec<String>,
    pub processing_mode: String,
    pub transcode_preset: Option<String>,
    pub optimization_mode: String,
    pub folder_structure: String,
    pub proxy_mode: String,
    pub handle_frames: i64,
    pub include_all_multicam_angles: bool,
    pub generate_unique_filenames: bool,
    pub use_project_item_names: bool,
    pub add_frame_range_to_filename: bool,
    pub copy_sidecar_files: bool,
    pub skip_offline_media: bool,
}

/// Open and parse a Premiere Pro project file
#[tauri::command]
pub async fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectInfo, String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err("Project file not found".to_string());
    }

    // Invalidate cache to force fresh parse when opening
    state.invalidate_project(&path);

    let project = state.get_project(&path)?;

    Ok(ProjectInfo {
        name: project.name.clone(),
        file_path: project.file_path.to_string_lossy().to_string(),
        version: project.version,
        sequence_count: project.sequences.len(),
        media_count: project.media_files.len(),
        bin_count: project.bins.len(),
    })
}

/// Get full project info including parsed content
#[tauri::command]
pub async fn get_project_info(path: String, state: State<'_, AppState>) -> Result<ProjectInfo, String> {
    let project = state.get_project(&path)?;

    Ok(ProjectInfo {
        name: project.name.clone(),
        file_path: project.file_path.to_string_lossy().to_string(),
        version: project.version,
        sequence_count: project.sequences.len(),
        media_count: project.media_files.len(),
        bin_count: project.bins.len(),
    })
}

/// Get all sequences in the project
#[tauri::command]
pub async fn get_sequences(path: String, state: State<'_, AppState>) -> Result<Vec<SequenceInfo>, String> {
    let project = state.get_project(&path)?;

    let sequences: Vec<SequenceInfo> = project
        .sequences
        .iter()
        .map(|seq| {
            let duration_seconds = seq.duration_ticks as f64 / TICKS_PER_SECOND;
            SequenceInfo {
                object_id: seq.object_id.clone(),
                name: seq.name.clone(),
                duration_seconds,
                frame_rate: seq.frame_rate.as_f64(),
                video_track_count: seq.video_tracks.len(),
                audio_track_count: seq.audio_tracks.len(),
                nested_count: seq.nested_sequences.len(),
            }
        })
        .collect();

    Ok(sequences)
}

/// Get all media items in the project
#[tauri::command]
pub async fn get_media_items(path: String, state: State<'_, AppState>) -> Result<Vec<MediaItemInfo>, String> {
    let project = state.get_project(&path)?;

    let scanner = MediaScanner::new(&project);
    let inventory = scanner.scan().map_err(|e| e.to_string())?;

    let items: Vec<MediaItemInfo> = inventory
        .items
        .into_iter()
        .map(|item| MediaItemInfo {
            object_id: item.object_id,
            file_path: item.file_path.to_string_lossy().to_string(),
            file_name: item.file_name,
            file_size: item.file_size,
            file_size_formatted: format_file_size(item.file_size),
            is_online: item.is_online,
            media_type: format!("{:?}", item.media_type),
            has_proxy: item.has_proxy,
            bin_path: item.bin_path,
        })
        .collect();

    Ok(items)
}

/// Analyze media usage in selected sequences
#[tauri::command]
pub async fn analyze_media_usage(
    path: String,
    sequence_ids: Vec<String>,
    handle_frames: i64,
    include_all_multicam: bool,
    state: State<'_, AppState>,
) -> Result<MediaUsageResult, String> {
    let project = state.get_project(&path)?;

    let analyzer = SequenceAnalyzer::new(&project)
        .with_handles(handle_frames)
        .include_all_multicam_angles(include_all_multicam);

    let usage = if sequence_ids.is_empty() {
        analyzer.analyze_all()
    } else {
        analyzer.analyze_sequences(&sequence_ids)
    };

    let scanner = MediaScanner::new(&project);
    let inventory = scanner.scan().map_err(|e| e.to_string())?;

    // Calculate sizes
    let mut used_size = 0u64;
    let mut unused_size = 0u64;

    for item in &inventory.items {
        if usage.used_media.contains_key(&item.object_id) {
            used_size += item.file_size;
        } else {
            unused_size += item.file_size;
        }
    }

    let used_media: Vec<UsedMediaItem> = usage
        .used_media
        .iter()
        .map(|(id, info)| {
            let file_name = project
                .media_files
                .get(id)
                .map(|m| {
                    m.file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            let (start, end) = info.merged_range.to_seconds();

            UsedMediaItem {
                object_id: id.clone(),
                file_name,
                usage_count: info.usage_count,
                time_range_seconds: (start, end),
                sequences: info.used_in_sequences.clone(),
            }
        })
        .collect();

    Ok(MediaUsageResult {
        used_count: usage.used_media.len(),
        unused_count: usage.unused_media.len(),
        used_size,
        unused_size,
        used_media,
        unused_media: usage.unused_media,
    })
}

/// Get list of unused media
#[tauri::command]
pub async fn get_unused_media(
    path: String,
    sequence_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MediaItemInfo>, String> {
    let project = state.get_project(&path)?;

    let analyzer = SequenceAnalyzer::new(&project);
    let usage = if sequence_ids.is_empty() {
        analyzer.analyze_all()
    } else {
        analyzer.analyze_sequences(&sequence_ids)
    };

    let scanner = MediaScanner::new(&project);
    let inventory = scanner.scan().map_err(|e| e.to_string())?;

    let unused: Vec<MediaItemInfo> = inventory
        .items
        .into_iter()
        .filter(|item| usage.unused_media.contains(&item.object_id))
        .map(|item| MediaItemInfo {
            object_id: item.object_id,
            file_path: item.file_path.to_string_lossy().to_string(),
            file_name: item.file_name,
            file_size: item.file_size,
            file_size_formatted: format_file_size(item.file_size),
            is_online: item.is_online,
            media_type: format!("{:?}", item.media_type),
            has_proxy: item.has_proxy,
            bin_path: item.bin_path,
        })
        .collect();

    Ok(unused)
}

/// Start consolidation process
#[tauri::command]
pub async fn start_consolidation(
    project_path: String,
    options: ConsolidationOptions,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let config = build_consolidation_config(project_path, options)?;

    let engine = ConsolidationEngine::new(config).map_err(|e| e.to_string())?;
    let job_id = engine.get_progress().job_id.clone();
    let cancel_flag = engine.get_cancel_flag();

    // Store the job
    let engine_arc = Arc::new(engine);
    {
        let mut jobs = state.running_jobs.write().unwrap();
        jobs.insert(
            job_id.clone(),
            JobInfo {
                engine: engine_arc.clone(),
                cancel_flag: cancel_flag.clone(),
            },
        );
    }

    // Run in background thread
    let engine_clone = engine_arc.clone();
    let job_id_clone = job_id.clone();
    let state_jobs = state.running_jobs.clone();

    std::thread::spawn(move || {
        let result = engine_clone.run();

        // Log completion
        match &result {
            Ok(r) => tracing::info!("Job {} completed successfully: {} files processed", job_id_clone, r.files_processed),
            Err(e) => tracing::error!("Job {} failed: {}", job_id_clone, e),
        }

        // Note: We keep the job in the map so progress can be queried
        // It will be cleaned up when a new consolidation starts or manually
    });

    Ok(job_id)
}

/// Cancel a running consolidation job
#[tauri::command]
pub async fn cancel_consolidation(job_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let jobs = state.running_jobs.read().unwrap();

    if let Some(job) = jobs.get(&job_id) {
        job.cancel_flag.store(true, Ordering::SeqCst);
        tracing::info!("Cancellation requested for job {}", job_id);
        Ok(())
    } else {
        Err(format!("Job {} not found", job_id))
    }
}

/// Get progress of a consolidation job
#[tauri::command]
pub async fn get_consolidation_progress(
    job_id: String,
    state: State<'_, AppState>,
) -> Result<ConsolidationProgress, String> {
    let jobs = state.running_jobs.read().unwrap();

    if let Some(job) = jobs.get(&job_id) {
        Ok(job.engine.get_progress())
    } else {
        Err(format!("Job {} not found", job_id))
    }
}

/// Check if FFmpeg is available
#[tauri::command]
pub async fn check_ffmpeg() -> Result<String, String> {
    FFmpeg::check_availability().map_err(|e| e.to_string())
}

/// Get metadata for a media file
#[tauri::command]
pub async fn get_media_metadata(path: String) -> Result<MediaMetadata, String> {
    let ffmpeg = FFmpeg::new().map_err(|e| e.to_string())?;
    ffmpeg.probe(&PathBuf::from(path)).map_err(|e| e.to_string())
}

/// Estimate output size for consolidation
#[tauri::command]
pub async fn estimate_output_size(
    project_path: String,
    options: ConsolidationOptions,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let config = build_consolidation_config(project_path.clone(), options)?;
    let project = state.get_project(&project_path)?;

    crate::consolidation::estimate_output_size(&project, &config).map_err(|e| e.to_string())
}

/// Validate output path
#[tauri::command]
pub async fn validate_output_path(path: String) -> Result<bool, String> {
    let path = PathBuf::from(&path);

    // Check if parent exists or can be created
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Ok(false);
        }
    }

    // Check write permissions
    if path.exists() {
        let test_file = path.join(".write_test");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    } else {
        if let Some(parent) = path.parent() {
            let test_file = parent.join(".write_test");
            match std::fs::write(&test_file, "test") {
                Ok(_) => {
                    let _ = std::fs::remove_file(&test_file);
                    Ok(true)
                }
                Err(_) => Ok(false),
            }
        } else {
            Ok(false)
        }
    }
}

// Helper functions

fn build_consolidation_config(
    project_path: String,
    options: ConsolidationOptions,
) -> Result<ConsolidationConfig, String> {
    let processing_mode = match options.processing_mode.as_str() {
        "trim" => ProcessingModeConfig::Trim,
        "transcode" => {
            let preset = match options.transcode_preset.as_deref() {
                Some("prores422") => TranscodePreset::ProRes422,
                Some("prores422hq") => TranscodePreset::ProRes422HQ,
                Some("prores422lt") => TranscodePreset::ProRes422LT,
                Some("prores4444") => TranscodePreset::ProRes4444,
                Some("dnxhd") => TranscodePreset::DNxHD,
                Some("dnxhr") => TranscodePreset::DNxHR,
                Some("h264high") => TranscodePreset::H264High,
                Some("h264medium") => TranscodePreset::H264Medium,
                Some("h265high") => TranscodePreset::H265High,
                Some("h265medium") => TranscodePreset::H265Medium,
                _ => TranscodePreset::ProRes422,
            };
            ProcessingModeConfig::Transcode { preset }
        }
        "copy" => ProcessingModeConfig::Copy,
        "no_process" => ProcessingModeConfig::NoProcess,
        _ => ProcessingModeConfig::Trim,
    };

    let optimization_mode = match options.optimization_mode.as_str() {
        "minimize" => OptimizationMode::MinimizeDiskSpace,
        "keep_files" => OptimizationMode::KeepSameNumberOfFiles,
        "unique_clips" => OptimizationMode::EachClipUnique,
        _ => OptimizationMode::KeepSameNumberOfFiles,
    };

    let folder_structure = match options.folder_structure.as_str() {
        "flat" => FolderStructure::Flat,
        "bins" => FolderStructure::BinStructure,
        "original" => FolderStructure::OriginalDiskStructure,
        _ => FolderStructure::Flat,
    };

    let proxy_mode = match options.proxy_mode.as_str() {
        "both" => ProxyMode::CopyBoth,
        "proxy_only" => ProxyMode::ProxyOnly,
        "main_only" => ProxyMode::MainOnly,
        "preserve" => ProxyMode::PreserveReferences,
        _ => ProxyMode::CopyBoth,
    };

    Ok(ConsolidationConfig {
        project_path: PathBuf::from(project_path),
        output_path: PathBuf::from(options.output_path),
        sequences: options.sequences,
        processing_mode,
        optimization_mode,
        folder_structure,
        proxy_mode,
        handle_frames: options.handle_frames,
        include_unused_multicam_angles: options.include_all_multicam_angles,
        generate_unique_filenames: options.generate_unique_filenames,
        use_project_item_names: options.use_project_item_names,
        add_frame_range_to_filename: options.add_frame_range_to_filename,
        copy_sidecar_files: options.copy_sidecar_files,
        skip_offline_media: options.skip_offline_media,
    })
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
