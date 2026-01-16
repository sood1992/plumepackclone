import { useState } from 'react';
import { Film, Music, Image, Video, Sparkles, File, AlertCircle, Check, Search } from 'lucide-react';
import { MediaItemInfo, MediaUsageResult } from '../types';
import { truncatePath } from '../lib/utils';

interface MediaListProps {
  mediaItems: MediaItemInfo[];
  mediaUsage: MediaUsageResult | null;
  showUnusedOnly?: boolean;
}

export function MediaList({ mediaItems, mediaUsage, showUnusedOnly = false }: MediaListProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [filterType, setFilterType] = useState<string>('all');

  const getMediaIcon = (type: string) => {
    const typeLower = type.toLowerCase();
    if (typeLower.includes('video')) return Film;
    if (typeLower.includes('audio')) return Music;
    if (typeLower.includes('image')) return Image;
    if (typeLower.includes('red') || typeLower.includes('braw')) return Video;
    if (typeLower.includes('graphics')) return Sparkles;
    return File;
  };

  const getMediaColor = (type: string) => {
    const typeLower = type.toLowerCase();
    if (typeLower.includes('video')) return 'blue';
    if (typeLower.includes('audio')) return 'green';
    if (typeLower.includes('image')) return 'yellow';
    if (typeLower.includes('red') || typeLower.includes('braw')) return 'red';
    if (typeLower.includes('graphics')) return 'purple';
    return 'gray';
  };

  const filteredItems = mediaItems.filter(item => {
    // Filter by used/unused
    if (showUnusedOnly && mediaUsage) {
      if (!mediaUsage.unused_media.includes(item.object_id)) {
        return false;
      }
    }

    // Filter by search query
    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      if (
        !item.file_name.toLowerCase().includes(query) &&
        !item.file_path.toLowerCase().includes(query)
      ) {
        return false;
      }
    }

    // Filter by type
    if (filterType !== 'all') {
      if (!item.media_type.toLowerCase().includes(filterType)) {
        return false;
      }
    }

    return true;
  });

  const usedCount = mediaUsage?.used_count ?? 0;
  const unusedCount = mediaUsage?.unused_count ?? 0;

  return (
    <div className="card overflow-hidden">
      {/* Header with stats and filters */}
      <div className="px-4 py-3 border-b border-[rgb(var(--border))] space-y-3">
        {/* Stats */}
        {mediaUsage && (
          <div className="flex items-center gap-4 text-sm">
            <div className="flex items-center gap-2">
              <span className="badge badge-green">{usedCount} used</span>
              <span className="text-[rgb(var(--text-muted))]">
                {formatSize(mediaUsage.used_size)}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <span className="badge badge-red">{unusedCount} unused</span>
              <span className="text-[rgb(var(--text-muted))]">
                {formatSize(mediaUsage.unused_size)}
              </span>
            </div>
          </div>
        )}

        {/* Filters */}
        <div className="flex items-center gap-3">
          {/* Search */}
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[rgb(var(--text-muted))]" />
            <input
              type="text"
              placeholder="Search media..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="input pl-9"
            />
          </div>

          {/* Type filter */}
          <select
            value={filterType}
            onChange={(e) => setFilterType(e.target.value)}
            className="select w-auto"
          >
            <option value="all">All Types</option>
            <option value="video">Video</option>
            <option value="audio">Audio</option>
            <option value="image">Image</option>
            <option value="red">RED</option>
            <option value="braw">BRAW</option>
          </select>
        </div>
      </div>

      {/* Media list */}
      <div className="max-h-[500px] overflow-y-auto">
        {filteredItems.length === 0 ? (
          <div className="p-8 text-center text-[rgb(var(--text-muted))]">
            No media items found
          </div>
        ) : (
          <table className="w-full">
            <thead className="sticky top-0 bg-[rgb(var(--surface))]">
              <tr className="border-b border-[rgb(var(--border))]">
                <th className="table-header px-4 py-2">Name</th>
                <th className="table-header px-4 py-2">Type</th>
                <th className="table-header px-4 py-2">Size</th>
                <th className="table-header px-4 py-2">Status</th>
                <th className="table-header px-4 py-2">Usage</th>
              </tr>
            </thead>
            <tbody>
              {filteredItems.map((item) => {
                const Icon = getMediaIcon(item.media_type);
                const color = getMediaColor(item.media_type);
                const isUsed = mediaUsage?.used_media.some(u => u.object_id === item.object_id);
                const usageInfo = mediaUsage?.used_media.find(u => u.object_id === item.object_id);

                return (
                  <tr key={item.object_id} className="table-row">
                    <td className="table-cell px-4">
                      <div className="flex items-center gap-3">
                        <div className={`w-8 h-8 rounded bg-[rgb(var(--accent-${color}))]/20 flex items-center justify-center flex-shrink-0`}>
                          <Icon className={`w-4 h-4 text-[rgb(var(--accent-${color}))]`} />
                        </div>
                        <div className="min-w-0">
                          <div className="font-medium truncate" title={item.file_name}>
                            {item.file_name}
                          </div>
                          <div className="text-xs text-[rgb(var(--text-muted))] truncate" title={item.file_path}>
                            {truncatePath(item.file_path, 40)}
                          </div>
                        </div>
                      </div>
                    </td>
                    <td className="table-cell px-4">
                      <span className={`badge badge-${color}`}>
                        {item.media_type.replace('MediaType::', '')}
                      </span>
                    </td>
                    <td className="table-cell px-4 text-[rgb(var(--text-secondary))]">
                      {item.file_size_formatted}
                    </td>
                    <td className="table-cell px-4">
                      {item.is_online ? (
                        <span className="flex items-center gap-1 text-[rgb(var(--accent-green))]">
                          <Check className="w-4 h-4" />
                          Online
                        </span>
                      ) : (
                        <span className="flex items-center gap-1 text-[rgb(var(--accent-red))]">
                          <AlertCircle className="w-4 h-4" />
                          Offline
                        </span>
                      )}
                    </td>
                    <td className="table-cell px-4">
                      {isUsed ? (
                        <span className="badge badge-green">
                          {usageInfo?.usage_count}x used
                        </span>
                      ) : (
                        <span className="badge badge-red">Unused</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
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
