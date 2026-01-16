//! Standalone test for the prproj parser
//! Run with: cargo run --release -- <path_to_prproj>

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

fn main() -> Result<()> {
    // Set up logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: parser-test <path_to_prproj>");
        eprintln!("Example: parser-test /path/to/project.prproj");
        std::process::exit(1);
    }

    let prproj_path = PathBuf::from(&args[1]);

    if !prproj_path.exists() {
        eprintln!("File not found: {:?}", prproj_path);
        std::process::exit(1);
    }

    println!("Parsing: {:?}", prproj_path);

    // Decompress and parse
    let xml_content = decompress_project(&prproj_path)?;
    println!("Decompressed XML: {} bytes", xml_content.len());

    // Parse the XML
    parse_xml(&xml_content)?;

    Ok(())
}

fn decompress_project(file_path: &Path) -> Result<String> {
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open project file: {:?}", file_path))?;

    let buf_reader = BufReader::new(file);
    let mut decoder = GzDecoder::new(buf_reader);
    let mut xml_content = String::new();

    decoder
        .read_to_string(&mut xml_content)
        .with_context(|| "Failed to decompress project file")?;

    Ok(xml_content)
}

#[derive(Debug, Default, Clone)]
struct XmlObject {
    tag: String,
    #[allow(dead_code)]
    object_id: Option<String>,
    #[allow(dead_code)]
    object_uid: Option<String>,
    attributes: HashMap<String, String>,
    children: HashMap<String, Vec<String>>,
}

struct ParserState {
    current_element: Vec<String>,
    objects_by_id: HashMap<String, Vec<XmlObject>>,
    objects_by_uid: HashMap<String, XmlObject>,
    /// Stack of object contexts - tracks (ObjectID, ObjectUID) for each nested element
    object_context_stack: Vec<(Option<String>, Option<String>)>,
    current_text: String,
    refs_from_id: HashMap<String, Vec<(String, String, bool)>>,
    refs_from_uid: HashMap<String, Vec<(String, String, bool)>>,
    media_file_paths: HashMap<String, PathBuf>,
}

impl ParserState {
    fn current_object_id(&self) -> Option<String> {
        for (obj_id, _) in self.object_context_stack.iter().rev() {
            if obj_id.is_some() {
                return obj_id.clone();
            }
        }
        None
    }

    fn current_object_uid(&self) -> Option<String> {
        for (_, obj_uid) in self.object_context_stack.iter().rev() {
            if obj_uid.is_some() {
                return obj_uid.clone();
            }
        }
        None
    }
}

