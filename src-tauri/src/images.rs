// images.rs — Image handling for cover art and screenshots.
//
// This module handles two cases:
//   1. Local file paths  → copy to app_data_dir/images/ with a unique name
//   2. Remote URLs       → download and save to app_data_dir/images/
//
// Both cases return a relative path that gets stored in the database.

use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use uuid::Uuid;
use tauri::Manager;

#[derive(Debug)]
pub enum ImageError {
    IoError(std::io::Error),
    HttpError(String),
    InvalidPath(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ImageError::IoError(e) => write!(f, "IO error: {}", e),
            ImageError::HttpError(e) => write!(f, "HTTP error: {}", e),
            ImageError::InvalidPath(e) => write!(f, "Invalid path: {}", e),
        }
    }
}

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        ImageError::IoError(e)
    }
}

/// Resolve the images directory: app_data_dir/images/
pub fn get_images_dir(app: &AppHandle) -> Result<PathBuf, ImageError> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| ImageError::InvalidPath(e.to_string()))?;
    
    let images_dir = app_data.join("images");
    
    // Create the directory if it doesn't exist
    if !images_dir.exists() {
        fs::create_dir_all(&images_dir)?;
    }
    
    Ok(images_dir)
}

/// Detect if a string is a remote URL or a local file path
fn is_remote_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// Extract the file extension from a path or URL
fn get_extension(input: &str) -> Option<String> {
    // For URLs, look for extension before query params
    let path_part = if input.contains('?') {
        input.split('?').next()?
    } else {
        input
    };
    
    Path::new(path_part)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
}

/// Generate a unique filename preserving the original extension
fn generate_filename(original: &str) -> String {
    let ext = get_extension(original).unwrap_or_else(|| "jpg".to_string());
    format!("{}.{}", Uuid::new_v4(), ext)
}

/// Copy a local file to the images directory
fn copy_local_file(source: &Path, dest: &Path) -> Result<(), ImageError> {
    if !source.exists() {
        return Err(ImageError::InvalidPath(format!(
            "Source file does not exist: {}",
            source.display()
        )));
    }
    
    fs::copy(source, dest)?;
    Ok(())
}

/// Download a remote image and save it to the images directory
fn download_remote_image(url: &str, dest: &Path) -> Result<(), ImageError> {
    // Use ureq for a simple blocking HTTP client (no async needed for this use case)
    let response = ureq::get(url)
        .call()
        .map_err(|e| ImageError::HttpError(format!("Failed to download: {}", e)))?;
    
    // Check that we got a successful response
    if response.status() != 200 {
        return Err(ImageError::HttpError(format!(
            "HTTP {} from {}",
            response.status(),
            url
        )));
    }
    
    // Read the response body into a byte buffer
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| ImageError::IoError(e))?;
    
    // Write to disk
    fs::write(dest, bytes)?;
    Ok(())
}

/// Main entry point: process an image (local path or URL) and return the saved path.
///
/// Returns an absolute path to the saved image in app_data_dir/images/.
/// The caller should store this path in the database.
pub fn process_image(app: &AppHandle, input: &str) -> Result<String, ImageError> {
    let images_dir = get_images_dir(app)?;
    let filename = generate_filename(input);
    let dest_path = images_dir.join(&filename);
    
    if is_remote_url(input) {
        // Download from URL
        download_remote_image(input, &dest_path)?;
    } else {
        // Copy from local filesystem
        let source_path = Path::new(input);
        copy_local_file(source_path, &dest_path)?;
    }
    
    // Return the absolute path as a string
    dest_path
        .to_str()
        .ok_or_else(|| ImageError::InvalidPath("Invalid UTF-8 in path".to_string()))
        .map(|s| s.to_string())
}