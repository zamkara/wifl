use std::{env, fs, path::PathBuf};

const TOOLS: &[(&str, &str, &str)] = &[
    ("wimlib-imagex", "WIFL_WIMLIB_IMAGEX", "wimlib_imagex"),
    ("sfdisk",        "WIFL_SFDISK",        "sfdisk"),
    ("mkntfs",        "WIFL_MKNTFS",        "mkntfs"),
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
    println!("cargo:rerun-if-env-changed=WIFL_UEFI_NTFS");
    println!("cargo:rustc-env=WIFL_BUILD_TAG={}", tag);
    println!("cargo:rustc-env=WIFL_TARGET={}", target);

    // Bundle uefi-ntfs.img (1 MiB FAT12 NTFS boot bridge from Rufus project).
    // Source priority: WIFL_UEFI_NTFS env var, then res/uefi-ntfs.img in repo root.
    {
        let dest = out.join("uefi_ntfs_img");
        if target_os == "linux" {
            let src = env::var("WIFL_UEFI_NTFS")
                .ok()
                .or_else(|| {
                    // Resolve relative to CARGO_MANIFEST_DIR
                    let manifest = env::var("CARGO_MANIFEST_DIR").ok()?;
                    let candidate = PathBuf::from(manifest).join("res/uefi-ntfs.img");
                    if candidate.exists() { Some(candidate.display().to_string()) } else { None }
                });
            if let Some(src) = src {
                fs::copy(&src, &dest)
                    .unwrap_or_else(|e| panic!("bundling uefi-ntfs.img from {}: {}", src, e));
                println!("cargo:rerun-if-changed={}", src);
            } else {
                fs::write(&dest, []).expect("write placeholder");
            }
        } else {
            fs::write(&dest, []).expect("write placeholder");
        }
    }

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
