# PlumePack Clone

A desktop application for Adobe Premiere Pro project consolidation, inspired by Autokroma's PlumePack V3. Built with Tauri (Rust backend) and React (TypeScript frontend).

## Features

### Core Functionality
- **Lossless Trimming**: Remove unused frames without re-encoding, preserving original quality, metadata, and codecs
- **Transcode**: Re-encode media to standardized formats (ProRes, DNxHD/HR, H.264/H.265)
- **Copy**: Duplicate files with all dependencies (proxies, sidecar files, etc.)
- **No Process**: Update project references without moving media

### Processing Modes
- **Trim (Lossless)**: Uses FFmpeg stream copy for supported codecs
- **Transcode**: Full re-encoding with preset options
- **Copy**: Complete file duplication with dependencies
- **No Process**: Reference-only updates

### Optimization Options
- **Keep Same Number of Files**: One output file per input
- **Minimize Disk Space**: Split files when used non-contiguously
- **Each Clip Unique**: Separate file per timeline clip (ideal for VFX roundtrips)

### Folder Structure Options
- **Flat**: All media in a single folder
- **Bin Structure**: Mirror project panel organization
- **Original Disk Structure**: Recreate source paths

### Additional Features
- Recursive nested sequence handling (unlimited depth)
- Proxy workflow management (copy both, proxy only, main only)
- Sidecar file collection (XMP, RED audio, BRAW sidecars)
- Handle frames support (extra frames before/after cuts)
- Multicam angle handling
- Duplicate detection via xxHash
- Cross-platform path normalization

## Supported Formats

### Lossless Trim Support
- ProRes (all profiles)
- DNxHD/DNxHR
- H.264/H.265
- Cineform
- MJPEG
- Image sequences (PNG, TIFF, DPX, EXR)

### RAW Format Support
- RED (R3D) with sidecar files
- Blackmagic RAW (BRAW) with sidecars

## Prerequisites

### All Platforms
- FFmpeg (required for media processing)
- Node.js 18+ and npm
- Rust (latest stable)

### Linux
```bash
# Debian/Ubuntu
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  ffmpeg

# Fedora
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel ffmpeg
sudo dnf group install "C Development Tools and Libraries"

# Arch
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module gtk3 libappindicator-gtk3 librsvg libvips ffmpeg
```

### macOS
```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install FFmpeg via Homebrew
brew install ffmpeg
```

### Windows
1. Install [Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
3. Install [FFmpeg](https://ffmpeg.org/download.html) and add to PATH

## Installation

```bash
# Clone the repository
git clone https://github.com/your-username/plumepack-clone.git
cd plumepack-clone

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Usage

1. **Open Project**: Drag and drop a .prproj file or use the file browser
2. **Select Sequences**: Choose which sequences to consolidate
3. **Configure Settings**:
   - Select processing mode (Trim, Transcode, Copy, No Process)
   - Choose optimization mode
   - Set folder structure preference
   - Configure proxy handling
   - Add handle frames if needed
4. **Start Consolidation**: Click "Start Consolidation" and monitor progress
5. **Output**: Find your consolidated project and media in the output folder

## Project Structure

```
plumepack-clone/
├── src/                      # React frontend
│   ├── components/           # UI components
│   │   ├── ProjectDropZone.tsx
│   │   ├── SequenceList.tsx
│   │   ├── MediaList.tsx
│   │   ├── ConsolidationSettings.tsx
│   │   └── ConsolidationProgress.tsx
│   ├── lib/                  # Utilities
│   ├── types.ts              # TypeScript types
│   ├── App.tsx               # Main application
│   └── index.css             # Tailwind styles
├── src-tauri/                # Rust backend
│   └── src/
│       ├── project_parser.rs # .prproj file parsing
│       ├── media_scanner.rs  # Media inventory
│       ├── sequence_analyzer.rs # Usage analysis
│       ├── ffmpeg.rs         # FFmpeg integration
│       ├── consolidation.rs  # Main processing engine
│       ├── commands.rs       # Tauri IPC commands
│       └── lib.rs            # Application entry
├── package.json
├── tailwind.config.js
└── README.md
```

## Technical Details

### .prproj File Format
Adobe Premiere Pro project files are GZIP-compressed XML. The parser:
1. Decompresses the GZIP content
2. Parses XML structure
3. Resolves ObjectID/ObjectRef relationships
4. Extracts sequences, clips, and media references

### Media Processing Pipeline
1. **Analysis Phase**: Scan sequences for used media and time ranges
2. **Optimization**: Merge overlapping time ranges, calculate handles
3. **Processing**: Execute FFmpeg operations (trim/transcode/copy)
4. **Rewriting**: Generate new project file with updated paths

### FFmpeg Integration
- Uses stream copy (`-c copy`) for lossless operations
- Supports GOP-aware trimming for long-GOP codecs
- Handles keyframe positioning for accurate cuts

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Development

### Running Tests
```bash
# Frontend type checking
npm run build

# Backend
cd src-tauri && cargo test
```

### Building for Production
```bash
npm run tauri build
```

Binaries will be in `src-tauri/target/release/bundle/`

## License

MIT License - See LICENSE file for details

## Acknowledgments

- Inspired by [Autokroma PlumePack V3](https://www.autokroma.com/PlumePack)
- Built with [Tauri](https://tauri.app/)
- UI components styled with [Tailwind CSS](https://tailwindcss.com/)
