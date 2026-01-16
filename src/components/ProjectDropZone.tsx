import { useState, useCallback } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, FileVideo, Upload } from 'lucide-react';

interface ProjectDropZoneProps {
  onProjectSelect: (path: string) => void;
  isLoading?: boolean;
}

export function ProjectDropZone({ onProjectSelect, isLoading }: ProjectDropZoneProps) {
  const [isDragging, setIsDragging] = useState(false);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);

    const files = Array.from(e.dataTransfer.files);
    const prprojFile = files.find(f => f.name.endsWith('.prproj'));

    if (prprojFile) {
      // Note: In Tauri, we need to use the file path
      // This is a simplified version - actual implementation may vary
      onProjectSelect((prprojFile as any).path || prprojFile.name);
    }
  }, [onProjectSelect]);

  const handleBrowse = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: 'Premiere Pro Project',
            extensions: ['prproj'],
          },
        ],
      });

      if (selected && typeof selected === 'string') {
        onProjectSelect(selected);
      }
    } catch (error) {
      console.error('Error opening file dialog:', error);
    }
  };

  return (
    <div
      className={`drop-zone p-12 text-center cursor-pointer transition-all duration-200 ${
        isDragging ? 'active' : ''
      } ${isLoading ? 'opacity-50 pointer-events-none' : ''}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onClick={handleBrowse}
    >
      <div className="flex flex-col items-center gap-4">
        <div className="w-16 h-16 rounded-full bg-[rgb(var(--accent-blue))]/10 flex items-center justify-center">
          {isLoading ? (
            <div className="w-8 h-8 border-2 border-[rgb(var(--accent-blue))] border-t-transparent rounded-full animate-spin" />
          ) : isDragging ? (
            <Upload className="w-8 h-8 text-[rgb(var(--accent-blue))]" />
          ) : (
            <FileVideo className="w-8 h-8 text-[rgb(var(--accent-blue))]" />
          )}
        </div>

        <div>
          <h3 className="text-lg font-medium text-[rgb(var(--text-primary))]">
            {isLoading ? 'Loading Project...' : 'Open Premiere Pro Project'}
          </h3>
          <p className="mt-1 text-sm text-[rgb(var(--text-secondary))]">
            {isLoading
              ? 'Parsing project file and scanning media...'
              : 'Drag and drop a .prproj file here, or click to browse'}
          </p>
        </div>

        {!isLoading && (
          <button className="btn btn-secondary mt-2">
            <FolderOpen className="w-4 h-4" />
            Browse Files
          </button>
        )}
      </div>
    </div>
  );
}
