use std::{env, fs, path::PathBuf};

const TOOLS: &[(&str, &str, &str)] = &[
    ("wimlib-imagex", "WIFL_WIMLIB_IMAGEX", "wimlib_imagex"),
    ("sfdisk",        "WIFL_SFDISK",        "sfdisk"),
    ("mkntfs",        "WIFL_MKNTFS",        "mkntfs"),
    ("mkfs.fat",      "WIFL_MKFS_FAT",      "mkfs_fat"),
    ("partprobe",     "WIFL_PARTPROBE",     "partprobe"),
    ("efibootmgr",    "WIFL_EFIBOOTMGR",   "efibootmgr"),
    ("lsblk",         "WIFL_LSBLK",         "lsblk"),
    ("fuser",         "WIFL_FUSER",         "fuser"),
];

fn main() {
    let out       = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target    = env::var("TARGET").unwrap_or_default();
    let tag       = env::var("TAG").unwrap_or_else(|_| "dev".to_string());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TAG");
    println!("cargo:rustc-env=WIFL_BUILD_TAG={}", tag);
    println!("cargo:rustc-env=WIFL_TARGET={}", target);

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

        fs::write(&dest, []).expect("write placeholder");
    }
}
