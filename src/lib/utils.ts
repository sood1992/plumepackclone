import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatFileSize(bytes: number): string {
  const KB = 1024;
  const MB = KB * 1024;
  const GB = MB * 1024;
  const TB = GB * 1024;

  if (bytes >= TB) {
    return `${(bytes / TB).toFixed(2)} TB`;
  } else if (bytes >= GB) {
    return `${(bytes / GB).toFixed(2)} GB`;
  } else if (bytes >= MB) {
    return `${(bytes / MB).toFixed(2)} MB`;
  } else if (bytes >= KB) {
    return `${(bytes / KB).toFixed(2)} KB`;
  } else {
    return `${bytes} B`;
  }
}

export function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  const frames = Math.floor((seconds % 1) * 24);

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}:${frames.toString().padStart(2, '0')}`;
  }
  return `${minutes}:${secs.toString().padStart(2, '0')}:${frames.toString().padStart(2, '0')}`;
}

export function formatTimecode(seconds: number, frameRate: number = 24): string {
  const totalFrames = Math.floor(seconds * frameRate);
  const hours = Math.floor(totalFrames / (3600 * frameRate));
  const minutes = Math.floor((totalFrames % (3600 * frameRate)) / (60 * frameRate));
  const secs = Math.floor((totalFrames % (60 * frameRate)) / frameRate);
  const frames = totalFrames % frameRate;

  return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}:${frames.toString().padStart(2, '0')}`;
}

export function getMediaTypeColor(mediaType: string): string {
  const type = mediaType.toLowerCase();
  if (type.includes('video')) return 'blue';
  if (type.includes('audio')) return 'green';
  if (type.includes('image')) return 'yellow';
  if (type.includes('red') || type.includes('braw')) return 'red';
  if (type.includes('graphics')) return 'purple';
  return 'gray';
}

export function getMediaTypeIcon(mediaType: string): string {
  const type = mediaType.toLowerCase();
  if (type.includes('video')) return 'Film';
  if (type.includes('audio')) return 'Music';
  if (type.includes('image')) return 'Image';
  if (type.includes('red') || type.includes('braw')) return 'Video';
  if (type.includes('graphics')) return 'Sparkles';
  return 'File';
}

export function truncatePath(path: string, maxLength: number = 50): string {
  if (path.length <= maxLength) return path;

  const parts = path.split(/[/\\]/);
  if (parts.length <= 2) return path;

  const fileName = parts[parts.length - 1];
  const firstPart = parts[0];

  if (fileName.length + firstPart.length + 5 > maxLength) {
    return '...' + path.slice(-maxLength + 3);
  }

  return `${firstPart}/.../${fileName}`;
}

export function calculatePercentage(current: number, total: number): number {
  if (total === 0) return 0;
  return Math.round((current / total) * 100);
}

export function debounce<T extends (...args: unknown[]) => unknown>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: ReturnType<typeof setTimeout> | null = null;

  return (...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => func(...args), wait);
  };
}
