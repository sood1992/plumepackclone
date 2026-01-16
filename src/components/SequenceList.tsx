import { Check, Film, Clock, Layers } from 'lucide-react';
import { SequenceInfo } from '../types';
import { formatDuration } from '../lib/utils';

interface SequenceListProps {
  sequences: SequenceInfo[];
  selectedSequences: string[];
  onSelectionChange: (ids: string[]) => void;
}

export function SequenceList({ sequences, selectedSequences, onSelectionChange }: SequenceListProps) {
  const toggleSequence = (id: string) => {
    if (selectedSequences.includes(id)) {
      onSelectionChange(selectedSequences.filter(s => s !== id));
    } else {
      onSelectionChange([...selectedSequences, id]);
    }
  };

  const toggleAll = () => {
    if (selectedSequences.length === sequences.length) {
      onSelectionChange([]);
    } else {
      onSelectionChange(sequences.map(s => s.object_id));
    }
  };

  if (sequences.length === 0) {
    return (
      <div className="card p-8 text-center">
        <Film className="w-12 h-12 mx-auto text-[rgb(var(--text-muted))] mb-4" />
        <p className="text-[rgb(var(--text-secondary))]">No sequences found in this project</p>
      </div>
    );
  }

  return (
    <div className="card overflow-hidden">
      {/* Header */}
      <div className="px-4 py-3 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-hover))]">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button
              onClick={toggleAll}
              className={`w-5 h-5 rounded border flex items-center justify-center transition-colors ${
                selectedSequences.length === sequences.length
                  ? 'bg-[rgb(var(--accent-blue))] border-[rgb(var(--accent-blue))]'
                  : selectedSequences.length > 0
                  ? 'bg-[rgb(var(--accent-blue))]/50 border-[rgb(var(--accent-blue))]'
                  : 'border-[rgb(var(--border))] hover:border-[rgb(var(--border-focus))]'
              }`}
            >
              {(selectedSequences.length === sequences.length || selectedSequences.length > 0) && (
                <Check className="w-3 h-3 text-white" />
              )}
            </button>
            <span className="text-sm font-medium text-[rgb(var(--text-primary))]">
              Sequences ({sequences.length})
            </span>
          </div>
          <span className="text-xs text-[rgb(var(--text-muted))]">
            {selectedSequences.length} selected
          </span>
        </div>
      </div>

      {/* Sequence list */}
      <div className="max-h-[400px] overflow-y-auto">
        {sequences.map((sequence) => {
          const isSelected = selectedSequences.includes(sequence.object_id);
          return (
            <div
              key={sequence.object_id}
              onClick={() => toggleSequence(sequence.object_id)}
              className={`flex items-center gap-4 px-4 py-3 cursor-pointer transition-colors border-b border-[rgb(var(--border))] last:border-b-0 ${
                isSelected ? 'bg-[rgb(var(--accent-blue))]/10' : 'hover:bg-[rgb(var(--surface-hover))]'
              }`}
            >
              {/* Checkbox */}
              <button
                className={`w-5 h-5 rounded border flex items-center justify-center transition-colors flex-shrink-0 ${
                  isSelected
                    ? 'bg-[rgb(var(--accent-blue))] border-[rgb(var(--accent-blue))]'
                    : 'border-[rgb(var(--border))]'
                }`}
              >
                {isSelected && <Check className="w-3 h-3 text-white" />}
              </button>

              {/* Icon */}
              <div className="w-8 h-8 rounded bg-[rgb(var(--accent-purple))]/20 flex items-center justify-center flex-shrink-0">
                <Film className="w-4 h-4 text-[rgb(var(--accent-purple))]" />
              </div>

              {/* Info */}
              <div className="flex-1 min-w-0">
                <div className="font-medium text-[rgb(var(--text-primary))] truncate">
                  {sequence.name}
                </div>
                <div className="flex items-center gap-4 text-xs text-[rgb(var(--text-muted))] mt-0.5">
                  <span className="flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {formatDuration(sequence.duration_seconds)}
                  </span>
                  <span>{sequence.frame_rate.toFixed(2)} fps</span>
                  <span className="flex items-center gap-1">
                    <Layers className="w-3 h-3" />
                    {sequence.video_track_count}V / {sequence.audio_track_count}A
                  </span>
                  {sequence.nested_count > 0 && (
                    <span className="badge badge-blue">{sequence.nested_count} nested</span>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
