import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, Scissors, RefreshCw, Copy, FileX } from 'lucide-react';
import { ConsolidationOptions, ProcessingMode, OptimizationMode, FolderStructure, ProxyMode, TranscodePreset } from '../types';

interface ConsolidationSettingsProps {
  options: ConsolidationOptions;
  onChange: (options: ConsolidationOptions) => void;
  estimatedSize?: number;
}

export function ConsolidationSettings({ options, onChange, estimatedSize }: ConsolidationSettingsProps) {
  const handleChange = <K extends keyof ConsolidationOptions>(
    key: K,
    value: ConsolidationOptions[K]
  ) => {
    onChange({ ...options, [key]: value });
  };

  const handleBrowseOutput = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });

      if (selected && typeof selected === 'string') {
        handleChange('output_path', selected);
      }
    } catch (error) {
      console.error('Error selecting directory:', error);
    }
  };

  const processingModes: { value: ProcessingMode; label: string; icon: any; description: string }[] = [
    {
      value: 'trim',
      label: 'Trim (Lossless)',
      icon: Scissors,
      description: 'Remove unused frames without re-encoding. Preserves original quality.',
    },
    {
      value: 'transcode',
      label: 'Transcode',
      icon: RefreshCw,
      description: 'Re-encode to a different format. Useful for standardizing codecs.',
    },
    {
      value: 'copy',
      label: 'Copy',
      icon: Copy,
      description: 'Copy files without modification. Includes all dependent files.',
    },
    {
      value: 'no_process',
      label: 'No Process',
      icon: FileX,
      description: 'Update project references only. No media files are copied.',
    },
  ];

  return (
    <div className="space-y-6">
      {/* Output Path */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Output Location</h3>
        <div className="flex gap-2">
          <input
            type="text"
            value={options.output_path}
            onChange={(e) => handleChange('output_path', e.target.value)}
            placeholder="Select output folder..."
            className="input flex-1"
          />
          <button onClick={handleBrowseOutput} className="btn btn-secondary">
            <FolderOpen className="w-4 h-4" />
          </button>
        </div>
        {estimatedSize !== undefined && (
          <p className="mt-2 text-xs text-[rgb(var(--text-muted))]">
            Estimated output size: {formatSize(estimatedSize)}
          </p>
        )}
      </div>

      {/* Processing Mode */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Processing Mode</h3>
        <div className="grid grid-cols-2 gap-3">
          {processingModes.map((mode) => {
            const Icon = mode.icon;
            const isSelected = options.processing_mode === mode.value;
            return (
              <button
                key={mode.value}
                onClick={() => handleChange('processing_mode', mode.value)}
                className={`p-3 rounded-lg border text-left transition-all ${
                  isSelected
                    ? 'border-[rgb(var(--accent-blue))] bg-[rgb(var(--accent-blue))]/10'
                    : 'border-[rgb(var(--border))] hover:border-[rgb(var(--border-focus))]'
                }`}
              >
                <div className="flex items-center gap-2 mb-1">
                  <Icon className={`w-4 h-4 ${isSelected ? 'text-[rgb(var(--accent-blue))]' : 'text-[rgb(var(--text-muted))]'}`} />
                  <span className="font-medium text-sm">{mode.label}</span>
                </div>
                <p className="text-xs text-[rgb(var(--text-muted))]">{mode.description}</p>
              </button>
            );
          })}
        </div>

        {/* Transcode Preset */}
        {options.processing_mode === 'transcode' && (
          <div className="mt-4">
            <label className="label block mb-2">Transcode Preset</label>
            <select
              value={options.transcode_preset || 'prores422'}
              onChange={(e) => handleChange('transcode_preset', e.target.value as TranscodePreset)}
              className="select"
            >
              <optgroup label="ProRes">
                <option value="prores422lt">ProRes 422 LT</option>
                <option value="prores422">ProRes 422</option>
                <option value="prores422hq">ProRes 422 HQ</option>
                <option value="prores4444">ProRes 4444</option>
              </optgroup>
              <optgroup label="DNx">
                <option value="dnxhd">DNxHD</option>
                <option value="dnxhr">DNxHR</option>
              </optgroup>
              <optgroup label="H.264">
                <option value="h264medium">H.264 Medium</option>
                <option value="h264high">H.264 High</option>
              </optgroup>
              <optgroup label="H.265">
                <option value="h265medium">H.265 Medium</option>
                <option value="h265high">H.265 High</option>
              </optgroup>
            </select>
          </div>
        )}
      </div>

      {/* Optimization Mode */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Optimization Mode</h3>
        <div className="space-y-2">
          {[
            {
              value: 'keep_files' as OptimizationMode,
              label: 'Keep Same Number of Files',
              description: 'One output file per input. Best for general use.',
            },
            {
              value: 'minimize' as OptimizationMode,
              label: 'Minimize Disk Space',
              description: 'Split files when used non-contiguously. Smallest output.',
            },
            {
              value: 'unique_clips' as OptimizationMode,
              label: 'Each Clip Unique',
              description: 'Separate file per timeline clip. Ideal for VFX roundtrips.',
            },
          ].map((mode) => (
            <label
              key={mode.value}
              className={`flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-all ${
                options.optimization_mode === mode.value
                  ? 'border-[rgb(var(--accent-blue))] bg-[rgb(var(--accent-blue))]/10'
                  : 'border-[rgb(var(--border))] hover:border-[rgb(var(--border-focus))]'
              }`}
            >
              <input
                type="radio"
                name="optimization_mode"
                value={mode.value}
                checked={options.optimization_mode === mode.value}
                onChange={() => handleChange('optimization_mode', mode.value)}
                className="mt-1"
              />
              <div>
                <div className="font-medium text-sm">{mode.label}</div>
                <div className="text-xs text-[rgb(var(--text-muted))]">{mode.description}</div>
              </div>
            </label>
          ))}
        </div>
      </div>

      {/* Folder Structure */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Folder Structure</h3>
        <select
          value={options.folder_structure}
          onChange={(e) => handleChange('folder_structure', e.target.value as FolderStructure)}
          className="select"
        >
          <option value="flat">Flat (All media in single folder)</option>
          <option value="bins">Bin Structure (Mirror project panel)</option>
          <option value="original">Original Disk Structure</option>
        </select>
      </div>

      {/* Proxy Settings */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Proxy Handling</h3>
        <select
          value={options.proxy_mode}
          onChange={(e) => handleChange('proxy_mode', e.target.value as ProxyMode)}
          className="select"
        >
          <option value="both">Copy both main and proxy</option>
          <option value="proxy_only">Copy proxy only (fallback to main)</option>
          <option value="main_only">Copy main only</option>
          <option value="preserve">Preserve references only</option>
        </select>
      </div>

      {/* Additional Options */}
      <div className="card p-4">
        <h3 className="text-sm font-medium text-[rgb(var(--text-primary))] mb-3">Additional Options</h3>
        <div className="space-y-3">
          {/* Handle frames */}
          <div className="flex items-center justify-between">
            <div>
              <label className="label">Handle Frames</label>
              <p className="text-xs text-[rgb(var(--text-muted))]">Extra frames before/after cuts</p>
            </div>
            <input
              type="number"
              min="0"
              max="120"
              value={options.handle_frames}
              onChange={(e) => handleChange('handle_frames', parseInt(e.target.value) || 0)}
              className="input w-20 text-center"
            />
          </div>

          {/* Checkboxes */}
          {[
            { key: 'include_all_multicam_angles', label: 'Include all multicam angles' },
            { key: 'generate_unique_filenames', label: 'Generate unique filenames' },
            { key: 'use_project_item_names', label: 'Use project item names' },
            { key: 'add_frame_range_to_filename', label: 'Add frame range to filename' },
            { key: 'copy_sidecar_files', label: 'Copy sidecar files (XMP, etc.)' },
            { key: 'skip_offline_media', label: 'Skip offline media' },
          ].map(({ key, label }) => (
            <label key={key} className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={options[key as keyof ConsolidationOptions] as boolean}
                onChange={(e) => handleChange(key as keyof ConsolidationOptions, e.target.checked as any)}
                className="checkbox"
              />
              <span className="text-sm text-[rgb(var(--text-secondary))]">{label}</span>
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  const GB = 1024 * 1024 * 1024;
  const MB = 1024 * 1024;

  if (bytes >= GB) {
    return `${(bytes / GB).toFixed(2)} GB`;
  }
  return `${(bytes / MB).toFixed(2)} MB`;
}
