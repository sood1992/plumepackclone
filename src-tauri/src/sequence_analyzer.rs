//! Sequence analysis for media usage tracking
//!
//! Analyzes sequences to determine which media is used, at which time ranges,
//! and handles nested sequences recursively.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::project_parser::{ClipType, PremiereProject, TrackClip};

/// Analysis result for media usage in project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUsageAnalysis {
    pub used_media: HashMap<String, MediaUsageInfo>,
    pub unused_media: Vec<String>,
    pub sequences_analyzed: Vec<String>,
}

/// Detailed usage information for a single media file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUsageInfo {
    pub object_id: String,
    pub usage_count: usize,
    pub time_ranges: Vec<TimeRange>,
    pub merged_range: TimeRange,
    pub used_in_sequences: Vec<String>,
    pub is_multicam_angle: bool,
    pub is_merged_component: bool,
}

/// A time range in ticks (Premiere uses 254016000000 ticks per second)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_ticks: i64,
    pub end_ticks: i64,
}

impl TimeRange {
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            start_ticks: start.min(end),
            end_ticks: start.max(end),
        }
    }

    /// Duration in ticks
    pub fn duration(&self) -> i64 {
        self.end_ticks - self.start_ticks
    }

    /// Convert to frames at given frame rate
    pub fn to_frames(&self, ticks_per_frame: i64) -> (i64, i64) {
        (
            self.start_ticks / ticks_per_frame,
            self.end_ticks / ticks_per_frame,
        )
    }

    /// Convert to seconds
    pub fn to_seconds(&self) -> (f64, f64) {
        const TICKS_PER_SECOND: f64 = 254016000000.0;
        (
            self.start_ticks as f64 / TICKS_PER_SECOND,
            self.end_ticks as f64 / TICKS_PER_SECOND,
        )
    }

    /// Add handles (extra frames) to the range
    pub fn with_handles(&self, handle_ticks: i64, max_duration: i64) -> Self {
        Self {
            start_ticks: (self.start_ticks - handle_ticks).max(0),
            end_ticks: (self.end_ticks + handle_ticks).min(max_duration),
        }
    }

    /// Merge overlapping or adjacent ranges
    pub fn merge_with(&self, other: &TimeRange, gap_tolerance: i64) -> Option<TimeRange> {
        // Check if ranges overlap or are close enough
        if self.end_ticks + gap_tolerance >= other.start_ticks
            && other.end_ticks + gap_tolerance >= self.start_ticks
        {
            Some(TimeRange::new(
                self.start_ticks.min(other.start_ticks),
                self.end_ticks.max(other.end_ticks),
            ))
        } else {
            None
        }
    }
}

/// Analyzer for sequences and media usage
pub struct SequenceAnalyzer<'a> {
    project: &'a PremiereProject,
    handle_frames: i64,
    include_unused_multicam_angles: bool,
}

impl<'a> SequenceAnalyzer<'a> {
    /// Premiere ticks per second
    const TICKS_PER_SECOND: i64 = 254016000000;

    pub fn new(project: &'a PremiereProject) -> Self {
        Self {
            project,
            handle_frames: 0,
            include_unused_multicam_angles: true,
        }
    }

    /// Set handle frames to include extra frames before/after clips
    pub fn with_handles(mut self, frames: i64) -> Self {
        self.handle_frames = frames;
        self
    }

    /// Set whether to include all multicam angles or only the active one
    pub fn include_all_multicam_angles(mut self, include: bool) -> Self {
        self.include_unused_multicam_angles = include;
        self
    }

    /// Analyze specific sequences
    pub fn analyze_sequences(&self, sequence_ids: &[String]) -> MediaUsageAnalysis {
        let mut used_media: HashMap<String, MediaUsageInfo> = HashMap::new();
        let mut analyzed_sequences: HashSet<String> = HashSet::new();

        for seq_id in sequence_ids {
            self.analyze_sequence_recursive(seq_id, &mut used_media, &mut analyzed_sequences);
        }

        // Find unused media
        let all_media_ids: HashSet<_> = self.project.media_files.keys().cloned().collect();
        let used_ids: HashSet<_> = used_media.keys().cloned().collect();
        let unused_media: Vec<_> = all_media_ids.difference(&used_ids).cloned().collect();

        MediaUsageAnalysis {
            used_media,
            unused_media,
            sequences_analyzed: analyzed_sequences.into_iter().collect(),
        }
    }

