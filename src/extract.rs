use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, File};
use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom, Write};
use std::path::Path;

const ALGO_STORE: u8 = 0;
const ALGO_LZ4: u8 = 1;
const ALGO_ZSTD: u8 = 2;
const ALGO_LZMA: u8 = 3;

pub fn extract_archive(archive_path: &str, output_dir: Option<&str>) -> Result<()> {
    // Determine the output directory.
    let output_dir_string;
    let output_dir = match output_dir {
        Some(dir) => dir,
        None => {
            // Use the archive name, without the extension.
            let path = Path::new(archive_path);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("extracted");
            output_dir_string = stem.to_string();
            &output_dir_string
        }
    };

    let mut archive = File::open(archive_path)?;

    // Read and validate the archive header.
    let mut magic = [0u8; 5];
    archive.read_exact(&mut magic)?;

    if &magic != b"QUART" {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Not a Quart archive (invalid magic).",
        ));
    }

    let mut version_bytes = [0u8; 2];
    archive.read_exact(&mut version_bytes)?;
    let version = u16::from_le_bytes(version_bytes);

    if version != 1 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("Unsupported version: {}", version),
        ));
    }

    // Read the end-of-archive record.
    let file_size = archive.seek(SeekFrom::End(0))?;

    if file_size < 28 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "File too small to be valid archive.",
        ));
    }

    archive.seek(SeekFrom::End(-21))?;

    let mut end_record = [0u8; 21];
    archive.read_exact(&mut end_record)?;

    let record_dir_offset = u64::from_le_bytes(end_record[0..8].try_into().unwrap());
    let record_dir_comp_size = u32::from_le_bytes(end_record[8..12].try_into().unwrap());
    let record_dir_uncomp_size = u32::from_le_bytes(end_record[12..16].try_into().unwrap());
    let entry_count = u32::from_le_bytes(end_record[16..20].try_into().unwrap());
    let directory_compression = end_record[20];

    // Validate the end record.
    if record_dir_offset >= file_size {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Invalid record directory offset.",
        ));
    }

    if record_dir_offset < 7 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "record directory overlaps header.",
        ));
    }

    if entry_count > 10_000_000 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Unreasonable entry count (possible corruption).",
        ));
    }

    if entry_count > 1 {
        println!("Decompressing {} file.", entry_count);
    } else {
        println!("Decompressing {} file.", entry_count);
    }
    println!();

    // Read and compress the record directory.
    archive.seek(SeekFrom::Start(record_dir_offset))?;

    let mut compressed_dir = vec![0u8; record_dir_comp_size as usize];
    archive.read_exact(&mut compressed_dir)?;

    let directory_data = decompress_data(&compressed_dir, directory_compression)?;

    if directory_data.len() != record_dir_uncomp_size as usize {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Directory decompression size mismatch.",
        ));
    }

    // Parse the directory entries.
    let mut cursor = 0;
    let mut entries = Vec::new();

    for i in 0..entry_count {
        let entry = parse_directory_entry(&directory_data, &mut cursor).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to parse entry {}: {}", i, e),
            )
        })?;
        entries.push(entry);
    }

    // Create the output directory.
    create_dir_all(output_dir)?;

    // Extract the files.
    let mut total_uncompressed_size = 0u64;
    for (i, entry) in entries.iter().enumerate() {
        println!("[{}/{}] Extracting: {}", i + 1, entry_count, entry.filename);

        // Validate the data offset.
        if entry.data_offset + entry.compressed_size > record_dir_offset {
            eprintln!("  Warning: File data overlaps directory, skipping...");
            continue;
        }

        // Read the compressed data.
        archive.seek(SeekFrom::Start(entry.data_offset))?;
        let mut compressed_data = vec![0u8; entry.compressed_size as usize];
        archive.read_exact(&mut compressed_data)?;

        // Decompress the file.
        let uncompressed_data = decompress_data(&compressed_data, entry.compression_algo)?;

        if uncompressed_data.len() != entry.uncompressed_size as usize {
            eprintln!("  Warning: Decompressed size mismatch.");
        }

        // Verify the SHA-256 hash.
        let mut hasher = Sha256::new();
        hasher.update(&uncompressed_data);
        let computed_hash = hasher.finalize();

        if computed_hash.as_slice() != entry.sha256_hash {
            eprintln!("  ERROR: SHA-256 checksum mismatch! File may be corrupted.");
            eprintln!("  Skipping...");
            continue;
        }

        // Build the output path and create the parent directories.
        let output_path = Path::new(output_dir).join(&entry.filename);

        // Create all parent directories.
        if let Some(parent) = output_path.parent() {
            create_dir_all(parent)?;
        }

        // Write the file.
        let mut output_file = File::create(&output_path)?;
        output_file.write_all(&uncompressed_data)?;

        // Set the file permissions (Unix only).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(entry.permissions);
            std::fs::set_permissions(&output_path, perms)?;
        }

        // Calculate the percentage change in total size.
        let size_change = ((entry.uncompressed_size as f64 - entry.compressed_size as f64)
            / entry.compressed_size as f64)
            * 100.0;

        if size_change > 0.0 {
            println!(
                "  {} bytes -> {} bytes (+{:.1}%)",
                entry.compressed_size, entry.uncompressed_size, size_change
            );
        } else if size_change < 0.0 {
            println!(
                "  {} bytes -> {} bytes (-{:.1}%)",
                entry.compressed_size, entry.uncompressed_size, size_change
            );
        } else {
            println!(
                "  {} bytes -> {} bytes (no change)",
                entry.compressed_size, entry.uncompressed_size
            );
        }
        total_uncompressed_size += entry.uncompressed_size;
    }

    let dir_name = Path::new(output_dir)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(output_dir);

    println!();
    println!("Directory created: {}", dir_name);
    println!("  Total size: {} bytes", total_uncompressed_size);

    Ok(())
}

