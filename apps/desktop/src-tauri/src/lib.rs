//! Open Notebook app shell: SQLite storage, an HTTP client without browser CORS
//! limits, and API keys kept on the device.

mod files;
mod podcasts;
mod secrets;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_websocket::init());
    // Reopens the window where it was, at the size it was.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    builder
        .invoke_handler(tauri::generate_handler![
            secrets::secret_get,
            secrets::secret_set,
            secrets::secret_delete,
            secrets::secret_sync_available,
            podcasts::podcast_save,
            podcasts::podcast_remove,
            files::file_save,
            files::file_read,
            files::file_remove,
            files::download_save
        ])
        .run(tauri::generate_context!())
        .expect("error while running Open Notebook");
}
