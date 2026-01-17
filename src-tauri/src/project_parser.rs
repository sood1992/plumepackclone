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
/// Premiere uses TWO reference systems:
/// 1. Numeric: ObjectID defines, ObjectRef references (e.g., "728")
/// 2. GUID: ObjectUID defines, ObjectURef references (e.g., "7e388a19-...")
///
/// IMPORTANT: ObjectIDs can be REUSED for different object types (collisions!)
/// We store Vec<XmlObject> per ID and filter by expected type when resolving.
struct ParserState {
    current_element: Vec<String>,
    /// Objects indexed by numeric ObjectID - Vec because IDs can collide across types!
    objects_by_id: HashMap<String, Vec<XmlObject>>,
    /// Objects indexed by GUID ObjectUID (GUIDs are unique, no collision)
    objects_by_uid: HashMap<String, XmlObject>,
    /// Stack of object contexts - tracks (ObjectID, ObjectUID) for each nested element
    /// This is CRITICAL: references must be associated with the IMMEDIATE parent, not ancestors
    object_context_stack: Vec<(Option<String>, Option<String>)>,
    current_text: String,
    /// References from numeric ObjectID: source_id -> Vec<(element_tag, target_id, is_guid_ref)>
    refs_from_id: HashMap<String, Vec<(String, String, bool)>>,
    /// References from GUID ObjectUID: source_uid -> Vec<(element_tag, target_id, is_guid_ref)>
    refs_from_uid: HashMap<String, Vec<(String, String, bool)>>,
    /// Media objects (have file paths) indexed by their ObjectUID
    media_file_paths: HashMap<String, PathBuf>,
}

impl ParserState {
    /// Get the current object context (the most recent ObjectID/ObjectUID from the stack)
    fn current_context(&self) -> (Option<String>, Option<String>) {
        // Find the most recent element with an ObjectID or ObjectUID
        for (obj_id, obj_uid) in self.object_context_stack.iter().rev() {
            if obj_id.is_some() || obj_uid.is_some() {
                return (obj_id.clone(), obj_uid.clone());
            }
        }
        (None, None)
    }

    /// Get the current object ID (preferring the most immediate context)
    fn current_object_id(&self) -> Option<String> {
        // Search from most recent first - find the closest ancestor with an ID
        for (obj_id, _) in self.object_context_stack.iter().rev() {
            if obj_id.is_some() {
                return obj_id.clone();
            }
        }
        None
    }

    /// Get the current object UID (preferring the most immediate context)
    fn current_object_uid(&self) -> Option<String> {
        // Search from most recent first - find the closest ancestor with a UID
        for (_, obj_uid) in self.object_context_stack.iter().rev() {
            if obj_uid.is_some() {
                return obj_uid.clone();
            }
        }
        None
    }
}

