use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let network = env::var("DFX_NETWORK").unwrap_or_else(|_| "local".to_string());

    let vetkd_system_api_id = Command::new("dfx")
        .args(["canister", "id", "vetkd_system_api", "--network", &network])
        .output()
        .map(|output| String::from_utf8(output.stdout).unwrap().trim().to_string())
        .unwrap_or_else(|_| "mock-canister-id".to_string()); // Default if `dfx` fails

    // Get the OUT_DIR path where Cargo wants us to store build artifacts
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("canister_ids.rs");

    fs::write(
        &dest_path,
        format!(
            r#"pub const VETKD_SYSTEM_API_CANISTER_ID: &str = "{}";"#,
            vetkd_system_api_id
        ),
    )
    .expect("Failed to write canister ID to file");

    println!("cargo:rerun-if-env-changed=DFX_NETWORK");
    println!("cargo:rerun-if-changed=build.rs"); // Always rerun when `build.rs` changes
                                                 // **🚀 NEW: Force rebuild if canister ID changes**
    let cache_path = Path::new(&out_dir).join("canister_id_cache.txt");
    let previous_id = fs::read_to_string(&cache_path).unwrap_or_default();
    if previous_id != vetkd_system_api_id {
        fs::write(&cache_path, &vetkd_system_api_id).expect("Failed to cache canister ID");
        println!("cargo:rerun-if-changed={}", cache_path.display()); // Track changes
    }
}
