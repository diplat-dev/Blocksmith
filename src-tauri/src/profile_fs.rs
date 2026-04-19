use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::error::{AppError, AppResult};

pub fn scaffold_profile_layout(profile_root: &Path) -> AppResult<()> {
    let directories = [
        profile_root.join(".launcher"),
        profile_root.join("minecraft"),
        profile_root.join("minecraft").join("mods"),
        profile_root.join("minecraft").join("resourcepacks"),
        profile_root.join("minecraft").join("shaderpacks"),
        profile_root.join("minecraft").join("datapacks"),
        profile_root.join("minecraft").join("config"),
        profile_root.join("minecraft").join("saves"),
        profile_root.join("minecraft").join("logs"),
    ];

    for directory in directories {
        fs::create_dir_all(directory)?;
    }

    Ok(())
}

pub fn ensure_profile_target(base: &Path, target: &Path) -> AppResult<()> {
    if !target.starts_with(base) {
        return Err(AppError::Path(format!(
            "refusing to operate outside profiles root: {}",
            target.to_string_lossy()
        )));
    }

    Ok(())
}

pub fn copy_profile_tree(source: &Path, destination: &Path) -> AppResult<()> {
    if !source.exists() {
        return Err(AppError::NotFound(format!(
            "profile directory does not exist: {}",
            source.to_string_lossy()
        )));
    }

    copy_dir_all(source, destination)
}

fn copy_dir_all(source: &Path, destination: &Path) -> AppResult<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let target_path: PathBuf = destination.join(entry.file_name());

        if entry_type.is_dir() {
            copy_dir_all(&entry.path(), &target_path)?;
        } else {
            fs::copy(entry.path(), target_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use uuid::Uuid;

    use super::scaffold_profile_layout;

    #[test]
    fn scaffold_profile_layout_creates_expected_directories() {
        let temp_root = env::temp_dir().join(format!("blocksmith-test-{}", Uuid::new_v4()));
        scaffold_profile_layout(&temp_root).expect("should scaffold profile layout");

        for expected in [
            temp_root.join(".launcher"),
            temp_root.join("minecraft"),
            temp_root.join("minecraft").join("mods"),
            temp_root.join("minecraft").join("resourcepacks"),
            temp_root.join("minecraft").join("shaderpacks"),
            temp_root.join("minecraft").join("datapacks"),
            temp_root.join("minecraft").join("config"),
            temp_root.join("minecraft").join("saves"),
            temp_root.join("minecraft").join("logs"),
        ] {
            assert!(expected.exists(), "missing {}", expected.to_string_lossy());
        }

        fs::remove_dir_all(temp_root).expect("should clean up temp profile layout");
    }
}
