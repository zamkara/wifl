use std::{env, fs, path::PathBuf};

// (name, env-var-key, safe filename for include_bytes!)
const TOOLS: &[(&str, &str, &str)] = &[
    ("wimlib-imagex", "WIFL_WIMLIB_IMAGEX", "wimlib_imagex"),
    ("sgdisk",        "WIFL_SGDISK",        "sgdisk"),
    ("mkntfs",        "WIFL_MKNTFS",        "mkntfs"),
    ("mkfs.fat",      "WIFL_MKFS_FAT",      "mkfs_fat"),
    ("partprobe",     "WIFL_PARTPROBE",     "partprobe"),
    ("efibootmgr",    "WIFL_EFIBOOTMGR",   "efibootmgr"),
    ("lsblk",         "WIFL_LSBLK",         "lsblk"),
    ("fuser",         "WIFL_FUSER",         "fuser"),
];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:rerun-if-changed=build.rs");

    for (name, env_key, filename) in TOOLS {
        println!("cargo:rerun-if-env-changed={}", env_key);

        let dest = out.join(filename);

        if target_os == "linux" {
            if let Ok(src) = env::var(env_key) {
                fs::copy(&src, &dest)
                    .unwrap_or_else(|e| panic!("bundling {} from {}: {}", name, src, e));
                println!("cargo:rerun-if-changed={}", src);
                continue;
            }
        }

        // Placeholder — not bundled; falls back to system PATH at runtime.
        fs::write(&dest, []).expect("write placeholder");
    }
}
