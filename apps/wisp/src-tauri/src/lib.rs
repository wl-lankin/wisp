mod dto;
mod settings;

pub fn run() {
    tauri::Builder::default()
        .build(tauri::generate_context!())
        .expect("failed to build the Wisp app")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