fn parse_xml(xml_content: &str) -> Result<()> {
    let mut reader = Reader::from_str(xml_content);
    reader.config_mut().trim_text(true);

    let mut state = ParserState {
        current_element: Vec::new(),
        objects_by_id: HashMap::new(),
        objects_by_uid: HashMap::new(),
        object_context_stack: Vec::new(),
        current_text: String::new(),
        refs_from_id: HashMap::new(),
        refs_from_uid: HashMap::new(),
        media_file_paths: HashMap::new(),
    };

    let mut buf = Vec::new();
    let mut version = 0u32;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                state.current_element.push(tag_name.clone());

                let attrs: Vec<(String, String)> = e.attributes()
                    .flatten()
                    .map(|attr| {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let value = String::from_utf8_lossy(&attr.value).to_string();
                        (key, value)
                    })
                    .collect();

                process_element_attrs(&tag_name, &attrs, &mut state, false);

                if tag_name == "PremiereData" {
                    for (key, value) in &attrs {
                        if key == "Version" {
                            if let Ok(v) = value.parse() {
                                version = v;
                            }
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                state.current_element.pop();
                state.object_context_stack.pop();
                state.current_text.clear();
            }
            Ok(Event::Empty(e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                let attrs: Vec<(String, String)> = e.attributes()
                    .flatten()
                    .map(|attr| {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let value = String::from_utf8_lossy(&attr.value).to_string();
                        (key, value)
                    })
                    .collect();

                process_element_attrs(&tag_name, &attrs, &mut state, true);
            }
            Ok(Event::Text(e)) => {
                state.current_text = e.unescape().unwrap_or_default().to_string();

                let current_tag = state.current_element.last().map(|s| s.as_str()).unwrap_or("");
                let text = &state.current_text;

                // Check for media file paths
                let is_file_path_element = matches!(current_tag,
                    "ActualMediaFilePath" | "FilePath" | "MediaFilePath"
                );

                let is_absolute_path = text.starts_with('/') ||
                    (text.len() > 2 && text.chars().nth(1) == Some(':'));

                let looks_like_path = text.len() > 5 &&
                    is_absolute_path &&
                    !text.contains("Peak Files") &&
                    !text.contains("Audio Previews") &&
                    !text.ends_with(".pek") &&
                    !text.ends_with(".cfa");

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

                if is_file_path_element && looks_like_path && has_media_extension {
                    let parent_uid = state.current_object_uid()
                        .or_else(|| state.current_object_id())
                        .unwrap_or_else(|| "unknown".to_string());
                    println!("  Found media file: {} (Media UID: {})", text, parent_uid);
                    state.media_file_paths.insert(parent_uid, PathBuf::from(text.clone()));
                }

                // Store text content in current object
                let child_key = current_tag.to_string();

                if let Some(obj_uid) = state.current_object_uid() {
                    if let Some(obj) = state.objects_by_uid.get_mut(&obj_uid) {
                        if !state.current_text.is_empty() {
                            obj.children.entry(child_key.clone()).or_default().push(state.current_text.clone());
                        }
                    }
                }
                if let Some(obj_id) = state.current_object_id() {
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
                eprintln!("XML parsing error: {:?}", e);
                continue;
            }
            _ => {}
        }
        buf.clear();
    }

    // Print summary
    let total_objects_by_id: usize = state.objects_by_id.values().map(|v| v.len()).sum();
    println!("\n=== Parsing Summary ===");
    println!("Project version: {}", version);
    println!("Unique numeric IDs: {} (total objects: {})", state.objects_by_id.len(), total_objects_by_id);
    println!("Objects by GUID: {}", state.objects_by_uid.len());
    println!("Refs from numeric IDs: {}", state.refs_from_id.values().map(|v| v.len()).sum::<usize>());
    println!("Refs from GUIDs: {}", state.refs_from_uid.values().map(|v| v.len()).sum::<usize>());
    println!("Media file paths: {}", state.media_file_paths.len());

    // Process sequences
    const SEQUENCE_CLASS_ID: &str = "6a15d903-8739-11d5-af2d-9b7855ad8974";
    const TICKS_PER_SECOND: i64 = 254016000000;

    println!("\n=== Sequences ===");
    let mut seq_count = 0;
    for (uid, obj) in &state.objects_by_uid {
        if obj.tag == "Sequence" {
            let class_id = obj.attributes.get("ClassID").map(|s| s.as_str()).unwrap_or("");
            if class_id == SEQUENCE_CLASS_ID {
                seq_count += 1;

                let name = obj.attributes.get("Name")
                    .or_else(|| obj.attributes.get("ObjectName"))
                    .or_else(|| obj.children.get("Name").and_then(|v| v.first()))
                    .or_else(|| obj.children.get("n").and_then(|v| v.first()))
                    .cloned()
                    .unwrap_or_else(|| format!("Sequence {}", uid));

                let duration_ticks = obj.children.get("MZ.OutPoint")
                    .and_then(|v| v.first())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);

                let duration_seconds = if duration_ticks > 0 {
                    duration_ticks as f64 / TICKS_PER_SECOND as f64
                } else {
                    0.0
                };

                // Format as HH:MM:SS
                let hours = (duration_seconds / 3600.0) as u32;
                let minutes = ((duration_seconds % 3600.0) / 60.0) as u32;
                let seconds = (duration_seconds % 60.0) as u32;

                println!("  {}. '{}' - Duration: {:02}:{:02}:{:02} ({} ticks)",
                    seq_count, name, hours, minutes, seconds, duration_ticks);

                // Show all children for debugging
                println!("     Children: {:?}", obj.children.keys().collect::<Vec<_>>());
            }
        }
    }
    println!("Total sequences: {}", seq_count);

    // Process clip track items
    let clip_items: Vec<(&String, &XmlObject)> = state.objects_by_id.iter()
        .flat_map(|(id, objs)| objs.iter().map(move |obj| (id, obj)))
        .filter(|(_, obj)| obj.tag == "VideoClipTrackItem" || obj.tag == "AudioClipTrackItem")
        .collect();

    println!("\n=== Clip Track Items ===");
    println!("Total clip track items: {}", clip_items.len());

    // Debug: Show sample clip IDs
    println!("\nSample clip IDs (first 5):");
    for (i, (clip_id, clip_obj)) in clip_items.iter().take(5).enumerate() {
        println!("  {}. ID='{}' tag={}", i+1, clip_id, clip_obj.tag);
    }

    // Debug: Show sample refs_from_id keys
    println!("\nSample refs_from_id keys (first 10):");
    for (i, (key, refs)) in state.refs_from_id.iter().take(10).enumerate() {
        println!("  {}. key='{}' -> {} refs", i+1, key, refs.len());
    }

    // Debug: Check if any clip IDs exist in refs_from_id
    let clip_ids: std::collections::HashSet<&String> = clip_items.iter().map(|(id, _)| *id).collect();
    let ref_keys: std::collections::HashSet<&String> = state.refs_from_id.keys().collect();
    let intersection: Vec<_> = clip_ids.intersection(&ref_keys).collect();
    println!("\nClip IDs that have refs: {} (out of {} clips)", intersection.len(), clip_items.len());
    for id in intersection.iter().take(5) {
        println!("  - {}", id);
    }

    let mut clips_with_refs = 0;
    let mut clips_without_refs = 0;
    let mut clips_resolved_to_media = 0;
    let mut used_media_uids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Show first few clips with refs for debugging
    let mut shown_clips = 0;

    for (clip_id, _clip_obj) in &clip_items {
        let refs_by_id = state.refs_from_id.get(*clip_id);
        let refs_by_uid = state.refs_from_uid.get(*clip_id);
        let has_refs = refs_by_id.is_some() || refs_by_uid.is_some();

        if !has_refs {
            clips_without_refs += 1;
        } else {
            clips_with_refs += 1;
            if shown_clips < 5 {
                println!("\nClip {} refs:", clip_id);
                if let Some(refs) = refs_by_id {
                    for (tag, target, is_guid) in refs {
                        println!("  --{}--> {} (guid: {})", tag, target, is_guid);
                    }
                }
                if let Some(refs) = refs_by_uid {
                    for (tag, target, is_guid) in refs {
                        println!("  --{}--> {} (guid: {})", tag, target, is_guid);
                    }
                }
                shown_clips += 1;
            }
        }

        if let Some(media_uid) = find_media_for_clip(clip_id, &state, 0) {
            clips_resolved_to_media += 1;
            if clips_resolved_to_media <= 5 {
                println!("SUCCESS: Clip {} -> Media {}", clip_id, media_uid);
            }
            used_media_uids.insert(media_uid);
        }
    }

    println!("\n=== Clip Summary ===");
    println!("Clips with refs: {}", clips_with_refs);
    println!("Clips without refs: {}", clips_without_refs);
    println!("Clips resolved to media: {}", clips_resolved_to_media);
    println!("Unique media files used: {}", used_media_uids.len());

    // Simulate what populate_sequence_tracks does
    println!("\n=== Simulating populate_sequence_tracks Fallback ===");

    // Build clip_to_media mapping (same as project_parser.rs)
    let mut clip_to_media: HashMap<String, String> = HashMap::new();
    for (clip_id, _) in &clip_items {
        if let Some(media_uid) = find_media_for_clip(clip_id, &state, 0) {
            clip_to_media.insert(clip_id.to_string(), media_uid);
        }
    }
    println!("clip_to_media has {} entries", clip_to_media.len());

    // Now simulate the fallback
    let mut video_clips = 0;
    let mut audio_clips = 0;
    let mut not_found = 0;
    let mut other_tag = 0;

    for (clip_id, media_uid) in &clip_to_media {
        if let Some(objs) = state.objects_by_id.get(clip_id) {
            let mut found_this = false;
            for obj in objs {
                if obj.tag == "VideoClipTrackItem" {
                    video_clips += 1;
                    found_this = true;
                    break;
                } else if obj.tag == "AudioClipTrackItem" {
                    audio_clips += 1;
                    found_this = true;
                    break;
                }
            }
            if !found_this {
                other_tag += 1;
                // Show what tag it actually has
                if other_tag <= 5 {
                    let tags: Vec<_> = objs.iter().map(|o| o.tag.as_str()).collect();
                    println!("  Clip {} has tags: {:?} but not Video/AudioClipTrackItem", clip_id, tags);
                }
            }
        } else {
            not_found += 1;
            if not_found <= 5 {
                println!("  Clip {} not found in objects_by_id", clip_id);
            }
        }
    }

    println!("\nFallback simulation result:");
    println!("  Video clips: {}", video_clips);
    println!("  Audio clips: {}", audio_clips);
    println!("  Not found in objects_by_id: {}", not_found);
    println!("  Other tag types: {}", other_tag);

    // Debug: Show what children clip track items actually have
    println!("\n=== Clip Track Item Children (first 3) ===");
    let mut shown = 0;
    for (clip_id, _) in &clip_to_media {
        if shown >= 3 {
            break;
        }
        if let Some(objs) = state.objects_by_id.get(clip_id) {
            for obj in objs {
                if obj.tag == "VideoClipTrackItem" || obj.tag == "AudioClipTrackItem" {
                    println!("  Clip {} ({}):", clip_id, obj.tag);
                    println!("    All children keys: {:?}", obj.children.keys().collect::<Vec<_>>());
                    // Show values for time-related keys
                    for key in ["Start", "End", "InPoint", "OutPoint", "MZ.Start", "MZ.End",
                                "MZ.InPoint", "MZ.OutPoint", "Duration", "MZ.Duration"] {
                        if let Some(vals) = obj.children.get(key) {
                            println!("    {}: {:?}", key, vals);
                        }
                    }

                    // Now follow refs to find SubClip and show its children
                    if let Some(refs) = state.refs_from_id.get(clip_id) {
                        for (ref_tag, target_id, _is_guid) in refs {
                            if ref_tag == "SubClip" {
                                println!("    -> Following SubClip ref to {}", target_id);
                                if let Some(subclip_objs) = state.objects_by_id.get(target_id) {
                                    for subclip in subclip_objs {
                                        println!("       SubClip {} children: {:?}", target_id, subclip.children.keys().collect::<Vec<_>>());
                                        for key in ["Start", "End", "InPoint", "OutPoint", "StartOffset", "EndOffset"] {
                                            if let Some(vals) = subclip.children.get(key) {
                                                println!("       SubClip {}: {:?}", key, vals);
                                            }
                                        }

                                        // Follow Clip ref from SubClip
                                        if let Some(subclip_refs) = state.refs_from_id.get(target_id) {
                                            for (ref_tag2, target_id2, _) in subclip_refs {
                                                if ref_tag2 == "Clip" {
                                                    println!("       -> Following Clip ref to {}", target_id2);
                                                    if let Some(clip_objs) = state.objects_by_id.get(target_id2) {
                                                        for clip_obj in clip_objs {
                                                            println!("          Clip {} children: {:?}", target_id2, clip_obj.children.keys().collect::<Vec<_>>());
                                                            for key in ["Start", "End", "InPoint", "OutPoint", "Duration"] {
                                                                if let Some(vals) = clip_obj.children.get(key) {
                                                                    println!("          Clip {}: {:?}", key, vals);
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    shown += 1;
                    break;
                }
            }
        }
    }

    // Now simulate what SequenceAnalyzer would do
    println!("\n=== Simulating SequenceAnalyzer ===");
    let mut total_used_media: std::collections::HashSet<String> = std::collections::HashSet::new();

    // First, check what refs sequences actually have
    println!("\n=== Sequence References ===");
    for (uid, obj) in &state.objects_by_uid {
        if obj.tag == "Sequence" {
            let class_id = obj.attributes.get("ClassID").map(|s| s.as_str()).unwrap_or("");
            if class_id == SEQUENCE_CLASS_ID {
                let name = obj.children.get("Name").and_then(|v| v.first()).cloned().unwrap_or_default();

                // Check refs from this sequence's UID
                let refs_by_uid = state.refs_from_uid.get(uid);
                let refs_by_id = state.refs_from_id.get(uid);

                let uid_ref_count = refs_by_uid.map(|v| v.len()).unwrap_or(0);
                let id_ref_count = refs_by_id.map(|v| v.len()).unwrap_or(0);

                println!("  Sequence '{}' (UID={})", name, uid);
                println!("    refs_from_uid: {} refs, refs_from_id: {} refs", uid_ref_count, id_ref_count);

                // Show first 5 refs
                if let Some(refs) = refs_by_uid {
                    for (tag, target, is_guid) in refs.iter().take(5) {
                        println!("      UID ref: {} -> {} (guid: {})", tag, target, is_guid);
                    }
                }
                if let Some(refs) = refs_by_id {
                    for (tag, target, is_guid) in refs.iter().take(5) {
                        println!("      ID ref: {} -> {} (guid: {})", tag, target, is_guid);
                    }
                }

                // Check if sequence has VideoTracks or AudioTracks refs
                let has_video_tracks_ref = refs_by_uid.map_or(false, |refs| refs.iter().any(|(tag, _, _)| tag == "VideoTracks"));
                let has_audio_tracks_ref = refs_by_uid.map_or(false, |refs| refs.iter().any(|(tag, _, _)| tag == "AudioTracks"));
                println!("    Has VideoTracks ref: {}, Has AudioTracks ref: {}", has_video_tracks_ref, has_audio_tracks_ref);
            }
        }
    }

    // For each sequence, count how many clips would be analyzed
    for (uid, obj) in &state.objects_by_uid {
        if obj.tag == "Sequence" {
            let class_id = obj.attributes.get("ClassID").map(|s| s.as_str()).unwrap_or("");
            if class_id == SEQUENCE_CLASS_ID {
                let name = obj.children.get("Name").and_then(|v| v.first()).cloned().unwrap_or_default();

                // In the fallback, ALL clips get added to each sequence
                // So each sequence would have video_clips + audio_clips clips
                // And the media_refs would all be counted

                // Collect unique media from clips
                for (_, media_uid) in &clip_to_media {
                    total_used_media.insert(media_uid.clone());
                }

                println!("  Sequence '{}' would have {} video + {} audio clips",
                    name, video_clips, audio_clips);
            }
        }
    }

    println!("\nTotal unique media that would be marked as used: {}", total_used_media.len());
    println!("Total media files in project: {}", state.media_file_paths.len());

    Ok(())
}

fn process_element_attrs(tag_name: &str, attrs: &[(String, String)], state: &mut ParserState, is_empty: bool) {
    let mut this_object_id: Option<String> = None;
    let mut this_object_uid: Option<String> = None;
    let mut object_ref: Option<String> = None;
    let mut object_uref: Option<String> = None;

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

    // Push context for non-empty elements
    if !is_empty {
        state.object_context_stack.push((this_object_id.clone(), this_object_uid.clone()));
    }

    if this_object_id.is_some() || this_object_uid.is_some() {
        let obj = XmlObject {
            tag: tag_name.to_string(),
            object_id: this_object_id.clone(),
            object_uid: this_object_uid.clone(),
            attributes: attrs.iter().cloned().collect(),
            children: HashMap::new(),
        };

        if let Some(ref id) = this_object_id {
            state.objects_by_id.entry(id.clone()).or_default().push(obj.clone());
        }
        if let Some(ref uid) = this_object_uid {
            state.objects_by_uid.insert(uid.clone(), obj);
        }
    }

    let has_ref = object_ref.is_some() || object_uref.is_some();
    if has_ref {
        // Get the context from BEFORE this element (the parent)
        let parent_context = if !is_empty && state.object_context_stack.len() >= 2 {
            let idx = state.object_context_stack.len() - 2;
            state.object_context_stack[..=idx].iter().rev()
                .find(|(id, uid)| id.is_some() || uid.is_some())
                .cloned()
        } else if is_empty && !state.object_context_stack.is_empty() {
            state.object_context_stack.iter().rev()
                .find(|(id, uid)| id.is_some() || uid.is_some())
                .cloned()
        } else {
            None
        };

        let (source_id, source_uid) = parent_context.unwrap_or((None, None));

        if let Some(ref target) = object_ref {
            // Store under the parent's ID - prefer ID over UID for clip items
            if let Some(ref src_id) = source_id {
                state.refs_from_id.entry(src_id.clone()).or_default()
                    .push((tag_name.to_string(), target.clone(), false));
            } else if let Some(ref src_uid) = source_uid {
                state.refs_from_uid.entry(src_uid.clone()).or_default()
                    .push((tag_name.to_string(), target.clone(), false));
            }
        }

        if let Some(ref target) = object_uref {
            if let Some(ref src_id) = source_id {
                state.refs_from_id.entry(src_id.clone()).or_default()
                    .push((tag_name.to_string(), target.clone(), true));
            } else if let Some(ref src_uid) = source_uid {
                state.refs_from_uid.entry(src_uid.clone()).or_default()
                    .push((tag_name.to_string(), target.clone(), true));
            }
        }
    }
}

fn find_media_for_clip(start_id: &str, state: &ParserState, depth: usize) -> Option<String> {
    if depth > 20 {
        return None;
    }

    let refs_from_id = state.refs_from_id.get(start_id);
    let refs_from_uid = state.refs_from_uid.get(start_id);

    let all_refs: Vec<_> = refs_from_id.into_iter()
        .flat_map(|v| v.iter())
        .chain(refs_from_uid.into_iter().flat_map(|v| v.iter()))
        .collect();

    for (ref_element_tag, target, is_guid) in all_refs {
        if *is_guid {
            if state.media_file_paths.contains_key(target) {
                return Some(target.clone());
            }
            if let Some(target_obj) = state.objects_by_uid.get(target) {
                if target_obj.tag == "Media" {
                    return Some(target.clone());
                }
                if let Some(media) = find_media_for_clip(target, state, depth + 1) {
                    return Some(media);
                }
            }
        } else {
            if let Some(target_objs) = state.objects_by_id.get(target) {
                let expected_type = ref_element_tag.as_str();

                let target_obj = target_objs.iter()
                    .find(|obj| obj.tag == expected_type)
                    .or_else(|| target_objs.iter().find(|obj| {
                        matches!(obj.tag.as_str(),
                            "SubClip" | "VideoClip" | "AudioClip" | "MasterClip" |
                            "VideoMediaSource" | "AudioMediaSource" | "Clip" | "Source"
                        )
                    }))
                    .or_else(|| target_objs.first());

                if let Some(obj) = target_obj {
                    if obj.tag == "VideoMediaSource" || obj.tag == "AudioMediaSource" {
                        if let Some(media) = find_media_for_clip(target, state, depth + 1) {
                            return Some(media);
                        }
                    }
                    if let Some(media) = find_media_for_clip(target, state, depth + 1) {
                        return Some(media);
                    }
                }
            }
        }
    }

    None
}
