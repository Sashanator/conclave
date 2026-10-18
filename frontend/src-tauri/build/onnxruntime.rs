use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process,
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

const WINDOWS_X64_TARGET: &str = "x86_64-pc-windows-msvc";

// Two NuGet packages (zip archives): ONNX Runtime with the DirectML provider, and the DirectML.dll it loads.
struct Package {
    url: &'static str,
    size: u64,
    sha256: &'static str,
}

const PACKAGES: [Package; 2] = [
    Package {
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.22.0/microsoft.ml.onnxruntime.directml.1.22.0.nupkg",
        size: 17_898_472,
        sha256: "29f9872d786236b79aa83f94482f3a17c14297e4833768d6d0ed4883ee732e60",
    },
    Package {
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg",
        size: 202_292_617,
        sha256: "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9",
    },
];

struct Artifact {
    package: usize,
    archive_path: &'static str,
    output_name: &'static str,
    size: u64,
    sha256: &'static str,
}

const ARTIFACTS: [Artifact; 5] = [
    Artifact {
        package: 0,
        archive_path: "runtimes/win-x64/native/onnxruntime.dll",
        output_name: "onnxruntime.dll",
        size: 16_471_584,
        sha256: "95366724919f4e95ecc60010912ed538ad9804b6683fbd0aad389749102834b9",
    },
    Artifact {
        package: 0,
        archive_path: "runtimes/win-x64/native/onnxruntime_providers_shared.dll",
        output_name: "onnxruntime_providers_shared.dll",
        size: 22_048,
        sha256: "dea79756b1ef0deb317115aa5da45eca8946eafcf1be2dbd9fa3b309551faae5",
    },
    Artifact {
        package: 0,
        archive_path: "LICENSE",
        output_name: "ONNXRUNTIME_LICENSE.txt",
        size: 1_094,
        sha256: "c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674",
    },
    Artifact {
        package: 1,
        archive_path: "bin/x64-win/DirectML.dll",
        output_name: "DirectML.dll",
        size: 18_527_776,
        sha256: "9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1",
    },
    Artifact {
        package: 1,
        archive_path: "LICENSE.txt",
        output_name: "DIRECTML_LICENSE.txt",
        size: 10_439,
        sha256: "a05138e3a085ff60a44881eedfa58dccb03ecc1d7b1f6ae888418e8c2fec4b8d",
    },
];

pub fn ensure_onnxruntime_runtime() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=binaries/onnxruntime");

    let target = env::var("TARGET").expect("TARGET environment variable not set");
    if target != WINDOWS_X64_TARGET {
        panic!("ONNX Runtime is bundled only for {WINDOWS_X64_TARGET}; got {target}");
    }

    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR environment variable not set");
    let destination = Path::new(&manifest_dir)
        .join("binaries")
        .join("onnxruntime");

    match verify_staged_runtime(&destination) {
        Ok(()) => {
            println!(
                "cargo:warning=Using verified bundled ONNX Runtime from {}",
                destination.display()
            );
            return;
        }
        Err(verification_error) => match fs::symlink_metadata(&destination) {
            Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                panic!(
                    "Refusing to replace linked or reparse-point ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Ok(metadata) if metadata.is_dir() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_dir_all(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime directory at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(metadata) if metadata.is_file() => {
                println!(
                    "cargo:warning=Replacing invalid bundled ONNX Runtime: {verification_error}"
                );
                fs::remove_file(&destination).unwrap_or_else(|remove_error| {
                    panic!(
                        "Failed to remove invalid ONNX Runtime file at {}: {remove_error}",
                        destination.display()
                    )
                });
            }
            Ok(_) => {
                panic!(
                    "Refusing to replace special ONNX Runtime stage at {}: {verification_error}",
                    destination.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                panic!(
                    "Failed to inspect invalid ONNX Runtime stage at {}: {error}",
                    destination.display()
                );
            }
        },
    }

    fs::create_dir_all(
        destination
            .parent()
            .expect("ONNX Runtime destination has no parent"),
    )
    .expect("Failed to create ONNX Runtime binaries directory");

    let temporary_destination =
        destination.with_file_name(format!(".onnxruntime-{}-{}", process::id(), target));
    let _ = fs::remove_dir_all(&temporary_destination);

    if let Err(error) = stage_runtime(&temporary_destination) {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!("Failed to stage bundled ONNX Runtime: {error}");
    }

    fs::rename(&temporary_destination, &destination).unwrap_or_else(|error| {
        let _ = fs::remove_dir_all(&temporary_destination);
        panic!(
            "Failed to finalize bundled ONNX Runtime at {}: {error}",
            destination.display()
        );
    });

    verify_staged_runtime(&destination).unwrap_or_else(|error| {
        panic!("Bundled ONNX Runtime verification failed after staging: {error}")
    });
    println!(
        "cargo:warning=Bundled verified ONNX Runtime at {}",
        destination.display()
    );
}

