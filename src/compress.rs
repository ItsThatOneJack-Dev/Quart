use sha2::{Digest, Sha256};
use std::fs::{metadata, read_dir, File};
use std::io::{Error, ErrorKind, Read, Result, Seek, Write};
use std::path::{Path, PathBuf};

const ALGO_STORE: u8 = 0;
const ALGO_LZ4: u8 = 1;
const ALGO_ZSTD: u8 = 2;
const ALGO_LZMA: u8 = 3;

pub fn create_archive(paths: &[String], output: &str, compression: &str) -> Result<()> {
    let algo_id = parse_compression_type(compression)?;

    let mut archive = File::create(output)?;

    // Write in the archive header.
    archive.write_all(b"QUART")?;
    archive.write_all(&[0x01, 0x00])?; // Version 1.

    let header_size = 7u64;
    let mut current_offset = header_size;

    // Collect files.
    let mut all_files = Vec::new();

    for path_str in paths {
        let path = Path::new(path_str);

        if !path.exists() {
            eprintln!("Warning: {} does not exist, skipping!", path_str);
            continue;
        }

        if path.is_file() {
            // This is a single file, so we just add it's path and filename.
            all_files.push((
                path.to_path_buf(),
                path.file_name().unwrap().to_string_lossy().to_string(),
            ));
        } else if path.is_dir() {
            // This is a directoryt, so we traverse it too, and add all of its children.
            collect_files_recursive(path, path, &mut all_files)?;
        }
    }

    if all_files.is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "No files to archive."));
    }

    if all_files.len() > 1 {
        println!("Archiving {} files.", all_files.len());
    } else {
        println!("Archiving {} file.", all_files.len());
    }
    println!();

    // Compress and write the files.
    let mut directory_entries: Vec<DirectoryEntry> = Vec::new();

    for (i, (file_path, archive_path)) in all_files.iter().enumerate() {
        println!("[{}/{}] Adding: {}", i + 1, all_files.len(), archive_path);

        // Read the file data.
        let mut file = File::open(&file_path)?;
        let mut file_data = Vec::new();
        file.read_to_end(&mut file_data)?;

        let uncompressed_size = file_data.len() as u64;

        // Compress the file data.
        let compressed_data = compress_data(&file_data, algo_id)?;
        let compressed_size = compressed_data.len() as u64;

        // Calculate the SHA-256 hash of the uncompressed file data.
        let mut hasher = Sha256::new();
        hasher.update(&file_data);
        let hash = hasher.finalize();

        // Get the file's metadata.
        let metadata = metadata(&file_path)?;
        let modified_time = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Write the compressed data to the archive.
        archive.write_all(&compressed_data)?;

        directory_entries.push(DirectoryEntry {
            filename: archive_path.clone(),
            data_offset: current_offset,
            compressed_size,
            uncompressed_size,
            sha256_hash: hash.to_vec(),
            modified_time,
            permissions: get_permissions(&metadata),
            compression_algo: algo_id,
            compression_level: get_compression_level(algo_id),
            encryption_algo: 0,
            encryption_key_id: 0,
        });

        current_offset += compressed_size;

        // Calculate percentage change in the file size.
        let size_change = ((compressed_size as f64 - uncompressed_size as f64)
            / uncompressed_size as f64)
            * 100.0;

        if size_change > 0.0 {
            println!(
                "  {} bytes -> {} bytes (+{:.1}%)",
                uncompressed_size, compressed_size, size_change
            );
        } else if size_change < 0.0 {
            println!(
                "  {} bytes -> {} bytes (-{:.1}%)",
                uncompressed_size,
                compressed_size,
                size_change.abs()
            );
        } else {
            println!(
                "  {} bytes -> {} bytes (no change)",
                uncompressed_size, compressed_size
            );
        }
    }

    let central_dir_offset = current_offset;

    // Build and compress the record directory.
    let mut directory_data = Vec::new();

    for entry in &directory_entries {
        // Write the filename length and filename itself.
        let filename_bytes = entry.filename.as_bytes();
        write_varint(&mut directory_data, filename_bytes.len() as u64);
        directory_data.extend_from_slice(filename_bytes);

        // Write the entry fields.
        write_varint(&mut directory_data, entry.data_offset);
        write_varint(&mut directory_data, entry.compressed_size);
        write_varint(&mut directory_data, entry.uncompressed_size);
        directory_data.extend_from_slice(&entry.sha256_hash);
        directory_data.extend_from_slice(&entry.modified_time.to_le_bytes());
        directory_data.extend_from_slice(&entry.permissions.to_le_bytes());
        directory_data.push(entry.compression_algo);
        directory_data.push(entry.compression_level);
        directory_data.push(entry.encryption_algo);
        directory_data.extend_from_slice(&entry.encryption_key_id.to_le_bytes());

        // The Attributes field is 0 bytes long (it is not used yet).
        write_varint(&mut directory_data, 0);
    }

    let directory_uncomp_size = directory_data.len() as u32;

    // Compress the record directory with ZSTD.
    let compressed_directory = zstd::encode_all(&directory_data[..], 15)?;
    let directory_comp_size = compressed_directory.len() as u32;

    archive.write_all(&compressed_directory)?;

    // Write the end-of-archive record.
    archive.write_all(&central_dir_offset.to_le_bytes())?;
    archive.write_all(&directory_comp_size.to_le_bytes())?;
    archive.write_all(&directory_uncomp_size.to_le_bytes())?;
    archive.write_all(&(directory_entries.len() as u32).to_le_bytes())?;
    archive.write_all(&[ALGO_ZSTD])?; // The directory is always compressed with ZSTD.

    let total_size = archive.stream_position()?;

    println!();
    println!("Archive created: {}", output);
    println!("  Total size: {} bytes", total_size);

    Ok(())
}

