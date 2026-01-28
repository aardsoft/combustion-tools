use std::io::{Cursor, Error, Write};

use fatfs::ReadWriteSeek;
use include_dir::{Dir, include_dir};
use js_sys::{Array, Map};
use serde_derive::Deserialize;
use wasm_bindgen::prelude::*;

static IMAGE_SIZE: usize = 1 << 20; // one MiB
static INPUT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/image_files");

fn fat_add_file<T: ReadWriteSeek>(
    root: &mut fatfs::Dir<T>,
    path: &str,
    contents: &[u8],
) -> Result<(), Error> {
    let mut fat_file = root.create_file(path)?;
    fat_file.truncate()?;
    fat_file.write_all(contents)?;
    fat_file.flush()
}

/// Copy the initial statically-defined set of files into the FAT image.
fn populate_image<T: ReadWriteSeek>(
    root: &mut fatfs::Dir<T>,
    input_dir: &Dir,
) -> Result<(), Error> {
    for file in input_dir.files() {
        let path = file.path().as_os_str().to_string_lossy();
        fat_add_file(root, &path, file.contents())?;
    }
    for subdir in input_dir.dirs() {
        let path = subdir.path().as_os_str().to_string_lossy();
        root.create_dir(&path)?;
        populate_image(root, subdir)?;
    }
    Ok(())
}

fn init_image_internal() -> Result<Vec<u8>, Error> {
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

#[wasm_bindgen]
/// Create a FAT image of `IMAGE_SIZE` size,
/// populated with files supplied at compile time.
pub fn init_image() -> Result<Vec<u8>, String> {
    init_image_internal().map_err(|e| format!("{e}"))
}

fn image_add_file_internal(image: &mut [u8], path: &str, contents: &[u8]) -> Result<(), Error> {
    let mut seekable_image = Cursor::new(image);

    let options = fatfs::FsOptions::new();
    let fs = fatfs::FileSystem::new(&mut seekable_image, options)?;
    let mut root = fs.root_dir();

    fat_add_file(&mut root, path, contents)
}

#[wasm_bindgen]
/// Create a file inside the given FAT image.
pub fn image_add_file(image: &mut [u8], path: &str, contents: Vec<u8>) -> Result<(), String> {
    image_add_file_internal(image, path, &contents).map_err(|e| format!("{e}"))
}

fn image_create_dir_internal(image: &mut [u8], path: &str) -> Result<(), Error> {
    let mut seekable_image = Cursor::new(image);

    let options = fatfs::FsOptions::new();
    let fs = fatfs::FileSystem::new(&mut seekable_image, options)?;
    fs.root_dir().create_dir(path)?;
    Ok(())
}

#[wasm_bindgen]
/// Create a file inside the given FAT image.
pub fn image_create_dir(image: &mut [u8], path: &str) -> Result<(), String> {
    image_create_dir_internal(image, path).map_err(|e| format!("{e}"))
}

#[wasm_bindgen]
/// Create a text file inside the given FAT image.
/// The contents must be valid UTF-8.
pub fn image_add_text_file(image: &mut [u8], path: &str, contents: &str) -> Result<(), String> {
    image_add_file_internal(image, path, contents.as_bytes()).map_err(|e| format!("{e}"))
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptOption {
    name: String,
    filename: String,
    preload: Option<bool>,
}

#[wasm_bindgen]
pub fn parse_options(script: &str) -> Result<Array, String> {
    let result = Array::new();
    let parsed: Result<Vec<ScriptOption>, _> = serde_norway::from_str(script);
    let vec = parsed.map_err(|e| format!("{e}"))?;
    for option in vec {
        let elem = Map::new();
        elem.set(&JsValue::from_str("name"), &JsValue::from_str(&option.name));
        elem.set(&JsValue::from_str("filename"), &JsValue::from_str(&option.filename));
        elem.set(&JsValue::from_str("preload"), &JsValue::from_bool(option.preload.is_some_and(|b| b)));
        result.push(&elem);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Read;

    #[test]
    fn test_add_file() {
        let mut image = init_image_internal().unwrap();
        let contents = b"TEST\0IMAGE";
        let path = "file";
        image_add_file_internal(&mut image, path, contents).unwrap();

        let mut seekable_image = Cursor::new(&mut image);
        let options = fatfs::FsOptions::new();
        let fs = fatfs::FileSystem::new(&mut seekable_image, options).unwrap();
        let root = fs.root_dir();
        let mut f = root.open_file(path).unwrap();
        let mut bytes = Vec::new();
        let n = f.read_to_end(&mut bytes).unwrap();
        assert_eq!(n, contents.len());
        assert_eq!(&bytes, contents);
    }
}
