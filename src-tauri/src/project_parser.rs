//! Premiere Pro project file parser
//!
//! Handles parsing of .prproj files which are GZIP-compressed XML.
//! Extracts project structure, sequences, and media references.

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// Represents a parsed Premiere Pro project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiereProject {
    pub file_path: PathBuf,
    pub name: String,
    pub version: u32,
    pub bins: Vec<Bin>,
    pub sequences: Vec<Sequence>,
    pub media_files: HashMap<String, MediaFile>,
    pub project_items: HashMap<String, ProjectItem>,
}

/// A bin (folder) in the project panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bin {
    pub object_id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub path: String, // Full path like "Footage/Raw/Camera A"
}

/// A sequence (timeline) in the project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub object_id: String,
    pub name: String,
    pub duration_ticks: i64,
    pub frame_rate: FrameRate,
    pub video_tracks: Vec<Track>,
    pub audio_tracks: Vec<Track>,
    pub nested_sequences: Vec<String>, // Object IDs of nested sequences
}

/// Frame rate representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

impl FrameRate {
    pub fn as_f64(&self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

/// A track within a sequence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub object_id: String,
    pub name: String,
    pub track_type: TrackType,
    pub clips: Vec<TrackClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackType {
    Video,
    Audio,
}

/// A clip on a track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackClip {
    pub object_id: String,
    pub name: String,
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub in_point_ticks: i64,
    pub out_point_ticks: i64,
    pub media_ref: Option<String>, // Reference to MediaFile
    pub clip_type: ClipType,
    pub speed: f64, // 1.0 = normal speed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipType {
    Standard,
    Subclip { parent_id: String },
    MergedClip { components: Vec<String> },
    Multicam { angles: Vec<MulticamAngle> },
    Nested { sequence_id: String },
    Adjustment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticamAngle {
    pub name: String,
    pub media_ref: String,
    pub is_active: bool,
}

/// A project item (clip in the project panel)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectItem {
    pub object_id: String,
    pub name: String,
    pub item_type: ProjectItemType,
    pub media_ref: Option<String>,
    pub bin_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectItemType {
    Clip,
    Sequence,
    Bin,
    Subclip,
    MergedClip,
    Multicam,
}

/// A media file reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub object_id: String,
    pub file_path: PathBuf,
    pub has_video: bool,
    pub has_audio: bool,
    pub duration_ticks: i64,
    pub frame_rate: Option<FrameRate>,
    pub proxy_path: Option<PathBuf>,
    pub is_offline: bool,
    pub media_type: MediaType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
    Image,
    ImageSequence,
    RED,       // R3D files
    BRAW,      // Blackmagic RAW
    Graphics,  // MOGRT, After Effects
    Unknown,
}

impl MediaType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Video formats
            "mp4" | "mov" | "avi" | "mxf" | "mkv" | "wmv" | "m4v" | "webm" => MediaType::Video,
            "prores" => MediaType::Video,
            // Audio formats
            "wav" | "mp3" | "aac" | "aiff" | "flac" | "ogg" | "m4a" => MediaType::Audio,
            // Image formats
            "jpg" | "jpeg" | "png" | "tiff" | "tif" | "bmp" | "gif" | "psd" | "exr" | "dpx" => {
                MediaType::Image
            }
            // RAW formats
            "r3d" => MediaType::RED,
            "braw" => MediaType::BRAW,
            // Graphics
            "mogrt" | "aep" | "aegraphic" => MediaType::Graphics,
            _ => MediaType::Unknown,
        }
    }
}

/// Parser state for streaming XML parsing
struct ParserState {
    current_element: Vec<String>,
    objects: HashMap<String, XmlObject>,
    current_object_id: Option<String>,
    current_text: String,
}

#[derive(Debug, Default)]
struct XmlObject {
    tag: String,
    attributes: HashMap<String, String>,
    children: HashMap<String, Vec<String>>,
    text_content: Option<String>,
}

/// Main parser for .prproj files
pub struct ProjectParser {
    file_path: PathBuf,
}

