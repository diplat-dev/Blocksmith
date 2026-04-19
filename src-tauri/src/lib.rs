mod auth;
mod commands;
mod credential_store;
mod db;
mod dto;
mod error;
mod logging;
mod minecraft;
mod modrinth;
mod modpack;
mod paths;
mod profile_fs;
mod state;

use commands::{
    accounts::{bind_profile_account, create_local_account, delete_account, list_accounts, sign_in_microsoft},
    content::{
        apply_install_plan, apply_update_candidate, create_install_plan, import_mrpack,
        install_modrinth_modpack, list_installed_content, list_update_candidates,
        remove_installed_content, search_modrinth,
        toggle_installed_content,
    },
    dashboard::get_dashboard_snapshot,
    launch::{
        launch_profile, list_fabric_loader_versions, list_launch_history, list_minecraft_versions,
        resolve_launch_plan,
    },
    profiles::{create_profile, delete_profile, duplicate_profile, get_profile_detail, list_profiles},
    settings::{list_settings, upsert_setting},
    share::{export_profile_share, import_profile_share, import_profile_share_file},
    skins::{apply_skin_to_account, delete_skin, import_skin, list_skins},
};
use logging::init_logging;
use paths::AppPaths;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_paths = AppPaths::discover().expect("failed to initialize Blocksmith paths");
    init_logging(&app_paths).expect("failed to initialize Blocksmith logging");
    let app_state = AppState::bootstrap(app_paths).expect("failed to bootstrap Blocksmith state");

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_dashboard_snapshot,
            list_profiles,
            get_profile_detail,
            create_profile,
            duplicate_profile,
            delete_profile,
            list_minecraft_versions,
            list_fabric_loader_versions,
            resolve_launch_plan,
            launch_profile,
            list_launch_history,
            search_modrinth,
            create_install_plan,
            apply_install_plan,
            import_mrpack,
            install_modrinth_modpack,
            list_installed_content,
            toggle_installed_content,
            remove_installed_content,
            list_update_candidates,
            apply_update_candidate,
            list_accounts,
            create_local_account,
            sign_in_microsoft,
            delete_account,
            bind_profile_account,
            list_skins,
            import_skin,
            delete_skin,
            apply_skin_to_account,
            export_profile_share,
            import_profile_share,
            import_profile_share_file,
            list_settings,
            upsert_setting
        ])
        .run(tauri::generate_context!())
        .expect("error while running Blocksmith");
}