    /// Analyze all sequences in the project
    pub fn analyze_all(&self) -> MediaUsageAnalysis {
        let sequence_ids: Vec<_> = self
            .project
            .sequences
            .iter()
            .map(|s| s.object_id.clone())
            .collect();
        self.analyze_sequences(&sequence_ids)
    }

    fn analyze_sequence_recursive(
        &self,
        sequence_id: &str,
        used_media: &mut HashMap<String, MediaUsageInfo>,
        analyzed: &mut HashSet<String>,
    ) {
        // Prevent infinite recursion
        if analyzed.contains(sequence_id) {
            return;
        }
        analyzed.insert(sequence_id.to_string());

        let sequence = match self.project.sequences.iter().find(|s| s.object_id == sequence_id) {
            Some(s) => s,
            None => {
                tracing::warn!("SequenceAnalyzer: Could not find sequence with id '{}'", sequence_id);
                return;
            }
        };

        let video_clip_count: usize = sequence.video_tracks.iter().map(|t| t.clips.len()).sum();
        let audio_clip_count: usize = sequence.audio_tracks.iter().map(|t| t.clips.len()).sum();
        tracing::info!("Analyzing sequence '{}' ({}): {} video tracks ({} clips), {} audio tracks ({} clips)",
            sequence.name, sequence_id,
            sequence.video_tracks.len(), video_clip_count,
            sequence.audio_tracks.len(), audio_clip_count);

        // Analyze video tracks
        for track in &sequence.video_tracks {
            for clip in &track.clips {
                self.analyze_clip(clip, sequence_id, used_media, analyzed);
            }
        }

        // Analyze audio tracks
        for track in &sequence.audio_tracks {
            for clip in &track.clips {
                self.analyze_clip(clip, sequence_id, used_media, analyzed);
            }
        }

        // Analyze nested sequences explicitly listed
        for nested_id in &sequence.nested_sequences {
            self.analyze_sequence_recursive(nested_id, used_media, analyzed);
        }
    }

    fn analyze_clip(
        &self,
        clip: &TrackClip,
        sequence_id: &str,
        used_media: &mut HashMap<String, MediaUsageInfo>,
        analyzed: &mut HashSet<String>,
    ) {
        match &clip.clip_type {
            ClipType::Standard => {
                if let Some(ref media_ref) = clip.media_ref {
                    self.add_media_usage(
                        media_ref,
                        clip,
                        sequence_id,
                        used_media,
                        false,
                        false,
                    );
                }
            }
            ClipType::Subclip { parent_id } => {
                // Use the parent media's range
                self.add_media_usage(parent_id, clip, sequence_id, used_media, false, false);
            }
            ClipType::MergedClip { components } => {
                // Include all component media
                for component_id in components {
                    if let Some(ref media_ref) = clip.media_ref {
                        self.add_media_usage(
                            media_ref,
                            clip,
                            sequence_id,
                            used_media,
                            false,
                            true,
                        );
                    }
                    // Also track the component directly
                    self.add_media_usage(
                        component_id,
                        clip,
                        sequence_id,
                        used_media,
                        false,
                        true,
                    );
                }
            }
            ClipType::Multicam { angles } => {
                for angle in angles {
                    // Include active angle always, others based on setting
                    if angle.is_active || self.include_unused_multicam_angles {
                        self.add_media_usage(
                            &angle.media_ref,
                            clip,
                            sequence_id,
                            used_media,
                            true,
                            false,
                        );
                    }
                }
            }
            ClipType::Nested { sequence_id: nested_id } => {
                // Recursively analyze nested sequence
                self.analyze_sequence_recursive(nested_id, used_media, analyzed);
            }
            ClipType::Adjustment => {
                // Adjustment layers don't reference media
            }
        }
    }

