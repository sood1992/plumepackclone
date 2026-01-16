mod project_parser;
mod media_scanner;
mod sequence_analyzer;
mod ffmpeg;
mod consolidation;
mod commands;

use commands::AppState;
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
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            // Project commands
            commands::open_project,
            commands::get_project_info,
            commands::get_sequences,
            commands::get_media_items,
            // Analysis commands
            commands::analyze_media_usage,
            commands::get_unused_media,
            // Consolidation commands
            commands::start_consolidation,
            commands::cancel_consolidation,
            commands::get_consolidation_progress,
            // FFmpeg commands
            commands::check_ffmpeg,
            commands::get_media_metadata,
            // Utility commands
            commands::estimate_output_size,
            commands::validate_output_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