#[derive(Debug, Default, Clone)]
struct XmlObject {
    tag: String,
    object_id: Option<String>,   // Numeric ID
    object_uid: Option<String>,  // GUID
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
            objects_by_id: HashMap::new(),  // Vec per ID to handle collisions
            objects_by_uid: HashMap::new(),
            object_context_stack: Vec::new(),  // Track nested object contexts
            current_text: String::new(),
            refs_from_id: HashMap::new(),
            refs_from_uid: HashMap::new(),
            media_file_paths: HashMap::new(),
        };

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    state.current_element.push(tag_name.clone());

                    // Collect all attributes
                    let attrs: Vec<(String, String)> = e.attributes()
                        .flatten()
                        .map(|attr| {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            (key, value)
                        })
                        .collect();

                    // Process this element
                    Self::process_element_attrs(&tag_name, &attrs, &mut state, false);

                    // Handle Version attribute on PremiereData
                    if tag_name == "PremiereData" {
                        for (key, value) in &attrs {
                            if key == "Version" {
                                if let Ok(version) = value.parse() {
                                    project.version = version;
                                }
                            }
                        }
                    }
                }
                Ok(Event::End(_)) => {
                    state.current_element.pop();
                    state.object_context_stack.pop();  // Pop the context for this element
                    state.current_text.clear();
                }
                Ok(Event::Empty(e)) => {
                    // CRITICAL: Self-closing tags like <Source ObjectRef="212"/> contain references!
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // Collect all attributes
                    let attrs: Vec<(String, String)> = e.attributes()
                        .flatten()
                        .map(|attr| {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            (key, value)
                        })
                        .collect();

                    // Process this element (is_empty = true)
                    Self::process_element_attrs(&tag_name, &attrs, &mut state, true);
                }
                Ok(Event::Text(e)) => {
                    state.current_text = e.unescape().unwrap_or_default().to_string();

                    let current_tag = state.current_element.last().map(|s| s.as_str()).unwrap_or("");
                    let text = &state.current_text;

                    // Check if this is a file path element inside a Media object
                    // Media objects are indexed by ObjectUID (GUID)
                    let is_file_path_element = matches!(current_tag,
                        "ActualMediaFilePath" | "FilePath" | "MediaFilePath"
                    );

                    // Check if the content looks like an actual ABSOLUTE file path
                    let is_absolute_path = text.starts_with('/') ||
                        (text.len() > 2 && text.chars().nth(1) == Some(':'));  // Windows C:\

                    let looks_like_path = text.len() > 5 &&
                        is_absolute_path &&
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

                    // Store file path - Media objects use ObjectUID (GUID) as their identifier
                    if is_file_path_element && looks_like_path && has_media_extension {
                        // Use the context stack to get the current object's UID or ID
                        let parent_uid = state.current_object_uid()
                            .or_else(|| state.current_object_id())
                            .unwrap_or_else(|| "unknown".to_string());
                        tracing::info!("Found media file: {} (Media UID: {})", text, parent_uid);
                        state.media_file_paths.insert(parent_uid, PathBuf::from(text.clone()));
                    }

                    // Store text content in current object
                    // Use just the current tag name as the key (not full path) for easier lookup
                    let child_key = current_tag.to_string();

                    // Use context stack to find the current object and store text in it
                    if let Some(obj_uid) = state.current_object_uid() {
                        if let Some(obj) = state.objects_by_uid.get_mut(&obj_uid) {
                            if !state.current_text.is_empty() {
                                obj.children.entry(child_key.clone()).or_default().push(state.current_text.clone());
                            }
                        }
                    }
                    // Also try ID-based lookup (some objects only have ObjectID)
                    if let Some(obj_id) = state.current_object_id() {
                        // Store in the LAST (most recent) object with this ID
                        if let Some(objs) = state.objects_by_id.get_mut(&obj_id) {
                            if let Some(obj) = objs.last_mut() {
                                if !state.current_text.is_empty() {
                                    obj.children.entry(child_key).or_default().push(state.current_text.clone());
                                }
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

        // Log parsing summary
        let total_objects_by_id: usize = state.objects_by_id.values().map(|v| v.len()).sum();
        tracing::info!("Parsing complete:");
        tracing::info!("  - Unique numeric IDs: {} (total objects: {})", state.objects_by_id.len(), total_objects_by_id);
        tracing::info!("  - Objects by GUID: {}", state.objects_by_uid.len());
        tracing::info!("  - Refs from numeric IDs: {}", state.refs_from_id.values().map(|v| v.len()).sum::<usize>());
        tracing::info!("  - Refs from GUIDs: {}", state.refs_from_uid.values().map(|v| v.len()).sum::<usize>());
        tracing::info!("  - Media file paths: {}", state.media_file_paths.len());

        // Log sample refs from numeric IDs
        let sample_refs: Vec<_> = state.refs_from_id.iter().take(5).collect();
        if !sample_refs.is_empty() {
            tracing::info!("Sample refs from numeric IDs:");
            for (parent_id, refs) in &sample_refs {
                for (tag, target, is_guid) in refs.iter().take(3) {
                    tracing::info!("  {} --{}--> {} (guid:{})", parent_id, tag, target, is_guid);
                }
            }
        }

        // Create media files from the file paths we found (indexed by GUID)
        for (media_uid, file_path) in &state.media_file_paths {
            let ext = file_path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let media_type = MediaType::from_extension(&ext);

            let media = MediaFile {
                object_id: media_uid.clone(),
                file_path: file_path.clone(),
                has_video: matches!(media_type, MediaType::Video | MediaType::Image | MediaType::ImageSequence | MediaType::RED | MediaType::BRAW),
                has_audio: matches!(media_type, MediaType::Audio | MediaType::Video),
                duration_ticks: 0,
                frame_rate: None,
                proxy_path: None,
                is_offline: !file_path.exists(),
                media_type,
            };
            project.media_files.insert(media_uid.clone(), media);
        }

        // Process parsed objects into structured data
        self.process_objects(&state, &mut project)?;

        tracing::info!(
            "Parsed project: {} sequences, {} media files",
            project.sequences.len(),
            project.media_files.len()
        );

        Ok(project)
    }

    /// Process element attributes - handles both Start and Empty events
    fn process_element_attrs(tag_name: &str, attrs: &[(String, String)], state: &mut ParserState, is_empty: bool) {
        let mut this_object_id: Option<String> = None;
        let mut this_object_uid: Option<String> = None;
        let mut object_ref: Option<String> = None;      // Numeric reference
        let mut object_uref: Option<String> = None;     // GUID reference

        // Extract all ID and reference attributes
        for (key, value) in attrs {
            match key.as_str() {
                "ObjectID" => {
                    this_object_id = Some(value.clone());
                }
                "ObjectUID" => {
                    this_object_uid = Some(value.clone());
                }
                "ObjectRef" => {
                    object_ref = Some(value.clone());
                }
                "ObjectURef" => {
                    object_uref = Some(value.clone());
                }
                _ => {}
            }
        }

        // Push context for non-empty elements (empty elements don't create a new context level)
        // This is CRITICAL: each Start element gets a context entry that will be popped on End
        if !is_empty {
            state.object_context_stack.push((this_object_id.clone(), this_object_uid.clone()));
        }

        // Create object if it has an ID or UID
        if this_object_id.is_some() || this_object_uid.is_some() {
            let obj = XmlObject {
                tag: tag_name.to_string(),
                object_id: this_object_id.clone(),
                object_uid: this_object_uid.clone(),
                attributes: attrs.iter().cloned().collect(),
                children: HashMap::new(),
                text_content: None,
            };

            // Push to Vec for numeric IDs (handles collisions - same ID, different types)
            if let Some(ref id) = this_object_id {
                state.objects_by_id.entry(id.clone()).or_default().push(obj.clone());
            }
            // GUIDs are unique, no collision
            if let Some(ref uid) = this_object_uid {
                state.objects_by_uid.insert(uid.clone(), obj);
            }
        }

        // Store references - associate with the current parent context (BEFORE this element)
        // References go FROM the current context TO the target
        // IMPORTANT: For references, we need the context BEFORE this element was pushed
        let has_ref = object_ref.is_some() || object_uref.is_some();
        if has_ref {
            // Get the context from BEFORE this element (the parent)
            // If this is non-empty, we just pushed ourselves, so look at the second-to-last
            // If this is empty, we didn't push, so look at the last
            let parent_context = if !is_empty && state.object_context_stack.len() >= 2 {
                // Look at context before we pushed ourselves
                let idx = state.object_context_stack.len() - 2;
                // Find the nearest ancestor with an ID or UID
                state.object_context_stack[..=idx].iter().rev()
                    .find(|(id, uid)| id.is_some() || uid.is_some())
                    .cloned()
            } else if is_empty && !state.object_context_stack.is_empty() {
                // Empty element - look at current context
                state.object_context_stack.iter().rev()
                    .find(|(id, uid)| id.is_some() || uid.is_some())
                    .cloned()
            } else {
                None
            };

            let (source_id, source_uid) = parent_context.unwrap_or((None, None));

            if let Some(ref target) = object_ref {
                // ObjectRef targets a numeric ObjectID
                // Store under the parent's ID - prefer ID over UID for clip items
                if let Some(ref src_id) = source_id {
                    state.refs_from_id.entry(src_id.clone()).or_default()
                        .push((tag_name.to_string(), target.clone(), false));
                } else if let Some(ref src_uid) = source_uid {
                    state.refs_from_uid.entry(src_uid.clone()).or_default()
                        .push((tag_name.to_string(), target.clone(), false));
                }
                tracing::debug!("Ref: {} --{}-> {} (numeric)",
                    source_id.as_ref().or(source_uid.as_ref()).unwrap_or(&"?".to_string()),
                    tag_name, target);
            }

            if let Some(ref target) = object_uref {
                // ObjectURef targets a GUID ObjectUID
                if let Some(ref src_id) = source_id {
                    state.refs_from_id.entry(src_id.clone()).or_default()
                        .push((tag_name.to_string(), target.clone(), true));
                } else if let Some(ref src_uid) = source_uid {
                    state.refs_from_uid.entry(src_uid.clone()).or_default()
                        .push((tag_name.to_string(), target.clone(), true));
                }
                tracing::debug!("Ref: {} --{}-> {} (GUID)",
                    source_id.as_ref().or(source_uid.as_ref()).unwrap_or(&"?".to_string()),
                    tag_name, target);
            }
        }
    }

    /// Process raw XML objects into structured project data
    fn process_objects(
        &self,
        state: &ParserState,
        project: &mut PremiereProject,
    ) -> Result<()> {
        // Collect clip track items - now we have Vec per ID, need to flatten and filter
        let clip_items: Vec<(&String, &XmlObject)> = state.objects_by_id.iter()
            .flat_map(|(id, objs)| objs.iter().map(move |obj| (id, obj)))
            .filter(|(_, obj)| obj.tag == "VideoClipTrackItem" || obj.tag == "AudioClipTrackItem")
            .collect();
        tracing::info!("Found {} clip track items (by ID)", clip_items.len());

        // Build clip_id -> media_uid mapping
        let mut clip_to_media: HashMap<String, String> = HashMap::new();

        for (clip_id, _clip_obj) in &clip_items {
            if let Some(media_uid) = self.find_media_for_clip(clip_id, state, 0) {
                clip_to_media.insert(clip_id.to_string(), media_uid);
            }
        }

        tracing::info!("Resolved {} clips to media", clip_to_media.len());

        // Process sequences - ONLY real Sequence objects with correct ClassID
        // Real Sequence ClassID: 6a15d903-8739-11d5-af2d-9b7855ad8974
        const SEQUENCE_CLASS_ID: &str = "6a15d903-8739-11d5-af2d-9b7855ad8974";

        for (uid, obj) in &state.objects_by_uid {
            if obj.tag == "Sequence" {
                // Check ClassID to filter out non-sequences
                let class_id = obj.attributes.get("ClassID").map(|s| s.as_str()).unwrap_or("");
                if class_id == SEQUENCE_CLASS_ID {
                    if let Some(mut sequence) = self.parse_sequence_from_obj(uid, obj) {
                        // Populate tracks by finding clips that belong to this sequence
                        self.populate_sequence_tracks(&mut sequence, uid, state, &clip_to_media);
                        project.sequences.push(sequence);
                    }
                } else {
                    tracing::debug!("Skipping non-Sequence with tag 'Sequence', ClassID: {}", class_id);
                }
            }
        }

        // NOTE: Removed VideoSequenceSource fallback - those are not real sequences
        // They were creating fake "Sequence 315" etc. entries

        tracing::info!("Found {} real sequences", project.sequences.len());

        // Process bins (flatten Vec for objects_by_id)
        for (id, objs) in &state.objects_by_id {
            for obj in objs {
                if matches!(obj.tag.as_str(), "Bin" | "BinProjectItem" | "RootProjectItem") {
                    if let Some(bin) = self.parse_bin_from_obj(id, obj) {
                        project.bins.push(bin);
                    }
                }
            }
        }
        for (uid, obj) in &state.objects_by_uid {
            if matches!(obj.tag.as_str(), "Bin" | "BinProjectItem" | "RootProjectItem") {
                if let Some(bin) = self.parse_bin_from_obj(uid, obj) {
                    project.bins.push(bin);
                }
            }
        }

        // Build bin paths
        self.build_bin_paths(&mut project.bins);

        Ok(())
    }

    /// Follow the reference chain from a clip track item to find the media file
    /// Chain: VideoClipTrackItem -> SubClip -> MasterClip -> Clip -> MediaSource -> Media
    ///
    /// IMPORTANT: The reference element name (e.g., "SubClip") tells us the expected target type!
    /// We use this to filter when ObjectIDs collide across types.
    fn find_media_for_clip(&self, start_id: &str, state: &ParserState, depth: usize) -> Option<String> {
        if depth > 20 {
            return None; // Prevent infinite loops
        }

        // Get refs from this ID (could be numeric ID or GUID)
        let refs_from_id = state.refs_from_id.get(start_id);
        let refs_from_uid = state.refs_from_uid.get(start_id);

        // Combine all refs: (element_tag, target_id, is_guid_ref)
        let all_refs: Vec<_> = refs_from_id.into_iter()
            .flat_map(|v| v.iter())
            .chain(refs_from_uid.into_iter().flat_map(|v| v.iter()))
            .collect();

        // Log at depth 0 for debugging
        if depth == 0 && all_refs.is_empty() {
            tracing::debug!("find_media_for_clip: {} has NO refs at depth 0", start_id);
        } else if depth <= 2 && !all_refs.is_empty() {
            tracing::debug!("find_media_for_clip depth {}: {} has {} refs: {:?}",
                depth, start_id, all_refs.len(), all_refs.iter().take(3).collect::<Vec<_>>());
        }

        for (ref_element_tag, target, is_guid) in all_refs {
            // If target is a GUID reference
            if *is_guid {
                // Check if we already have the file path for this Media
                if state.media_file_paths.contains_key(target) {
                    return Some(target.clone());
                }
                // Check if target exists as a GUID object
                if let Some(target_obj) = state.objects_by_uid.get(target) {
                    if target_obj.tag == "Media" {
                        return Some(target.clone());
                    }
                    // Recurse with the GUID
                    if let Some(media) = self.find_media_for_clip(target, state, depth + 1) {
                        return Some(media);
                    }
                }
            } else {
                // Numeric reference - look up by ID, but FILTER by expected type!
                // The element tag hints at the expected type (e.g., <SubClip ObjectRef="X"/> expects a SubClip)
                if let Some(target_objs) = state.objects_by_id.get(target) {
                    // Try to find an object matching the expected type first
                    let expected_type = ref_element_tag.as_str();

                    // Find the object that matches the expected type, or fall back to any
                    let target_obj = target_objs.iter()
                        .find(|obj| obj.tag == expected_type)
                        .or_else(|| target_objs.iter().find(|obj| {
                            // Also accept related types
                            matches!(obj.tag.as_str(),
                                "SubClip" | "VideoClip" | "AudioClip" | "MasterClip" |
                                "VideoMediaSource" | "AudioMediaSource" | "Clip" | "Source"
                            )
                        }))
                        .or_else(|| target_objs.first());

                    if let Some(obj) = target_obj {
                        // If it's a media source, look for its Media reference
                        if obj.tag == "VideoMediaSource" || obj.tag == "AudioMediaSource" {
                            if let Some(media) = self.find_media_for_clip(target, state, depth + 1) {
                                return Some(media);
                            }
                        }
                        // Otherwise, keep following the chain
                        if let Some(media) = self.find_media_for_clip(target, state, depth + 1) {
                            return Some(media);
                        }
                    }
                }
            }
        }

        None
    }

    fn parse_sequence_from_obj(&self, id: &str, obj: &XmlObject) -> Option<Sequence> {
        // Try various ways to find the sequence name
        // Premiere stores names in <Name> element (direct child) or <n> element
        let name = obj
            .attributes
            .get("Name")
            .or_else(|| obj.attributes.get("ObjectName"))
            // Direct child element <Name>
            .or_else(|| obj.children.get("Name").and_then(|v| v.first()))
            // Some objects use <n> for name
            .or_else(|| obj.children.get("n").and_then(|v| v.first()))
            .cloned()
            .unwrap_or_else(|| format!("Sequence {}", id));

        // Extract duration from MZ.OutPoint (in ticks)
        // 254016000000 ticks per second
        const TICKS_PER_SECOND: i64 = 254016000000;
        let duration_ticks = obj.children.get("MZ.OutPoint")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let duration_seconds = if duration_ticks > 0 {
            duration_ticks as f64 / TICKS_PER_SECOND as f64
        } else {
            0.0
        };

        // Log what we found for debugging
        tracing::info!("Sequence '{}' ({}): MZ.OutPoint={}, duration={:.1}s",
            name, id, duration_ticks, duration_seconds);

        Some(Sequence {
            object_id: id.to_string(),
            name,
            duration_ticks,
            frame_rate: FrameRate {
                numerator: 24000,
                denominator: 1001,
            },
            video_tracks: Vec::new(),
            audio_tracks: Vec::new(),
            nested_sequences: Vec::new(),
        })
    }

    /// Populate sequence tracks by following the reference chain from sequence to clips
    fn populate_sequence_tracks(
        &self,
        sequence: &mut Sequence,
        sequence_uid: &str,
        state: &ParserState,
        clip_to_media: &HashMap<String, String>,
    ) {
        // Find all clips that belong to this sequence by following the reference chain:
        // Sequence -> VideoTracks/AudioTracks -> Track -> ClipItems -> ClipTrackItem

        let mut video_clips: Vec<TrackClip> = Vec::new();
        let mut audio_clips: Vec<TrackClip> = Vec::new();

        // Get refs from this sequence (by UID)
        if let Some(seq_refs) = state.refs_from_uid.get(sequence_uid) {
            tracing::debug!("Sequence '{}' has {} refs by UID", sequence.name, seq_refs.len());
            for (ref_tag, target, is_guid) in seq_refs {
                tracing::debug!("  Ref: {} -> {} (guid: {})", ref_tag, target, is_guid);
                // Look for VideoTracks and AudioTracks references
                if ref_tag == "VideoTracks" || ref_tag == "AudioTracks" {
                    let is_video = ref_tag == "VideoTracks";
                    // Follow to find tracks
                    self.find_clips_in_tracks(target, *is_guid, is_video, state, clip_to_media,
                        if is_video { &mut video_clips } else { &mut audio_clips });
                }
            }
        } else {
            tracing::debug!("Sequence '{}' has NO refs by UID", sequence.name);
        }

        // Also check refs by ID (some sequences might use ID)
        if let Some(seq_refs) = state.refs_from_id.get(sequence_uid) {
            tracing::debug!("Sequence '{}' has {} refs by ID", sequence.name, seq_refs.len());
            for (ref_tag, target, is_guid) in seq_refs {
                if ref_tag == "VideoTracks" || ref_tag == "AudioTracks" {
                    let is_video = ref_tag == "VideoTracks";
                    self.find_clips_in_tracks(target, *is_guid, is_video, state, clip_to_media,
                        if is_video { &mut video_clips } else { &mut audio_clips });
                }
            }
        }

        // FALLBACK: If we didn't find clips through the reference chain,
        // add all clips with media to this sequence (simplified approach)
        // This ensures Used Media shows SOMETHING while we debug the proper hierarchy
        if video_clips.is_empty() && audio_clips.is_empty() {
            tracing::warn!("Sequence '{}': No clips found via refs, using fallback with {} clips",
                sequence.name, clip_to_media.len());

            // Add all clips that have media references
            let mut found_video = 0;
            let mut found_audio = 0;
            let mut not_found = 0;

            for (clip_id, media_uid) in clip_to_media {
                if let Some(objs) = state.objects_by_id.get(clip_id) {
                    for obj in objs {
                        // Get timeline position from track item
                        let start_ticks = obj.children.get("Start")
                            .and_then(|v| v.first())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        let end_ticks = obj.children.get("End")
                            .and_then(|v| v.first())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);

                        // InPoint/OutPoint are NOT on the track item - they're on the Clip object
                        // Reference chain: TrackItem -> SubClip -> Clip
                        // We need to follow this chain to get the source media in/out points
                        let (in_point_ticks, out_point_ticks) = self.get_clip_in_out_points(
                            clip_id, state, end_ticks - start_ticks
                        );

                        // Log the first few clips for debugging (use WARN to ensure visibility)
                        if found_video + found_audio < 5 {
                            const TICKS_PER_SEC: f64 = 254016000000.0;
                            let duration_sec = (out_point_ticks - in_point_ticks) as f64 / TICKS_PER_SEC;
                            tracing::warn!(
                                "Clip {}: source in={:.3}s out={:.3}s duration={:.3}s",
                                clip_id,
                                in_point_ticks as f64 / TICKS_PER_SEC,
                                out_point_ticks as f64 / TICKS_PER_SEC,
                                duration_sec,
                            );
                        }

                        let clip = TrackClip {
                            object_id: clip_id.clone(),
                            name: String::new(),
                            start_ticks,
                            end_ticks,
                            in_point_ticks,
                            out_point_ticks,
                            media_ref: Some(media_uid.clone()),
                            clip_type: ClipType::Standard,
                            speed: 1.0,
                        };

                        if obj.tag == "VideoClipTrackItem" {
                            video_clips.push(clip);
                            found_video += 1;
                        } else if obj.tag == "AudioClipTrackItem" {
                            audio_clips.push(clip);
                            found_audio += 1;
                        }
                        break;
                    }
                } else {
                    not_found += 1;
                }
            }

            tracing::warn!("Fallback result: {} video, {} audio, {} not found in objects_by_id",
                found_video, found_audio, not_found);
        }

        // Create a single track for each type with all clips
        if !video_clips.is_empty() {
            sequence.video_tracks.push(Track {
                object_id: format!("{}_video", sequence_uid),
                name: "Video".to_string(),
                track_type: TrackType::Video,
                clips: video_clips,
            });
        }

        if !audio_clips.is_empty() {
            sequence.audio_tracks.push(Track {
                object_id: format!("{}_audio", sequence_uid),
                name: "Audio".to_string(),
                track_type: TrackType::Audio,
                clips: audio_clips,
            });
        }

        tracing::info!("Sequence '{}': {} video clips, {} audio clips",
            sequence.name,
            sequence.video_tracks.iter().map(|t| t.clips.len()).sum::<usize>(),
            sequence.audio_tracks.iter().map(|t| t.clips.len()).sum::<usize>());
    }

    /// Recursively find clips within tracks container
    fn find_clips_in_tracks(
        &self,
        target_id: &str,
        is_guid: bool,
        is_video: bool,
        state: &ParserState,
        clip_to_media: &HashMap<String, String>,
        clips: &mut Vec<TrackClip>,
    ) {
        // Get refs from this object to find nested tracks or clips
        let refs = if is_guid {
            state.refs_from_uid.get(target_id)
        } else {
            state.refs_from_id.get(target_id)
        };

        if let Some(refs) = refs {
            for (ref_tag, nested_target, nested_is_guid) in refs {
                // Look for Track references or ClipItems
                if ref_tag.contains("Track") || ref_tag == "ClipItems" {
                    self.find_clips_in_tracks(nested_target, *nested_is_guid, is_video, state, clip_to_media, clips);
                }
                // Look for actual clip track items
                if ref_tag == "VideoClipTrackItem" || ref_tag == "AudioClipTrackItem" {
                    if let Some(media_uid) = clip_to_media.get(nested_target) {
                        clips.push(TrackClip {
                            object_id: nested_target.clone(),
                            name: String::new(),
                            start_ticks: 0,
                            end_ticks: 0,
                            in_point_ticks: 0,
                            out_point_ticks: 0,
                            media_ref: Some(media_uid.clone()),
                            clip_type: ClipType::Standard,
                            speed: 1.0,
                        });
                    }
                }
            }
        }

        // Also check if target_id itself is a clip
        if let Some(media_uid) = clip_to_media.get(target_id) {
            // Check if this ID corresponds to a clip track item
            if let Some(objs) = state.objects_by_id.get(target_id) {
                for obj in objs {
                    if obj.tag == "VideoClipTrackItem" || obj.tag == "AudioClipTrackItem" {
                        clips.push(TrackClip {
                            object_id: target_id.to_string(),
                            name: String::new(),
                            start_ticks: 0,
                            end_ticks: 0,
                            in_point_ticks: 0,
                            out_point_ticks: 0,
                            media_ref: Some(media_uid.clone()),
                            clip_type: ClipType::Standard,
                            speed: 1.0,
                        });
                        break;
                    }
                }
            }
        }
    }

    /// Follow reference chain from TrackItem -> SubClip -> Clip to get InPoint/OutPoint
    /// The track item only has timeline position (Start/End), not source media times.
    /// Returns (in_point_ticks, out_point_ticks) from the Clip object.
    fn get_clip_in_out_points(
        &self,
        track_item_id: &str,
        state: &ParserState,
        timeline_duration: i64,
    ) -> (i64, i64) {
        // Maximum reasonable InPoint: 24 hours in ticks
        // If InPoint is larger than this, it's probably not a real source media time
        const MAX_REASONABLE_TICKS: i64 = 254016000000 * 86400; // 24 hours

        // Follow: TrackItem -> SubClip ref -> Clip ref -> InPoint/OutPoint
        if let Some(refs) = state.refs_from_id.get(track_item_id) {
            for (ref_tag, target_id, _is_guid) in refs {
                if ref_tag == "SubClip" {
                    // Found SubClip reference, now follow to Clip
                    if let Some(subclip_refs) = state.refs_from_id.get(target_id) {
                        for (ref_tag2, clip_id, _) in subclip_refs {
                            if ref_tag2 == "Clip" {
                                // Found Clip reference, get InPoint/OutPoint from it
                                if let Some(clip_objs) = state.objects_by_id.get(clip_id) {
                                    for clip_obj in clip_objs {
                                        // Skip synthetic/adjustment clips - they don't have real source media
                                        let is_synthetic = clip_obj.children.contains_key("AdjustmentLayer")
                                            || clip_obj.children.contains_key("SyntheticMedia")
                                            || clip_obj.children.keys().any(|k| k.contains("SyntheticMedia"));

                                        if is_synthetic {
                                            tracing::debug!(
                                                "Clip {} is synthetic/adjustment layer, using timeline duration",
                                                clip_id
                                            );
                                            if timeline_duration > 0 {
                                                return (0, timeline_duration);
                                            }
                                            continue;
                                        }

                                        let in_point = clip_obj.children.get("InPoint")
                                            .and_then(|v| v.first())
                                            .and_then(|s| s.parse::<i64>().ok())
                                            .unwrap_or(0);
                                        let out_point = clip_obj.children.get("OutPoint")
                                            .and_then(|v| v.first())
                                            .and_then(|s| s.parse::<i64>().ok())
                                            .unwrap_or(0);

                                        // Sanity check: if InPoint is unreasonably large (> 24 hours),
                                        // this might be a composition clip, not source media
                                        if in_point > MAX_REASONABLE_TICKS {
                                            tracing::debug!(
                                                "Clip {} has unreasonable InPoint {}, using timeline duration",
                                                clip_id, in_point
                                            );
                                            if timeline_duration > 0 {
                                                return (0, timeline_duration);
                                            }
                                            continue;
                                        }

                                        if out_point > in_point {
                                            return (in_point, out_point);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback: if we couldn't find via ref chain, use timeline duration
        // This assumes the whole clip is used starting from 0
        if timeline_duration > 0 {
            return (0, timeline_duration);
        }

        (0, 0)
    }

    fn parse_bin_from_obj(&self, id: &str, obj: &XmlObject) -> Option<Bin> {
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
