import { AlertCircle, CheckCircle, XCircle, Loader2, AlertTriangle } from 'lucide-react';
import { ConsolidationProgress as ProgressType, ConsolidationStatus } from '../types';
import { calculatePercentage, truncatePath, formatFileSize } from '../lib/utils';

interface ConsolidationProgressProps {
  progress: ProgressType;
  onCancel?: () => void;
}

export function ConsolidationProgress({ progress, onCancel }: ConsolidationProgressProps) {
  const percentage = calculatePercentage(progress.bytes_processed, progress.bytes_total);
  const filesPercentage = calculatePercentage(progress.files_processed, progress.files_total);

  const getStatusColor = (status: ConsolidationStatus) => {
    switch (status) {
      case 'Completed':
        return 'green';
      case 'Failed':
      case 'Cancelled':
        return 'red';
      default:
        return 'blue';
    }
  };

  const getStatusIcon = (status: ConsolidationStatus) => {
    switch (status) {
      case 'Completed':
        return CheckCircle;
      case 'Failed':
        return XCircle;
      case 'Cancelled':
        return AlertCircle;
      default:
        return Loader2;
    }
  };

  const StatusIcon = getStatusIcon(progress.status);
  const statusColor = getStatusColor(progress.status);
  const isRunning = ['Pending', 'Analyzing', 'Processing', 'WritingProject'].includes(progress.status);

  return (
    <div className="card overflow-hidden">
      {/* Header */}
      <div className={`px-4 py-3 bg-[rgb(var(--accent-${statusColor}))]/10 border-b border-[rgb(var(--accent-${statusColor}))]/30`}>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <StatusIcon
              className={`w-5 h-5 text-[rgb(var(--accent-${statusColor}))] ${
                isRunning ? 'animate-spin' : ''
              }`}
            />
            <div>
              <h3 className="font-medium text-[rgb(var(--text-primary))]">
                {getStatusLabel(progress.status)}
              </h3>
              <p className="text-sm text-[rgb(var(--text-muted))]">
                {progress.current_operation}
              </p>
            </div>
          </div>

          {isRunning && onCancel && (
            <button
              onClick={onCancel}
              className="btn btn-secondary text-sm"
            >
              Cancel
            </button>
          )}
        </div>
      </div>

      {/* Progress content */}
      <div className="p-4 space-y-4">
        {/* Overall progress bar */}
        <div>
          <div className="flex items-center justify-between text-sm mb-2">
            <span className="text-[rgb(var(--text-secondary))]">Overall Progress</span>
            <span className="text-[rgb(var(--text-primary))] font-medium">{percentage}%</span>
          </div>
          <div className="h-2 bg-[rgb(var(--background))] rounded-full overflow-hidden">
            <div
              className={`h-full bg-[rgb(var(--accent-${statusColor}))] transition-all duration-300 ${
                isRunning && percentage < 100 ? 'progress-indeterminate' : ''
              }`}
              style={{ width: `${Math.max(percentage, isRunning ? 5 : 0)}%` }}
            />
          </div>
        </div>

        {/* Stats */}
        <div className="grid grid-cols-2 gap-4 text-sm">
          <div className="bg-[rgb(var(--background))] rounded-lg p-3">
            <div className="text-[rgb(var(--text-muted))] mb-1">Files</div>
            <div className="text-[rgb(var(--text-primary))] font-medium">
              {progress.files_processed} / {progress.files_total}
            </div>
            <div className="text-xs text-[rgb(var(--text-muted))]">
              {filesPercentage}% complete
            </div>
          </div>
          <div className="bg-[rgb(var(--background))] rounded-lg p-3">
            <div className="text-[rgb(var(--text-muted))] mb-1">Data</div>
            <div className="text-[rgb(var(--text-primary))] font-medium">
              {formatFileSize(progress.bytes_processed)}
            </div>
            <div className="text-xs text-[rgb(var(--text-muted))]">
              of {formatFileSize(progress.bytes_total)}
            </div>
          </div>
        </div>

        {/* Current file */}
        {progress.current_file && (
          <div className="bg-[rgb(var(--background))] rounded-lg p-3">
            <div className="text-xs text-[rgb(var(--text-muted))] mb-1">Current File</div>
            <div className="text-sm text-[rgb(var(--text-primary))] font-mono truncate" title={progress.current_file}>
              {truncatePath(progress.current_file, 60)}
            </div>
          </div>
        )}

        {/* Warnings */}
        {progress.warnings.length > 0 && (
          <div className="bg-[rgb(var(--accent-yellow))]/10 rounded-lg p-3">
            <div className="flex items-center gap-2 text-[rgb(var(--accent-yellow))] mb-2">
              <AlertTriangle className="w-4 h-4" />
              <span className="text-sm font-medium">Warnings ({progress.warnings.length})</span>
            </div>
            <div className="max-h-24 overflow-y-auto space-y-1">
              {progress.warnings.map((warning, i) => (
                <div key={i} className="text-xs text-[rgb(var(--text-secondary))]">
                  {warning}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Errors */}
        {progress.errors.length > 0 && (
          <div className="bg-[rgb(var(--accent-red))]/10 rounded-lg p-3">
            <div className="flex items-center gap-2 text-[rgb(var(--accent-red))] mb-2">
              <XCircle className="w-4 h-4" />
              <span className="text-sm font-medium">Errors ({progress.errors.length})</span>
            </div>
            <div className="max-h-32 overflow-y-auto space-y-2">
              {progress.errors.map((error, i) => (
                <div key={i} className="text-xs">
                  <div className="text-[rgb(var(--text-primary))] font-medium truncate">
                    {truncatePath(error.file_path, 40)}
                  </div>
                  <div className="text-[rgb(var(--text-muted))]">{error.error_message}</div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function getStatusLabel(status: ConsolidationStatus): string {
  switch (status) {
    case 'Pending':
      return 'Preparing...';
    case 'Analyzing':
      return 'Analyzing Project';
    case 'Processing':
      return 'Processing Media';
    case 'WritingProject':
      return 'Writing Project File';
    case 'Completed':
      return 'Consolidation Complete';
    case 'Cancelled':
      return 'Consolidation Cancelled';
    case 'Failed':
      return 'Consolidation Failed';
    default:
      return status;
  }
}