impl ProjectParser {
    pub fn new(file_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
        }
    }

    /// Parse the project file
    pub fn parse(&self) -> Result<PremiereProject> {
        let xml_content = self.decompress_project()?;
        self.parse_xml(&xml_content)
    }

    /// Decompress GZIP-compressed .prproj file
    fn decompress_project(&self) -> Result<String> {
        let file = File::open(&self.file_path)
            .with_context(|| format!("Failed to open project file: {:?}", self.file_path))?;

        let buf_reader = BufReader::new(file);
        let mut decoder = GzDecoder::new(buf_reader);
        let mut xml_content = String::new();

        decoder
            .read_to_string(&mut xml_content)
            .with_context(|| "Failed to decompress project file")?;

        Ok(xml_content)
    }

    /// Parse the XML content into project structure
    fn parse_xml(&self, xml_content: &str) -> Result<PremiereProject> {
        let mut reader = Reader::from_str(xml_content);
        reader.config_mut().trim_text(true);

        let mut project = PremiereProject {
            file_path: self.file_path.clone(),
            name: self
                .file_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            version: 0,
            bins: Vec::new(),
            sequences: Vec::new(),
            media_files: HashMap::new(),
            project_items: HashMap::new(),
        };

        let mut state = ParserState {
            current_element: Vec::new(),
            objects: HashMap::new(),
            current_object_id: None,
            current_text: String::new(),
        };

        // Separate storage for file paths found anywhere in the XML
        let mut file_paths: Vec<(String, PathBuf)> = Vec::new(); // (parent_object_id, path)

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    state.current_element.push(tag_name.clone());

                    // Extract ObjectID if present
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"ObjectID" {
                            let id = String::from_utf8_lossy(&attr.value).to_string();
                            state.current_object_id = Some(id.clone());

                            let mut obj = XmlObject::default();
                            obj.tag = tag_name.clone();

                            // Store all attributes
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let value = String::from_utf8_lossy(&attr.value).to_string();
                                obj.attributes.insert(key, value);
                            }

                            state.objects.insert(id, obj);
                        }
                    }

                    // Handle Version attribute on PremiereData
                    if tag_name == "PremiereData" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"Version" {
                                if let Ok(version) = String::from_utf8_lossy(&attr.value).parse() {
                                    project.version = version;
                                }
                            }
                        }
                    }
                }
                Ok(Event::End(_)) => {
                    state.current_element.pop();
                    state.current_text.clear();
                }
                Ok(Event::Text(e)) => {
                    state.current_text = e.unescape().unwrap_or_default().to_string();

                    // Check if current element is a file path element (based on Premiere XML structure)
                    // Prefer absolute path elements over relative ones
                    let current_tag = state.current_element.last().map(|s| s.as_str()).unwrap_or("");
                    let is_absolute_path_element = matches!(current_tag,
                        "ActualMediaFilePath" | "FilePath" | "MediaFilePath"
                    );

                    // Check if the content looks like an actual ABSOLUTE file path
                    let text = &state.current_text;
                    let is_absolute_path = text.starts_with('/') ||
                        (text.len() > 2 && text.chars().nth(1) == Some(':'));  // Windows C:\

                    let looks_like_path = text.len() > 5 &&
                        is_absolute_path &&  // Only accept absolute paths
                        // Exclude cache/temp files
                        !text.contains("Peak Files") &&
                        !text.contains("Audio Previews") &&
                        !text.ends_with(".pek") &&
                        !text.ends_with(".cfa");

                    // Check for valid media extensions
                    let text_lower = text.to_lowercase();
                    let has_media_extension =
                        text_lower.ends_with(".mp4") || text_lower.ends_with(".mov") ||
                        text_lower.ends_with(".mxf") || text_lower.ends_with(".wav") ||
                        text_lower.ends_with(".mp3") || text_lower.ends_with(".avi") ||
                        text_lower.ends_with(".r3d") || text_lower.ends_with(".braw") ||
                        text_lower.ends_with(".m4a") || text_lower.ends_with(".aif") ||
                        text_lower.ends_with(".aiff") || text_lower.ends_with(".png") ||
                        text_lower.ends_with(".jpg") || text_lower.ends_with(".jpeg") ||
                        text_lower.ends_with(".tiff") || text_lower.ends_with(".tif") ||
                        text_lower.ends_with(".aep") || text_lower.ends_with(".mogrt") ||
                        text_lower.ends_with(".prproj") || text_lower.ends_with(".gif") ||
                        text_lower.ends_with(".webm") || text_lower.ends_with(".mkv");

                    // Store if it's an absolute file path element with valid content
                    if is_absolute_path_element && looks_like_path && has_media_extension {
                        let parent_id = state.current_object_id.clone().unwrap_or_else(|| "unknown".to_string());
                        tracing::info!("Found media file path in {}: {} (parent: {})", current_tag, text, parent_id);
                        file_paths.push((parent_id, PathBuf::from(text.clone())));
                    }

                    // Store text content for current path
                    if let Some(ref obj_id) = state.current_object_id {
                        if let Some(obj) = state.objects.get_mut(obj_id) {
                            if !state.current_text.is_empty() {
                                let path = state.current_element.join("/");
                                obj.children
                                    .entry(path)
                                    .or_default()
                                    .push(state.current_text.clone());
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    tracing::warn!("XML parsing error: {:?}", e);
                    continue;
                }
                _ => {}
            }
            buf.clear();
        }

        tracing::info!("Found {} direct file paths in XML", file_paths.len());

        // Create media files from the file paths we found
        for (parent_id, file_path) in &file_paths {
            if !project.media_files.contains_key(parent_id) {
                let ext = file_path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let media_type = MediaType::from_extension(&ext);

                let media = MediaFile {
                    object_id: parent_id.clone(),
                    file_path: file_path.clone(),
                    has_video: matches!(media_type, MediaType::Video | MediaType::Image | MediaType::ImageSequence | MediaType::RED | MediaType::BRAW),
                    has_audio: matches!(media_type, MediaType::Audio | MediaType::Video),
                    duration_ticks: 0,
                    frame_rate: None,
                    proxy_path: None,
                    is_offline: !file_path.exists(),
                    media_type,
                };
                project.media_files.insert(parent_id.clone(), media);
            }
        }

        // Process parsed objects into structured data
        self.process_objects(&state.objects, &mut project)?;

        Ok(project)
    }

    /// Process raw XML objects into structured project data
    fn process_objects(
        &self,
        objects: &HashMap<String, XmlObject>,
        project: &mut PremiereProject,
    ) -> Result<()> {
        for (id, obj) in objects {
            match obj.tag.as_str() {
                // Premiere Pro uses VideoSequenceSource for sequences
                "VideoSequenceSource" => {
                    if let Some(sequence) = self.parse_sequence(id, obj, objects) {
                        project.sequences.push(sequence);
                    }
                }
                "Bin" | "BinProjectItem" | "RootProjectItem" => {
                    if let Some(bin) = self.parse_bin(id, obj) {
                        project.bins.push(bin);
                    }
                }
                "ClipProjectItem" | "ProjectItem" | "SubClip" => {
                    if let Some(item) = self.parse_project_item(id, obj) {
                        project.project_items.insert(id.clone(), item);
                    }
                }
                _ => {}
            }
        }

        // Build bin paths
        self.build_bin_paths(&mut project.bins);

        Ok(())
    }

    fn parse_sequence(
        &self,
        id: &str,
        obj: &XmlObject,
        _objects: &HashMap<String, XmlObject>,
    ) -> Option<Sequence> {
        // Try various attribute/child names for the sequence name
        let name = obj
            .attributes
            .get("Name")
            .or_else(|| obj.attributes.get("ObjectName"))
            .or_else(|| obj.children.get("Name").and_then(|v| v.first()))
            .cloned()
            .unwrap_or_else(|| format!("Sequence {}", id));

        Some(Sequence {
            object_id: id.to_string(),
            name,
            duration_ticks: 0,
            frame_rate: FrameRate {
                numerator: 24000,
                denominator: 1001,
            },
            video_tracks: Vec::new(),
            audio_tracks: Vec::new(),
            nested_sequences: Vec::new(),
        })
    }

    fn parse_bin(&self, id: &str, obj: &XmlObject) -> Option<Bin> {
        let name = obj
            .attributes
            .get("Name")
            .or_else(|| obj.children.get("Name").and_then(|v| v.first()))
            .cloned()
            .unwrap_or_else(|| format!("Bin {}", id));

        Some(Bin {
            object_id: id.to_string(),
            name,
            parent_id: obj.attributes.get("ParentID").cloned(),
            children: Vec::new(),
            path: String::new(),
        })
    }

    // Note: Media files are now parsed directly in parse_xml from FilePath elements
    #[allow(dead_code)]
    fn parse_media_file(&self, _id: &str, _obj: &XmlObject) -> Option<MediaFile> {
        None
    }

    fn parse_project_item(&self, id: &str, obj: &XmlObject) -> Option<ProjectItem> {
        let name = obj
            .attributes
            .get("Name")
            .or_else(|| obj.children.get("Name").and_then(|v| v.first()))
            .cloned()
            .unwrap_or_else(|| format!("Item {}", id));

        let item_type = match obj.tag.as_str() {
            "SequenceProjectItem" => ProjectItemType::Sequence,
            "BinProjectItem" => ProjectItemType::Bin,
            "SubclipProjectItem" => ProjectItemType::Subclip,
            "MergedClipProjectItem" => ProjectItemType::MergedClip,
            "MultiCameraClipProjectItem" => ProjectItemType::Multicam,
            _ => ProjectItemType::Clip,
        };

        let media_ref = obj
            .attributes
            .get("MediaRef")
            .or_else(|| obj.children.get("MediaRef").and_then(|v| v.first()))
            .cloned();

        Some(ProjectItem {
            object_id: id.to_string(),
            name,
            item_type,
            media_ref,
            bin_id: obj.attributes.get("ParentBinID").cloned(),
        })
    }

    fn build_bin_paths(&self, bins: &mut [Bin]) {
        // Store both name and parent_id for each bin
        let bin_info: HashMap<String, (String, Option<String>)> = bins
            .iter()
            .map(|b| (b.object_id.clone(), (b.name.clone(), b.parent_id.clone())))
            .collect();

        for bin in bins.iter_mut() {
            let mut path_parts = vec![bin.name.clone()];
            let mut current_parent = bin.parent_id.clone();

            while let Some(parent_id) = current_parent {
                if let Some((parent_name, grandparent_id)) = bin_info.get(&parent_id) {
                    path_parts.insert(0, parent_name.clone());
                    // Get parent's parent from the map
                    current_parent = grandparent_id.clone();
                } else {
                    break;
                }
            }

            bin.path = path_parts.join("/");
        }
    }
}

