use super::*;

/// Defines the persisted stage identity used when the runtime tracks one Unity build step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityBuildStageIdentity {
    pub step_key: String,
    pub step_label: String,
    pub log_stem: String,
}

/// Resolves the final Unity artifact path that should exist under the artifact root after packaging.
pub fn resolve_final_unity_artifact_output_path(
    plan: &UnityBuildExecutionPlan,
    artifact_root_path: &Path,
) -> io::Result<PathBuf> {
    resolve_artifact_output_path(
        artifact_root_path,
        Some(artifact_output_relative_path(plan).as_str()),
    )
}

/// Resolves the stage identity the runtime persists for the Unity build execution step.
pub fn resolve_unity_build_stage_identity(
    plan: &UnityBuildExecutionPlan,
) -> UnityBuildStageIdentity {
    UnityBuildStageIdentity {
        step_key: String::from("unity-build"),
        step_label: String::from("Execute Unity Build"),
        log_stem: super::build_unity_log_stem(&plan.unity_target_platform),
    }
}

/// Packages one Unity output directory into the runtime-owned artifact archive.
pub fn package_unity_build_output(
    plan: &UnityBuildExecutionPlan,
    source_directory: &Path,
    artifact_root_path: &Path,
) -> io::Result<()> {
    if !source_directory.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "expected Unity archive source directory at {}",
                source_directory.display()
            ),
        ));
    }

    let artifact_path = resolve_final_unity_artifact_output_path(plan, artifact_root_path)?;
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if artifact_path.exists() {
        let metadata = fs::metadata(&artifact_path)?;
        if metadata.is_dir() {
            fs::remove_dir_all(&artifact_path)?;
        } else {
            fs::remove_file(&artifact_path)?;
        }
    }

    let file = fs::File::create(&artifact_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    super::add_unity_build_output_directory_to_zip(
        &mut zip,
        source_directory,
        source_directory,
        options,
        plan,
    )?;
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}