// Recursively collect all of the files in a directory.
fn collect_files_recursive(
    base_path: &Path,
    current_path: &Path,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<()> {
    for entry in read_dir(current_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            // Calculate the relative path from the base.
            let relative = path
                .strip_prefix(base_path)
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "Path strip failed."))?;

            // Convert the path to forward slashes for consistency.
            let archive_path = relative.to_string_lossy().replace('\\', "/");

            files.push((path, archive_path));
        } else if path.is_dir() {
            // Recurse into the subdirectory.
            collect_files_recursive(base_path, &path, files)?;
        }
    }

    Ok(())
}

struct DirectoryEntry {
    filename: String,
    data_offset: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    sha256_hash: Vec<u8>,
    modified_time: u64,
    permissions: u32,
    compression_algo: u8,
    compression_level: u8,
    encryption_algo: u8,
    encryption_key_id: u16,
}

fn parse_compression_type(s: &str) -> Result<u8> {
    match s.to_lowercase().as_str() {
        "store" | "none" => Ok(ALGO_STORE),
        "lz4" => Ok(ALGO_LZ4),
        "zstd" | "zstandard" => Ok(ALGO_ZSTD),
        "lzma" | "lzma2" => Ok(ALGO_LZMA),
        _ => Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Unknown compression type: {}. Use: store, lz4, zstd, or lzma.",
                s
            ),
        )),
    }
}

fn compress_data(data: &[u8], algo_id: u8) -> Result<Vec<u8>> {
    match algo_id {
        ALGO_STORE => Ok(data.to_vec()),
        ALGO_LZ4 => Ok(lz4_flex::compress_prepend_size(data)),
        ALGO_ZSTD => {
            zstd::encode_all(data, 10) // Medium compression.
        }
        ALGO_LZMA => {
            let mut compressed = Vec::new();
            let mut encoder = xz2::write::XzEncoder::new(&mut compressed, 6);
            std::io::copy(&mut std::io::Cursor::new(data), &mut encoder)?;
            encoder.finish()?;
            Ok(compressed)
        }
        _ => Err(Error::new(
            ErrorKind::InvalidInput,
            format!("Unknown algorithm ID: {}", algo_id),
        )),
    }
}

fn get_compression_level(algo_id: u8) -> u8 {
    match algo_id {
        ALGO_STORE => 0,
        ALGO_LZ4 => 0,
        ALGO_ZSTD => 10,
        ALGO_LZMA => 6,
        _ => 0,
    }
}

fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(unix)]
fn get_permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn get_permissions(_metadata: &std::fs::Metadata) -> u32 {
    0o644 // Default permissions for non-Unix systems
}