/// Get sidecar files for media (e.g., .xmp, audio files for RED/BRAW)
pub fn get_sidecar_files(media_path: &Path) -> Vec<PathBuf> {
    let mut sidecars = Vec::new();
    let parent = match media_path.parent() {
        Some(p) => p,
        None => return sidecars,
    };

    let stem = match media_path.file_stem() {
        Some(s) => s.to_string_lossy().to_string(),
        None => return sidecars,
    };

    let ext = media_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // XMP sidecar
    let xmp_path = parent.join(format!("{}.xmp", stem));
    if xmp_path.exists() {
        sidecars.push(xmp_path);
    }

    // RED camera specific sidecars
    if ext == "r3d" {
        // RED audio files (.wav)
        for entry in std::fs::read_dir(parent).into_iter().flatten() {
            if let Ok(entry) = entry {
                let entry_path = entry.path();
                if let Some(entry_stem) = entry_path.file_stem() {
                    let entry_stem = entry_stem.to_string_lossy();
                    if entry_stem.starts_with(&stem) {
                        if let Some(entry_ext) = entry_path.extension() {
                            if entry_ext.to_string_lossy().to_lowercase() == "wav" {
                                sidecars.push(entry_path);
                            }
                        }
                    }
                }
            }
        }
    }

    // BRAW sidecars
    if ext == "braw" {
        // Look for .sidecar files
        let sidecar_path = parent.join(format!("{}.sidecar", stem));
        if sidecar_path.exists() {
            sidecars.push(sidecar_path);
        }
    }

    sidecars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_type_from_extension() {
        assert!(matches!(
            MediaType::from_extension("mp4"),
            MediaType::Video
        ));
        assert!(matches!(
            MediaType::from_extension("wav"),
            MediaType::Audio
        ));
        assert!(matches!(MediaType::from_extension("r3d"), MediaType::RED));
        assert!(matches!(
            MediaType::from_extension("braw"),
            MediaType::BRAW
        ));
    }

    #[test]
    fn test_frame_rate() {
        let fr = FrameRate {
            numerator: 24000,
            denominator: 1001,
        };
        assert!((fr.as_f64() - 23.976).abs() < 0.01);
    }
}
