import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  FileVideo,
  Layers,
  Film,
  Settings,
  Play,
  FolderOpen,
  AlertCircle,
  CheckCircle,
  Info,
  X,
} from 'lucide-react';
import { ProjectDropZone } from './components/ProjectDropZone';
import { SequenceList } from './components/SequenceList';
import { MediaList } from './components/MediaList';
import { ConsolidationSettings } from './components/ConsolidationSettings';
import { ConsolidationProgress } from './components/ConsolidationProgress';
import {
  ProjectInfo,
  SequenceInfo,
  MediaItemInfo,
  MediaUsageResult,
  ConsolidationOptions,
  ConsolidationProgress as ProgressType,
  ViewTab,
} from './types';
import './index.css';

function App() {
  // Project state
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [projectInfo, setProjectInfo] = useState<ProjectInfo | null>(null);
  const [sequences, setSequences] = useState<SequenceInfo[]>([]);
  const [selectedSequences, setSelectedSequences] = useState<string[]>([]);
  const [mediaItems, setMediaItems] = useState<MediaItemInfo[]>([]);
  const [mediaUsage, setMediaUsage] = useState<MediaUsageResult | null>(null);

  // UI state
  const [activeTab, setActiveTab] = useState<ViewTab>('project');
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ffmpegStatus, setFfmpegStatus] = useState<string | null>(null);

  // Consolidation state
  const [consolidationOptions, setConsolidationOptions] = useState<ConsolidationOptions>({
    output_path: '',
    sequences: [],
    processing_mode: 'trim',
    transcode_preset: null,
    optimization_mode: 'keep_files',
    folder_structure: 'flat',
    proxy_mode: 'both',
    handle_frames: 0,
    include_all_multicam_angles: true,
    generate_unique_filenames: true,
    use_project_item_names: false,
    add_frame_range_to_filename: false,
    copy_sidecar_files: true,
    skip_offline_media: true,
  });
  const [consolidationProgress, setConsolidationProgress] = useState<ProgressType | null>(null);
  const [estimatedSize, setEstimatedSize] = useState<number | undefined>(undefined);

  // Check FFmpeg on mount
  useEffect(() => {
    const checkFfmpeg = async () => {
      try {
        const version = await invoke<string>('check_ffmpeg');
        setFfmpegStatus(version);
      } catch (err) {
        setFfmpegStatus(null);
        console.error('FFmpeg not found:', err);
      }
    };
    checkFfmpeg();
  }, []);

  // Load project
  const handleProjectSelect = useCallback(async (path: string) => {
    setIsLoading(true);
    setError(null);
    setProjectPath(path);

    try {
      // Get project info
      const info = await invoke<ProjectInfo>('get_project_info', { path });
      setProjectInfo(info);

      // Get sequences
      const seqs = await invoke<SequenceInfo[]>('get_sequences', { path });
      setSequences(seqs);

      // Select all sequences by default
      setSelectedSequences(seqs.map(s => s.object_id));

      // Get media items
      const media = await invoke<MediaItemInfo[]>('get_media_items', { path });
      setMediaItems(media);

      // Analyze media usage
      const usage = await invoke<MediaUsageResult>('analyze_media_usage', {
        path,
        sequenceIds: seqs.map(s => s.object_id),
        handleFrames: 0,
        includeAllMulticam: true,
      });
      setMediaUsage(usage);

      // Set default output path
      const defaultOutput = path.replace('.prproj', '_consolidated');
      setConsolidationOptions(prev => ({
        ...prev,
        output_path: defaultOutput,
      }));

      setActiveTab('sequences');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Update media usage when sequence selection changes
  useEffect(() => {
    if (!projectPath || selectedSequences.length === 0) return;

    const updateUsage = async () => {
      try {
        const usage = await invoke<MediaUsageResult>('analyze_media_usage', {
          path: projectPath,
          sequenceIds: selectedSequences,
          handleFrames: consolidationOptions.handle_frames,
          includeAllMulticam: consolidationOptions.include_all_multicam_angles,
        });
        setMediaUsage(usage);
      } catch (err) {
        console.error('Error updating media usage:', err);
      }
    };

    updateUsage();
  }, [projectPath, selectedSequences, consolidationOptions.handle_frames, consolidationOptions.include_all_multicam_angles]);

  // Estimate output size when options change
  useEffect(() => {
    if (!projectPath || !consolidationOptions.output_path) return;

    const estimate = async () => {
      try {
        const size = await invoke<number>('estimate_output_size', {
          projectPath,
          options: {
            ...consolidationOptions,
            sequences: selectedSequences,
          },
        });
        setEstimatedSize(size);
      } catch (err) {
        console.error('Error estimating size:', err);
      }
    };

    estimate();
  }, [projectPath, consolidationOptions, selectedSequences]);

  // Start consolidation
  const handleStartConsolidation = async () => {
    if (!projectPath) return;

    try {
      setConsolidationProgress({
        job_id: '',
        status: 'Pending',
        current_file: '',
        current_operation: 'Starting...',
        files_processed: 0,
        files_total: mediaUsage?.used_count || 0,
        bytes_processed: 0,
        bytes_total: mediaUsage?.used_size || 0,
        errors: [],
        warnings: [],
      });
      setActiveTab('consolidate');

      const jobId = await invoke<string>('start_consolidation', {
        projectPath,
        options: {
          ...consolidationOptions,
          sequences: selectedSequences,
        },
      });

      // Update progress with the actual job ID
      setConsolidationProgress(prev => prev ? {
        ...prev,
        job_id: jobId,
        status: 'Processing',
        current_operation: 'Starting consolidation...',
      } : null);

      // Poll for progress
      const pollProgress = async () => {
        try {
          const progress = await invoke<ProgressType>('get_consolidation_progress', { jobId });
          setConsolidationProgress(progress);

          // Continue polling if job is still running
          if (['Pending', 'Analyzing', 'Processing', 'WritingProject'].includes(progress.status)) {
            setTimeout(pollProgress, 500);
          }
        } catch (err) {
          console.error('Error polling progress:', err);
        }
      };

      // Start polling for progress
      pollProgress();

    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setConsolidationProgress(null);
    }
  };

  // Cancel consolidation
  const handleCancelConsolidation = async () => {
    if (consolidationProgress?.job_id) {
      try {
        await invoke('cancel_consolidation', { jobId: consolidationProgress.job_id });
        setConsolidationProgress(prev => prev ? {
          ...prev,
          status: 'Cancelled',
        } : null);
      } catch (err) {
        console.error('Error cancelling:', err);
      }
    }
  };

  // Reset project
  const handleCloseProject = () => {
    setProjectPath(null);
    setProjectInfo(null);
    setSequences([]);
    setSelectedSequences([]);
    setMediaItems([]);
    setMediaUsage(null);
    setConsolidationProgress(null);
    setEstimatedSize(undefined);
    setActiveTab('project');
  };

  const tabs: { id: ViewTab; label: string; icon: any }[] = [
    { id: 'sequences', label: 'Sequences', icon: Layers },
    { id: 'media', label: 'Media', icon: Film },
    { id: 'settings', label: 'Settings', icon: Settings },
  ];

  return (
    <div className="min-h-screen bg-[rgb(var(--background))]">
      {/* Header */}
      <header className="bg-[rgb(var(--surface))] border-b border-[rgb(var(--border))]">
        <div className="px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-[rgb(var(--accent-blue))] to-[rgb(var(--accent-purple))] flex items-center justify-center">
                <FileVideo className="w-5 h-5 text-white" />
              </div>
              <div>
                <h1 className="text-lg font-semibold text-[rgb(var(--text-primary))]">
                  PlumePack Clone
                </h1>
                <p className="text-xs text-[rgb(var(--text-muted))]">
                  Premiere Pro Project Consolidation
                </p>
              </div>
            </div>

            {/* FFmpeg status */}
            <div className="flex items-center gap-4">
              {ffmpegStatus ? (
                <span className="flex items-center gap-2 text-xs text-[rgb(var(--accent-green))]">
                  <CheckCircle className="w-3 h-3" />
                  FFmpeg Ready
                </span>
              ) : (
                <span className="flex items-center gap-2 text-xs text-[rgb(var(--accent-red))]">
                  <AlertCircle className="w-3 h-3" />
                  FFmpeg Not Found
                </span>
              )}

              {projectInfo && (
                <button
                  onClick={handleCloseProject}
                  className="btn btn-ghost text-sm"
                >
                  <X className="w-4 h-4" />
                  Close Project
                </button>
              )}
            </div>
          </div>

          {/* Project info bar */}
          {projectInfo && (
            <div className="flex items-center gap-6 mt-4 text-sm">
              <div className="flex items-center gap-2">
                <FolderOpen className="w-4 h-4 text-[rgb(var(--text-muted))]" />
                <span className="text-[rgb(var(--text-primary))] font-medium">{projectInfo.name}</span>
              </div>
              <span className="text-[rgb(var(--text-muted))]">
                {projectInfo.sequence_count} sequences
              </span>
              <span className="text-[rgb(var(--text-muted))]">
                {projectInfo.media_count} media items
              </span>
              <span className="badge badge-blue">v{projectInfo.version}</span>
            </div>
          )}

          {/* Tabs */}
          {projectInfo && (
            <div className="flex gap-1 mt-4 -mb-4">
              {tabs.map(tab => {
                const Icon = tab.icon;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActiveTab(tab.id)}
                    className={`px-4 py-2 text-sm font-medium rounded-t-lg transition-colors ${
                      activeTab === tab.id
                        ? 'bg-[rgb(var(--background))] text-[rgb(var(--text-primary))] border-t border-x border-[rgb(var(--border))]'
                        : 'text-[rgb(var(--text-muted))] hover:text-[rgb(var(--text-secondary))]'
                    }`}
                  >
                    <span className="flex items-center gap-2">
                      <Icon className="w-4 h-4" />
                      {tab.label}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </header>

      {/* Main content */}
      <main className="p-6">
        {/* Error banner */}
        {error && (
          <div className="mb-6 p-4 bg-[rgb(var(--accent-red))]/10 border border-[rgb(var(--accent-red))]/30 rounded-lg flex items-center justify-between">
            <div className="flex items-center gap-3">
              <AlertCircle className="w-5 h-5 text-[rgb(var(--accent-red))]" />
              <span className="text-[rgb(var(--text-primary))]">{error}</span>
            </div>
            <button
              onClick={() => setError(null)}
              className="text-[rgb(var(--text-muted))] hover:text-[rgb(var(--text-primary))]"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        )}

        {/* No project loaded */}
        {!projectInfo && (
          <div className="max-w-2xl mx-auto">
            <ProjectDropZone onProjectSelect={handleProjectSelect} isLoading={isLoading} />

            <div className="mt-8 grid grid-cols-3 gap-4">
              <div className="card p-4 text-center">
                <div className="w-10 h-10 mx-auto rounded-full bg-[rgb(var(--accent-blue))]/20 flex items-center justify-center mb-3">
                  <FileVideo className="w-5 h-5 text-[rgb(var(--accent-blue))]" />
                </div>
                <h3 className="font-medium text-[rgb(var(--text-primary))] mb-1">Trim Losslessly</h3>
                <p className="text-xs text-[rgb(var(--text-muted))]">
                  Remove unused frames while preserving original quality
                </p>
              </div>
              <div className="card p-4 text-center">
                <div className="w-10 h-10 mx-auto rounded-full bg-[rgb(var(--accent-green))]/20 flex items-center justify-center mb-3">
                  <Layers className="w-5 h-5 text-[rgb(var(--accent-green))]" />
                </div>
                <h3 className="font-medium text-[rgb(var(--text-primary))] mb-1">Nested Sequences</h3>
                <p className="text-xs text-[rgb(var(--text-muted))]">
                  Handles nested sequences to unlimited depth
                </p>
              </div>
              <div className="card p-4 text-center">
                <div className="w-10 h-10 mx-auto rounded-full bg-[rgb(var(--accent-purple))]/20 flex items-center justify-center mb-3">
                  <Film className="w-5 h-5 text-[rgb(var(--accent-purple))]" />
                </div>
                <h3 className="font-medium text-[rgb(var(--text-primary))] mb-1">Smart Proxy</h3>
                <p className="text-xs text-[rgb(var(--text-muted))]">
                  Manage proxy relationships during consolidation
                </p>
              </div>
            </div>
          </div>
        )}

        {/* Project loaded - show tabs */}
        {projectInfo && (
          <div className="max-w-5xl mx-auto">
            {/* Sequences tab */}
            {activeTab === 'sequences' && (
              <div className="space-y-6">
                <div className="flex items-center justify-between">
                  <div>
                    <h2 className="text-lg font-semibold text-[rgb(var(--text-primary))]">
                      Select Sequences
                    </h2>
                    <p className="text-sm text-[rgb(var(--text-muted))]">
                      Choose which sequences to include in consolidation
                    </p>
                  </div>
                  <button
                    onClick={() => setActiveTab('settings')}
                    disabled={selectedSequences.length === 0}
                    className="btn btn-primary"
                  >
                    Continue to Settings
                  </button>
                </div>

                <SequenceList
                  sequences={sequences}
                  selectedSequences={selectedSequences}
                  onSelectionChange={setSelectedSequences}
                />

                {/* Usage summary */}
                {mediaUsage && (
                  <div className="card p-4">
                    <div className="flex items-center gap-2 mb-3">
                      <Info className="w-4 h-4 text-[rgb(var(--accent-blue))]" />
                      <span className="font-medium text-[rgb(var(--text-primary))]">Usage Summary</span>
                    </div>
                    <div className="grid grid-cols-4 gap-4 text-sm">
                      <div>
                        <div className="text-[rgb(var(--text-muted))]">Used Media</div>
                        <div className="text-xl font-semibold text-[rgb(var(--accent-green))]">
                          {mediaUsage.used_count}
                        </div>
                      </div>
                      <div>
                        <div className="text-[rgb(var(--text-muted))]">Unused Media</div>
                        <div className="text-xl font-semibold text-[rgb(var(--accent-red))]">
                          {mediaUsage.unused_count}
                        </div>
                      </div>
                      <div>
                        <div className="text-[rgb(var(--text-muted))]">Used Size</div>
                        <div className="text-xl font-semibold text-[rgb(var(--text-primary))]">
                          {formatSize(mediaUsage.used_size)}
                        </div>
                      </div>
                      <div>
                        <div className="text-[rgb(var(--text-muted))]">Unused Size</div>
                        <div className="text-xl font-semibold text-[rgb(var(--text-primary))]">
                          {formatSize(mediaUsage.unused_size)}
                        </div>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* Media tab */}
            {activeTab === 'media' && (
              <div className="space-y-6">
                <div>
                  <h2 className="text-lg font-semibold text-[rgb(var(--text-primary))]">
                    Media Browser
                  </h2>
                  <p className="text-sm text-[rgb(var(--text-muted))]">
                    View all media in the project and their usage status
                  </p>
                </div>

                <MediaList
                  mediaItems={mediaItems}
                  mediaUsage={mediaUsage}
                />
              </div>
            )}

            {/* Settings tab */}
            {activeTab === 'settings' && (
              <div className="space-y-6">
                <div className="flex items-center justify-between">
                  <div>
                    <h2 className="text-lg font-semibold text-[rgb(var(--text-primary))]">
                      Consolidation Settings
                    </h2>
                    <p className="text-sm text-[rgb(var(--text-muted))]">
                      Configure how your project will be consolidated
                    </p>
                  </div>
                  <button
                    onClick={handleStartConsolidation}
                    disabled={!consolidationOptions.output_path || selectedSequences.length === 0}
                    className="btn btn-primary"
                  >
                    <Play className="w-4 h-4" />
                    Start Consolidation
                  </button>
                </div>

                <ConsolidationSettings
                  options={consolidationOptions}
                  onChange={setConsolidationOptions}
                  estimatedSize={estimatedSize}
                />
              </div>
            )}

            {/* Consolidate tab (progress) */}
            {activeTab === 'consolidate' && consolidationProgress && (
              <div className="space-y-6">
                <div>
                  <h2 className="text-lg font-semibold text-[rgb(var(--text-primary))]">
                    Consolidation Progress
                  </h2>
                  <p className="text-sm text-[rgb(var(--text-muted))]">
                    Processing your project...
                  </p>
                </div>

                <ConsolidationProgress
                  progress={consolidationProgress}
                  onCancel={handleCancelConsolidation}
                />
              </div>
            )}
          </div>
        )}
      </main>

      {/* Footer */}
      <footer className="fixed bottom-0 left-0 right-0 bg-[rgb(var(--surface))] border-t border-[rgb(var(--border))] px-6 py-3">
        <div className="flex items-center justify-between text-xs text-[rgb(var(--text-muted))]">
          <span>PlumePack Clone v0.1.0 - Neofox Media</span>
          <span>Supported formats: ProRes, H.264/H.265, DNxHD/HR, BRAW, R3D, Image Sequences</span>
        </div>
      </footer>
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

export default App;
