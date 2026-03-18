use std::io::{Cursor, Error, Read, Write};

use fatfs::ReadWriteSeek;
use include_dir::{Dir, include_dir};
use js_sys::{Array, Map};
use serde_derive::Deserialize;
use wasm_bindgen::prelude::*;

const IMAGE_SIZE: usize = 1 << 20; // one MiB
const VOLUME_LABEL: [u8; 11] = *b"combustion ";
const INPUT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/image_files");

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

/// Create a new, empty image of the given size.
/// Use `into_inner()` on the returned cursor to get the bytes.
fn new_image(size: usize) -> Result<Cursor<Vec<u8>>, Error> {
    let mut seekable_image = Cursor::new(vec![0; size]);
    let options = fatfs::FormatVolumeOptions::new().volume_label(VOLUME_LABEL);
    fatfs::format_volume(&mut seekable_image, options)?;
    Ok(seekable_image)
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

/// Recursively copy all files and directories from src to dst
fn copy_all<T: ReadWriteSeek, U: ReadWriteSeek>(
    src: fatfs::Dir<T>,
    dst: fatfs::Dir<U>,
) -> Result<(), Error> {
    let mut contents = Vec::new();
    for entry in src.iter() {
        let entry = entry?;
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        } else if entry.is_dir() {
            let dst_dir = dst.create_dir(&name)?;
            copy_all(entry.to_dir(), dst_dir)?;
        } else {
            let mut dst_file = dst.create_file(&name)?;
            entry.to_file().read_to_end(&mut contents)?;
            dst_file.write_all(&contents)?;
            dst_file.flush()?;
            contents.clear();
        }
    }
    Ok(())
}

fn init_image_internal() -> Result<Vec<u8>, Error> {
    let mut seekable_image = new_image(IMAGE_SIZE)?;
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
pub fn image_add_file(image: &[u8], path: &str, contents: Vec<u8>) -> Result<Vec<u8>, String> {
    resize_loop(image, |image| {
        image_add_file_internal(image, path, &contents)
    })
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
pub fn image_create_dir(image: &[u8], path: &str) -> Result<Vec<u8>, String> {
    resize_loop(image, |image| image_create_dir_internal(image, path))
}

#[wasm_bindgen]
/// Create a text file inside the given FAT image.
/// The contents must be valid UTF-8.
pub fn image_add_text_file(image: &[u8], path: &str, contents: &str) -> Result<Vec<u8>, String> {
    resize_loop(image, |image| {
        image_add_file_internal(image, path, contents.as_bytes())
    })
}

fn resize_loop<F>(image: &[u8], mut f: F) -> Result<Vec<u8>, String>
where
    F: FnMut(&mut [u8]) -> Result<(), Error>,
{
    let mut image: Vec<u8> = image.into();
    loop {
        match f(&mut image) {
            Ok(_) => {
                return Ok(image);
            }
            // TODO: this has to be adapted if we upgrade fatfs to latest master
            Err(e) if e.to_string() == "No space left on device" => {
                let mut new_seekable_image =
                    new_image(image.len() * 2).map_err(|e| format!("{e}"))?;
                let mut old_seekable_image = Cursor::new(image);
                let options = fatfs::FsOptions::new();
                {
                    let new_fs = fatfs::FileSystem::new(&mut new_seekable_image, options)
                        .map_err(|e| format!("{e}"))?;
                    let old_fs = fatfs::FileSystem::new(&mut old_seekable_image, options)
                        .map_err(|e| format!("{e}"))?;
                    copy_all(old_fs.root_dir(), new_fs.root_dir()).map_err(|e| format!("{e}"))?;
                }
                image = new_seekable_image.into_inner();
            }
            Err(e) => {
                return Err(format!("{e}"));
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptOption {
    name: String,
    filename: String,
    tag: Option<String>,
    preload: Option<bool>,
    variables: Option<Vec<VariableDefinition>>,
}

#[derive(Debug, Clone, Deserialize)]
struct VariableDefinition {
    name: String,
    label: String,
    help: String,
    optional: Option<bool>,
}

#[wasm_bindgen]
pub fn parse_options(script: &str) -> Result<Array, String> {
    let result = Array::new();
    let parsed: Result<Vec<ScriptOption>, _> = serde_norway::from_str(script);
    let vec = parsed.map_err(|e| format!("{e}"))?;
    for option in vec {
        let elem = Map::new();
        elem.set(&JsValue::from_str("name"), &JsValue::from_str(&option.name));
        elem.set(
            &JsValue::from_str("filename"),
            &JsValue::from_str(&option.filename),
        );
        if let Some(tag) = &option.tag {
            elem.set(&JsValue::from_str("tag"), &JsValue::from_str(tag));
        }
        elem.set(
            &JsValue::from_str("preload"),
            &JsValue::from_bool(option.preload.is_some_and(|b| b)),
        );
        let varsjs = Array::new();
        if let Some(vars) = option.variables {
            for var in vars {
                let varelem = Map::new();
                varelem.set(&JsValue::from_str("name"), &JsValue::from_str(&var.name));
                varelem.set(&JsValue::from_str("label"), &JsValue::from_str(&var.label));
                varelem.set(&JsValue::from_str("help"), &JsValue::from_str(&var.help));
                varelem.set(
                    &JsValue::from_str("optional"),
                    &JsValue::from_bool(var.optional.is_some_and(|b| b)),
                );
                varsjs.push(&varelem);
            }
        }
        elem.set(&JsValue::from_str("variables"), &varsjs);
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
