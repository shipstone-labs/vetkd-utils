use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let vetkd_system_api_id = env::var("VETKD_SYSTEM_API_CANISTER_ID")
        .expect("Missing VETKD_SYSTEM_API_CANISTER_ID environment variable");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("canister_ids.rs");

    fs::write(
        dest_path,
        format!(
            r#"pub const VETKD_SYSTEM_API_CANISTER_ID: &str = "{}";"#,
            vetkd_system_api_id
        ),
    )
    .expect("Failed to write canister ID to file");
}
