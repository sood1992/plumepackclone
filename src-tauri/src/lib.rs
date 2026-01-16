mod project_parser;
mod media_scanner;
mod sequence_analyzer;
mod ffmpeg;
mod consolidation;
mod commands;

use commands::*;
use tracing_subscriber;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            // Project commands
            open_project,
            get_project_info,
            get_sequences,
            get_media_items,
            // Analysis commands
            analyze_media_usage,
            get_unused_media,
            // Consolidation commands
            start_consolidation,
            cancel_consolidation,
            get_consolidation_progress,
            // FFmpeg commands
            check_ffmpeg,
            get_media_metadata,
            // Utility commands
            estimate_output_size,
            validate_output_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
