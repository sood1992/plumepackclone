//! FFmpeg integration for media processing
//!
//! Handles lossless trimming, transcoding, and metadata extraction.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Media metadata extracted via FFprobe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub file_path: PathBuf,
    pub format: FormatInfo,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub duration_seconds: f64,
    pub file_size: u64,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatInfo {
    pub format_name: String,
    pub format_long_name: String,
    pub duration: f64,
    pub bit_rate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStream {
    pub index: usize,
    pub codec_name: String,
    pub codec_long_name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bit_rate: Option<u64>,
    pub pix_fmt: Option<String>,
    pub is_lossless_trimmable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: usize,
    pub codec_name: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub bit_rate: Option<u64>,
}

/// Processing mode for consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingMode {
    /// Lossless trimming using stream copy
    Trim {
        start_seconds: f64,
        end_seconds: f64,
        handle_seconds: f64,
    },
    /// Transcode to a different format
    Transcode {
        start_seconds: f64,
        end_seconds: f64,
        preset: TranscodePreset,
    },
    /// Copy file without modification
    Copy,
    /// No processing, just update references
    NoProcess,
}

/// Transcode presets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TranscodePreset {
    ProRes422,
    ProRes422HQ,
    ProRes422LT,
    ProRes4444,
    DNxHD,
    DNxHR,
    H264High,
    H264Medium,
    H265High,
    H265Medium,
    Custom {
        video_codec: String,
        video_bitrate: Option<String>,
        audio_codec: String,
        audio_bitrate: Option<String>,
        extra_args: Vec<String>,
    },
}

