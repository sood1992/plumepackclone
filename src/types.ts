// API Response Types

export interface ProjectInfo {
  name: string;
  file_path: string;
  version: number;
  sequence_count: number;
  media_count: number;
  bin_count: number;
}

export interface SequenceInfo {
  object_id: string;
  name: string;
  duration_seconds: number;
  frame_rate: number;
  video_track_count: number;
  audio_track_count: number;
  nested_count: number;
}

export interface MediaItemInfo {
  object_id: string;
  file_path: string;
  file_name: string;
  file_size: number;
  file_size_formatted: string;
  is_online: boolean;
  media_type: string;
  has_proxy: boolean;
  bin_path: string | null;
}

export interface MediaUsageResult {
  used_count: number;
  unused_count: number;
  used_size: number;
  unused_size: number;
  used_media: UsedMediaItem[];
  unused_media: string[];
}

export interface UsedMediaItem {
  object_id: string;
  file_name: string;
  usage_count: number;
  time_range_seconds: [number, number];
  sequences: string[];
}

export interface MediaMetadata {
  file_path: string;
  format: FormatInfo;
  video_streams: VideoStream[];
  audio_streams: AudioStream[];
  duration_seconds: number;
  file_size: number;
  bit_rate: number | null;
}

export interface FormatInfo {
  format_name: string;
  format_long_name: string;
  duration: number;
  bit_rate: number | null;
}

export interface VideoStream {
  index: number;
  codec_name: string;
  codec_long_name: string | null;
  width: number;
  height: number;
  frame_rate: number;
  bit_rate: number | null;
  pix_fmt: string | null;
  is_lossless_trimmable: boolean;
}

export interface AudioStream {
  index: number;
  codec_name: string;
  sample_rate: number;
  channels: number;
  bit_rate: number | null;
}

export interface ConsolidationProgress {
  job_id: string;
  status: ConsolidationStatus;
  current_file: string;
  current_operation: string;
  files_processed: number;
  files_total: number;
  bytes_processed: number;
  bytes_total: number;
  errors: ProcessingError[];
  warnings: string[];
}

export type ConsolidationStatus =
  | 'Pending'
  | 'Analyzing'
  | 'Processing'
  | 'WritingProject'
  | 'Completed'
  | 'Cancelled'
  | 'Failed';

export interface ProcessingError {
  file_path: string;
  error_message: string;
  is_fatal: boolean;
}

// Configuration Types

export interface ConsolidationOptions {
  output_path: string;
  sequences: string[];
  processing_mode: ProcessingMode;
  transcode_preset: TranscodePreset | null;
  optimization_mode: OptimizationMode;
  folder_structure: FolderStructure;
  proxy_mode: ProxyMode;
  handle_frames: number;
  include_all_multicam_angles: boolean;
  generate_unique_filenames: boolean;
  use_project_item_names: boolean;
  add_frame_range_to_filename: boolean;
  copy_sidecar_files: boolean;
  skip_offline_media: boolean;
}

export type ProcessingMode = 'trim' | 'transcode' | 'copy' | 'no_process';

export type TranscodePreset =
  | 'prores422'
  | 'prores422hq'
  | 'prores422lt'
  | 'prores4444'
  | 'dnxhd'
  | 'dnxhr'
  | 'h264high'
  | 'h264medium'
  | 'h265high'
  | 'h265medium';

export type OptimizationMode = 'minimize' | 'keep_files' | 'unique_clips';

export type FolderStructure = 'flat' | 'bins' | 'original';

export type ProxyMode = 'both' | 'proxy_only' | 'main_only' | 'preserve';

// UI State Types

export interface AppState {
  projectPath: string | null;
  projectInfo: ProjectInfo | null;
  sequences: SequenceInfo[];
  selectedSequences: string[];
  mediaItems: MediaItemInfo[];
  mediaUsage: MediaUsageResult | null;
  isLoading: boolean;
  error: string | null;
  consolidationProgress: ConsolidationProgress | null;
}

export type ViewTab = 'project' | 'sequences' | 'media' | 'settings' | 'consolidate';