#[allow(dead_code)]
#[derive(Debug)]
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

fn parse_directory_entry(data: &[u8], cursor: &mut usize) -> Result<DirectoryEntry> {
    let filename_len = read_varint(data, cursor)? as usize;
    if *cursor + filename_len > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Filename exceeds buffer.",
        ));
    }
    let filename = String::from_utf8_lossy(&data[*cursor..*cursor + filename_len]).to_string();
    *cursor += filename_len;

    let data_offset = read_varint(data, cursor)?;
    let compressed_size = read_varint(data, cursor)?;
    let uncompressed_size = read_varint(data, cursor)?;

    if *cursor + 32 > data.len() {
        return Err(Error::new(ErrorKind::InvalidData, "Hash exceeds buffer."));
    }
    let sha256_hash = data[*cursor..*cursor + 32].to_vec();
    *cursor += 32;

    if *cursor + 8 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Timestamp exceeds buffer.",
        ));
    }
    let modified_time = u64::from_le_bytes(data[*cursor..*cursor + 8].try_into().unwrap());
    *cursor += 8;

    if *cursor + 4 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Permissions exceed buffer.",
        ));
    }
    let permissions = u32::from_le_bytes(data[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;

    if *cursor + 1 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Compression algo exceeds buffer.",
        ));
    }
    let compression_algo = data[*cursor];
    *cursor += 1;

    if *cursor + 1 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Compression level exceeds buffer.",
        ));
    }
    let compression_level = data[*cursor];
    *cursor += 1;

    if *cursor + 1 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Encryption algo exceeds buffer.",
        ));
    }
    let encryption_algo = data[*cursor];
    *cursor += 1;

    if *cursor + 2 > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Encryption key ID exceeds buffer.",
        ));
    }
    let encryption_key_id = u16::from_le_bytes(data[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;

    let attributes_len = read_varint(data, cursor)? as usize;
    if *cursor + attributes_len > data.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Attributes exceed buffer.",
        ));
    }
    *cursor += attributes_len;

    Ok(DirectoryEntry {
        filename,
        data_offset,
        compressed_size,
        uncompressed_size,
        sha256_hash,
        modified_time,
        permissions,
        compression_algo,
        compression_level,
        encryption_algo,
        encryption_key_id,
    })
}

fn read_varint(data: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut result = 0u64;
    let mut shift = 0;

    loop {
        if *cursor >= data.len() {
            return Err(Error::new(ErrorKind::InvalidData, "Varint exceeds buffer."));
        }

        let byte = data[*cursor];
        *cursor += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            break;
        }

        shift += 7;
        if shift >= 64 {
            return Err(Error::new(ErrorKind::InvalidData, "Varint overflow."));
        }
    }

    Ok(result)
}

fn decompress_data(data: &[u8], algo_id: u8) -> Result<Vec<u8>> {
    match algo_id {
        ALGO_STORE => Ok(data.to_vec()),
        ALGO_LZ4 => lz4_flex::decompress_size_prepended(data).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("LZ4 decompression failed: {}", e),
            )
        }),
        ALGO_ZSTD => zstd::decode_all(data),
        ALGO_LZMA => {
            let mut decompressed = Vec::new();
            let mut decoder = xz2::read::XzDecoder::new(data);
            std::io::copy(&mut decoder, &mut decompressed)?;
            Ok(decompressed)
        }
        _ => Err(Error::new(
            ErrorKind::InvalidData,
            format!("Unknown algorithm ID: {}", algo_id),
        )),
    }
}