    fn add_media_usage(
        &self,
        media_id: &str,
        clip: &TrackClip,
        sequence_id: &str,
        used_media: &mut HashMap<String, MediaUsageInfo>,
        is_multicam: bool,
        is_merged: bool,
    ) {
        // Calculate the time range from the source media
        let time_range = TimeRange::new(clip.in_point_ticks, clip.out_point_ticks);

        // Get media duration for handle calculations
        let media_duration = self
            .project
            .media_files
            .get(media_id)
            .map(|m| m.duration_ticks)
            .unwrap_or(i64::MAX);

        // Calculate handle ticks (assuming 24fps for now, should be dynamic)
        let handle_ticks = self.handle_frames * (Self::TICKS_PER_SECOND / 24);
        let time_range_with_handles = time_range.with_handles(handle_ticks, media_duration);

        let entry = used_media.entry(media_id.to_string()).or_insert_with(|| {
            MediaUsageInfo {
                object_id: media_id.to_string(),
                usage_count: 0,
                time_ranges: Vec::new(),
                merged_range: time_range_with_handles,
                used_in_sequences: Vec::new(),
                is_multicam_angle: is_multicam,
                is_merged_component: is_merged,
            }
        });

        entry.usage_count += 1;
        entry.time_ranges.push(time_range_with_handles);

        if !entry.used_in_sequences.contains(&sequence_id.to_string()) {
            entry.used_in_sequences.push(sequence_id.to_string());
        }

        // Update merged range
        entry.merged_range = TimeRange::new(
            entry.merged_range.start_ticks.min(time_range_with_handles.start_ticks),
            entry.merged_range.end_ticks.max(time_range_with_handles.end_ticks),
        );

        // Update flags
        entry.is_multicam_angle |= is_multicam;
        entry.is_merged_component |= is_merged;
    }
}

/// Optimize time ranges by merging overlapping/adjacent ones
pub fn optimize_time_ranges(ranges: &[TimeRange], gap_tolerance: i64) -> Vec<TimeRange> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<_> = ranges.to_vec();
    sorted.sort_by_key(|r| r.start_ticks);

    let mut optimized = vec![sorted[0]];

    for range in sorted.into_iter().skip(1) {
        let last = optimized.last_mut().unwrap();
        if let Some(merged) = last.merge_with(&range, gap_tolerance) {
            *last = merged;
        } else {
            optimized.push(range);
        }
    }

    optimized
}

/// Calculate the most common ancestor path for media files
pub fn find_common_ancestor(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    if paths.is_empty() {
        return None;
    }

    let first = paths[0].components().collect::<Vec<_>>();

    let mut common_length = first.len();

    for path in paths.iter().skip(1) {
        let components: Vec<_> = path.components().collect();
        let mut match_len = 0;

        for (i, comp) in components.iter().enumerate() {
            if i >= common_length || i >= first.len() {
                break;
            }
            if comp == &first[i] {
                match_len = i + 1;
            } else {
                break;
            }
        }

        common_length = common_length.min(match_len);
    }

    if common_length == 0 {
        return None;
    }

    let common_path: std::path::PathBuf = first[..common_length].iter().collect();
    Some(common_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_range_merge() {
        let r1 = TimeRange::new(0, 100);
        let r2 = TimeRange::new(90, 200);
        let r3 = TimeRange::new(300, 400);

        assert!(r1.merge_with(&r2, 0).is_some());
        assert!(r1.merge_with(&r3, 0).is_none());
        assert!(r1.merge_with(&r3, 200).is_some()); // With gap tolerance
    }

    #[test]
    fn test_optimize_ranges() {
        let ranges = vec![
            TimeRange::new(0, 100),
            TimeRange::new(50, 150),
            TimeRange::new(200, 300),
        ];

        let optimized = optimize_time_ranges(&ranges, 0);
        assert_eq!(optimized.len(), 2);
        assert_eq!(optimized[0].start_ticks, 0);
        assert_eq!(optimized[0].end_ticks, 150);
    }

    #[test]
    fn test_common_ancestor() {
        use std::path::PathBuf;

        let paths = vec![
            PathBuf::from("/Users/editor/Projects/MyFilm/Footage/A001.mov"),
            PathBuf::from("/Users/editor/Projects/MyFilm/Footage/A002.mov"),
            PathBuf::from("/Users/editor/Projects/MyFilm/Audio/Boom.wav"),
        ];

        let common = find_common_ancestor(&paths);
        assert!(common.is_some());
        let common = common.unwrap();
        assert!(common.ends_with("MyFilm") || common.to_string_lossy().contains("MyFilm"));
    }
}