fn stage_runtime(destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;

    for (index, package) in PACKAGES.iter().enumerate() {
        let archive_path =
            env::temp_dir().join(format!("meetily-onnxruntime-{}-{index}.nupkg", process::id()));
        let result = stage_package(package, index, &archive_path, destination);
        let _ = fs::remove_file(&archive_path);
        result?;
    }

    Ok(())
}

fn stage_package(
    package: &Package,
    index: usize,
    archive_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    download_archive(package, archive_path)?;
    verify_file(archive_path, package.url, package.size, package.sha256)?;

    let archive_file = File::open(archive_path)
        .map_err(|error| format!("failed to open {}: {error}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| format!("failed to read ONNX Runtime package: {error}"))?;

    for artifact in ARTIFACTS.iter().filter(|artifact| artifact.package == index) {
        let mut source = archive.by_name(artifact.archive_path).map_err(|error| {
            format!(
                "missing {} in ONNX Runtime package: {error}",
                artifact.archive_path
            )
        })?;
        let output = destination.join(artifact.output_name);
        let mut file = File::create(&output)
            .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
        std::io::copy(&mut source, &mut file)
            .map_err(|error| format!("failed to extract {}: {error}", artifact.archive_path))?;
        verify_file(
            &output,
            artifact.output_name,
            artifact.size,
            artifact.sha256,
        )?;
    }

    Ok(())
}

fn download_archive(package: &Package, destination: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| format!("failed to create download client: {error}"))?;
    let mut response = client
        .get(package.url)
        .send()
        .map_err(|error| format!("failed to download ONNX Runtime: {error}"))?
        .error_for_status()
        .map_err(|error| format!("ONNX Runtime download failed: {error}"))?;
    let mut file = File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    copy_exact(&mut response, &mut file, package.size)
}

fn copy_exact<R: Read, W: Write>(
    source: &mut R,
    destination: &mut W,
    expected_size: u64,
) -> Result<(), String> {
    let copied = std::io::copy(
        &mut source.by_ref().take(expected_size),
        destination,
    )
    .map_err(|error| format!("failed to copy ONNX Runtime download: {error}"))?;
    if copied != expected_size {
        return Err(format!(
            "downloaded ONNX Runtime archive has size {copied}, expected {expected_size}"
        ));
    }

    let mut probe = [0_u8; 1];
    if source
        .read(&mut probe)
        .map_err(|error| format!("failed to copy ONNX Runtime download: {error}"))?
        != 0
    {
        return Err(format!(
            "downloaded ONNX Runtime archive exceeds expected size {expected_size}"
        ));
    }

    Ok(())
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        return metadata.file_attributes() & 0x0000_0400 != 0;
    }

    #[cfg(not(windows))]
    false
}

fn verify_staged_runtime(destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| format!("failed to inspect runtime stage: {error}"))?;
    if is_link_or_reparse_point(&metadata) {
        return Err("runtime stage is a link or reparse point".to_string());
    }
    if !metadata.file_type().is_dir() {
        return Err("runtime stage is not a directory".to_string());
    }

    let entries = fs::read_dir(destination)
        .map_err(|error| format!("failed to enumerate runtime stage: {error}"))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to enumerate runtime stage entry: {error}"))?;
        let entry_metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!(
                "failed to inspect runtime stage entry {}: {error}",
                entry.path().display()
            )
        })?;
        if is_link_or_reparse_point(&entry_metadata) {
            return Err(format!(
                "runtime stage entry {} is a link or reparse point",
                entry.path().display()
            ));
        }
        if !entry_metadata.file_type().is_file() {
            return Err(format!(
                "runtime stage entry {} is not a regular file",
                entry.path().display()
            ));
        }

        let name = entry.file_name();
        if !ARTIFACTS
            .iter()
            .any(|artifact| name.as_os_str() == artifact.output_name)
        {
            return Err(format!(
                "runtime stage contains undeclared entry {}",
                entry.path().display()
            ));
        }
    }

    for artifact in ARTIFACTS {
        verify_file(
            &destination.join(artifact.output_name),
            artifact.output_name,
            artifact.size,
            artifact.sha256,
        )?;
    }
    Ok(())
}

fn verify_file(
    path: &Path,
    name: &str,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("failed to inspect {name}: {error}"))?;
    if metadata.len() != expected_size {
        return Err(format!(
            "{name} has size {}, expected {expected_size}",
            metadata.len()
        ));
    }

    let mut file = File::open(path).map_err(|error| format!("failed to open {name}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {name}: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "{name} has SHA-256 {actual_sha256}, expected {expected_sha256}"
        ));
    }

    Ok(())
}
