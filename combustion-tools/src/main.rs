use std::io::{Cursor, Result, Write, Error, ErrorKind};

use fatfs::ReadWriteSeek;
use include_dir::{Dir, include_dir};

static IMAGE_SIZE: usize = 1 << 20; // one MiB
static INPUT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/image_files");

/// Copy the initial statically-defined set of files into the FAT image.
fn populate_image<T: ReadWriteSeek>(root: &mut fatfs::Dir<T>, input_dir: &Dir) -> Result<()> {
    for file in input_dir.files() {
        let path = file.path().as_os_str().to_string_lossy();
        let mut fat_file = root.create_file(&path)?;
        fat_file.truncate()?;
        let contents = file.contents();
        let written = fat_file.write(contents)?;
        if written != contents.len() {
            return Err(Error::from(ErrorKind::Interrupted));
        }
    }
    for subdir in input_dir.dirs() {
        let path = subdir.path().as_os_str().to_string_lossy();
        root.create_dir(&path)?;
        populate_image(root, subdir)?;
    }
    Ok(())
}

/// Create a FAT image of `IMAGE_SIZE` size,
/// populated with files supplied at compile time.
fn init_image() -> Result<Vec<u8>> {
    let mut seekable_image = Cursor::new(vec![0; IMAGE_SIZE]);
    let options = fatfs::FormatVolumeOptions::new().volume_label(*b"combustion ");
    fatfs::format_volume(&mut seekable_image, options)?;

    {
        let options = fatfs::FsOptions::new();
        let fs = fatfs::FileSystem::new(&mut seekable_image, options)?;
        let mut root = fs.root_dir();

        populate_image(&mut root, &INPUT_DIR)?;
    }

    Ok(seekable_image.into_inner())
}

/// Scaffolding; will be replaced with WASM calls later.
fn main() -> Result<()> {
    let image = init_image()?;
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("test.img")?
        .write(&image)?;
    if written != image.len() {
        eprintln!("error: wrote only {written} bytes of {}", image.len());
    }
    Ok(())
}