impl TranscodePreset {
    pub fn to_ffmpeg_args(&self) -> Vec<String> {
        match self {
            Self::ProRes422 => vec![
                "-c:v".to_string(),
                "prores_ks".to_string(),
                "-profile:v".to_string(),
                "2".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::ProRes422HQ => vec![
                "-c:v".to_string(),
                "prores_ks".to_string(),
                "-profile:v".to_string(),
                "3".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::ProRes422LT => vec![
                "-c:v".to_string(),
                "prores_ks".to_string(),
                "-profile:v".to_string(),
                "1".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::ProRes4444 => vec![
                "-c:v".to_string(),
                "prores_ks".to_string(),
                "-profile:v".to_string(),
                "4".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::DNxHD => vec![
                "-c:v".to_string(),
                "dnxhd".to_string(),
                "-b:v".to_string(),
                "185M".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::DNxHR => vec![
                "-c:v".to_string(),
                "dnxhd".to_string(),
                "-profile:v".to_string(),
                "dnxhr_hq".to_string(),
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
            ],
            Self::H264High => vec![
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "slow".to_string(),
                "-crf".to_string(),
                "18".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "320k".to_string(),
            ],
            Self::H264Medium => vec![
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-crf".to_string(),
                "23".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "192k".to_string(),
            ],
            Self::H265High => vec![
                "-c:v".to_string(),
                "libx265".to_string(),
                "-preset".to_string(),
                "slow".to_string(),
                "-crf".to_string(),
                "18".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "320k".to_string(),
            ],
            Self::H265Medium => vec![
                "-c:v".to_string(),
                "libx265".to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-crf".to_string(),
                "23".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "192k".to_string(),
            ],
            Self::Custom {
                video_codec,
                video_bitrate,
                audio_codec,
                audio_bitrate,
                extra_args,
            } => {
                let mut args = vec!["-c:v".to_string(), video_codec.clone()];
                if let Some(vbr) = video_bitrate {
                    args.extend(["-b:v".to_string(), vbr.clone()]);
                }
                args.extend(["-c:a".to_string(), audio_codec.clone()]);
                if let Some(abr) = audio_bitrate {
                    args.extend(["-b:a".to_string(), abr.clone()]);
                }
                args.extend(extra_args.clone());
                args
            }
        }
    }
}

/// FFmpeg wrapper for media operations
pub struct FFmpeg {
    ffmpeg_path: PathBuf,
    ffprobe_path: PathBuf,
}

impl FFmpeg {
    /// Create new FFmpeg instance, finding the binaries
    pub fn new() -> Result<Self> {
        let ffmpeg_path = Self::find_binary("ffmpeg")?;
        let ffprobe_path = Self::find_binary("ffprobe")?;

        Ok(Self {
            ffmpeg_path,
            ffprobe_path,
        })
    }

    /// Check if FFmpeg is available and return version info
    pub fn check_availability() -> Result<String> {
        let output = Command::new("ffmpeg")
            .args(["-version"])
            .output()
            .context("FFmpeg not found. Please install FFmpeg.")?;

        let version = String::from_utf8_lossy(&output.stdout);
        let first_line = version.lines().next().unwrap_or("Unknown version");
        Ok(first_line.to_string())
    }

    fn find_binary(name: &str) -> Result<PathBuf> {
        // Check PATH first
        if let Ok(output) = Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok(PathBuf::from(path));
            }
        }

        // Check common locations
        let common_paths = [
            format!("/usr/bin/{}", name),
            format!("/usr/local/bin/{}", name),
            format!("/opt/homebrew/bin/{}", name),
        ];

        for path in common_paths {
            let path = PathBuf::from(&path);
            if path.exists() {
                return Ok(path);
            }
        }

        anyhow::bail!("{} not found in PATH or common locations", name)
    }

    /// Get media metadata using FFprobe
    pub fn probe(&self, file_path: &Path) -> Result<MediaMetadata> {
        let output = Command::new(&self.ffprobe_path)
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(file_path)
            .output()
            .context("Failed to run FFprobe")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("FFprobe failed: {}", stderr);
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("Failed to parse FFprobe output")?;

        self.parse_probe_output(&json, file_path)
    }

    fn parse_probe_output(&self, json: &serde_json::Value, file_path: &Path) -> Result<MediaMetadata> {
        let format = json.get("format").context("No format information")?;

        let format_info = FormatInfo {
            format_name: format["format_name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            format_long_name: format["format_long_name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            duration: format["duration"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            bit_rate: format["bit_rate"]
                .as_str()
                .and_then(|s| s.parse().ok()),
        };

        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();

        if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
            for (i, stream) in streams.iter().enumerate() {
                let codec_type = stream["codec_type"].as_str().unwrap_or("");

                match codec_type {
                    "video" => {
                        let codec_name = stream["codec_name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        let frame_rate = stream["r_frame_rate"]
                            .as_str()
                            .and_then(|s| {
                                let parts: Vec<&str> = s.split('/').collect();
                                if parts.len() == 2 {
                                    let num: f64 = parts[0].parse().ok()?;
                                    let den: f64 = parts[1].parse().ok()?;
                                    if den > 0.0 {
                                        Some(num / den)
                                    } else {
                                        None
                                    }
                                } else {
                                    s.parse().ok()
                                }
                            })
                            .unwrap_or(24.0);

                        video_streams.push(VideoStream {
                            index: i,
                            codec_name: codec_name.clone(),
                            codec_long_name: stream["codec_long_name"]
                                .as_str()
                                .map(|s| s.to_string()),
                            width: stream["width"].as_u64().unwrap_or(0) as u32,
                            height: stream["height"].as_u64().unwrap_or(0) as u32,
                            frame_rate,
                            bit_rate: stream["bit_rate"]
                                .as_str()
                                .and_then(|s| s.parse().ok()),
                            pix_fmt: stream["pix_fmt"].as_str().map(|s| s.to_string()),
                            is_lossless_trimmable: Self::is_lossless_trimmable(&codec_name),
                        });
                    }
                    "audio" => {
                        audio_streams.push(AudioStream {
                            index: i,
                            codec_name: stream["codec_name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            sample_rate: stream["sample_rate"]
                                .as_str()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(48000),
                            channels: stream["channels"].as_u64().unwrap_or(2) as u32,
                            bit_rate: stream["bit_rate"]
                                .as_str()
                                .and_then(|s| s.parse().ok()),
                        });
                    }
                    _ => {}
                }
            }
        }

        let file_size = std::fs::metadata(file_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(MediaMetadata {
            file_path: file_path.to_path_buf(),
            format: format_info.clone(),
            video_streams,
            audio_streams,
            duration_seconds: format_info.duration,
            file_size,
            bit_rate: format_info.bit_rate,
        })
    }

    /// Check if codec supports lossless trimming (stream copy)
    fn is_lossless_trimmable(codec_name: &str) -> bool {
        matches!(
            codec_name.to_lowercase().as_str(),
            "prores" | "prores_ks" | "dnxhd" | "dnxhr" |
            "h264" | "avc" | "h265" | "hevc" |
            "mjpeg" | "jpeg2000" |
            "cineform" | "cfhd" |
            "v210" | "v410" |
            "rawvideo" |
            // Image sequences are inherently lossless trimmable
            "png" | "tiff" | "dpx" | "exr"
        )
    }

    /// Trim media losslessly using stream copy
    pub fn trim_lossless(
        &self,
        input: &Path,
        output: &Path,
        start_seconds: f64,
        end_seconds: f64,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<()> {
        let duration = end_seconds - start_seconds;

        // Use OUTPUT seeking (ss after -i) for more reliable stream copy
        // Input seeking can cause corrupted output when not landing on keyframes
        let mut cmd = Command::new(&self.ffmpeg_path);
        cmd.args(["-y", "-i"])
        .arg(input)
        .args([
            // Output seeking - more accurate with stream copy
            "-ss",
            &format!("{:.6}", start_seconds),
            "-t",
            &format!("{:.6}", duration),
            "-c",
            "copy",
            // Only map video and audio streams, not data/metadata streams
            // Data streams (rtmd, timecode, etc.) often can't be written to MP4 containers
            "-map",
            "0:v?",    // Map all video streams (? = optional if none exist)
            "-map",
            "0:a?",    // Map all audio streams (? = optional if none exist)
            // Avoid negative timestamps and fix timestamp issues
            "-avoid_negative_ts",
            "make_zero",
            // Reset timestamps so output starts at 0
            "-reset_timestamps",
            "1",
        ])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to start FFmpeg")?;

        // Monitor for cancellation
        if let Some(flag) = cancel_flag {
            while let Ok(None) = child.try_wait() {
                if flag.load(Ordering::Relaxed) {
                    child.kill().ok();
                    anyhow::bail!("Operation cancelled");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("FFmpeg trim failed: {}", stderr);
        }

        Ok(())
    }

    /// Transcode media to a different format
    pub fn transcode(
        &self,
        input: &Path,
        output: &Path,
        start_seconds: Option<f64>,
        end_seconds: Option<f64>,
        preset: &TranscodePreset,
        cancel_flag: Option<Arc<AtomicBool>>,
        progress_callback: Option<Box<dyn Fn(f64) + Send>>,
    ) -> Result<()> {
        let mut args = vec!["-y".to_string()];

        // Add seek if start time specified
        if let Some(start) = start_seconds {
            args.extend(["-ss".to_string(), format!("{:.6}", start)]);
        }

        args.extend(["-i".to_string(), input.to_string_lossy().to_string()]);

        // Add duration if end time specified
        if let (Some(start), Some(end)) = (start_seconds, end_seconds) {
            args.extend(["-t".to_string(), format!("{:.6}", end - start)]);
        }

        // Add preset arguments
        args.extend(preset.to_ffmpeg_args());

        // Add output
        args.push(output.to_string_lossy().to_string());

        let mut cmd = Command::new(&self.ffmpeg_path);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to start FFmpeg")?;

        // Monitor for cancellation and progress
        if let Some(flag) = cancel_flag {
            while let Ok(None) = child.try_wait() {
                if flag.load(Ordering::Relaxed) {
                    child.kill().ok();
                    anyhow::bail!("Operation cancelled");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("FFmpeg transcode failed: {}", stderr);
        }

        Ok(())
    }

    /// Get keyframe positions for accurate trimming
    pub fn get_keyframes(&self, file_path: &Path) -> Result<Vec<f64>> {
        let output = Command::new(&self.ffprobe_path)
            .args([
                "-v",
                "quiet",
                "-select_streams",
                "v:0",
                "-show_entries",
                "packet=pts_time,flags",
                "-of",
                "csv=print_section=0",
            ])
            .arg(file_path)
            .output()
            .context("Failed to run FFprobe for keyframes")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let keyframes: Vec<f64> = output_str
            .lines()
            .filter(|line| line.contains('K'))
            .filter_map(|line| {
                let pts = line.split(',').next()?;
                pts.parse().ok()
            })
            .collect();

        Ok(keyframes)
    }

    /// Find the nearest keyframe before a given time
    pub fn find_nearest_keyframe(&self, file_path: &Path, target_time: f64) -> Result<f64> {
        let keyframes = self.get_keyframes(file_path)?;

        let nearest = keyframes
            .iter()
            .filter(|&&t| t <= target_time)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .copied()
            .unwrap_or(0.0);

        Ok(nearest)
    }
}

/// Calculate estimated output size for trimmed media
pub fn estimate_trimmed_size(
    original_size: u64,
    original_duration: f64,
    trimmed_duration: f64,
) -> u64 {
    if original_duration <= 0.0 {
        return original_size;
    }
    ((original_size as f64) * (trimmed_duration / original_duration)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_lossless_trimmable() {
        assert!(FFmpeg::is_lossless_trimmable("prores"));
        assert!(FFmpeg::is_lossless_trimmable("h264"));
        assert!(FFmpeg::is_lossless_trimmable("dnxhd"));
        assert!(!FFmpeg::is_lossless_trimmable("unknown_codec"));
    }

    #[test]
    fn test_estimate_trimmed_size() {
        let original_size = 1_000_000_000; // 1GB
        let original_duration = 60.0; // 1 minute
        let trimmed_duration = 30.0; // 30 seconds

        let estimated = estimate_trimmed_size(original_size, original_duration, trimmed_duration);
        assert_eq!(estimated, 500_000_000); // 500MB
    }

    #[test]
    fn test_transcode_preset_args() {
        let preset = TranscodePreset::ProRes422;
        let args = preset.to_ffmpeg_args();
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"prores_ks".to_string()));
    }
}
