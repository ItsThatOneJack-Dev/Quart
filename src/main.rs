mod compress;
mod extract;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        std::process::exit(1);
    }

    let command = &args[1];

    let result = match command.as_str() {
        "create" | "c" => handle_create(&args[2..]),
        "extract" | "x" => handle_extract(&args[2..]),
        "help" | "-h" | "--help" => {
            print_usage(&args[0]);
            Ok(())
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage(&args[0]);
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn handle_create(args: &[String]) -> std::io::Result<()> {
    if args.len() < 2 {
        eprintln!("Usage: quart create [options] <archive.q> <file1> [file2] ...");
        eprintln!("Options:");
        eprintln!("  -c <type>    Compression type: store, lz4, zstd (default), lzma");
        std::process::exit(1);
    }

    let mut compression = "zstd";
    let mut i = 0;

    // Parse options
    while i < args.len() && args[i].starts_with('-') {
        match args[i].as_str() {
            "-c" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -c requires a compression type");
                    std::process::exit(1);
                }
                compression = &args[i + 1];
                i += 2;
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    if i >= args.len() {
        eprintln!("Error: No archive name specified");
        std::process::exit(1);
    }

    let archive_name = &args[i];
    let files: Vec<String> = args[i + 1..].to_vec();

    if files.is_empty() {
        eprintln!("Error: No files specified");
        std::process::exit(1);
    }

    compress::create_archive(&files, archive_name, compression)
}

fn handle_extract(args: &[String]) -> std::io::Result<()> {
    if args.is_empty() {
        eprintln!("Usage: quart extract <archive.q> [output_dir]");
        std::process::exit(1);
    }

    let archive_name = &args[0];
    let output_dir = if args.len() > 1 {
        Some(args[1].as_str())
    } else {
        None
    };

    extract::extract_archive(archive_name, output_dir)
}

fn print_usage(program: &str) {
    println!("Quart Archive Tool v1.0");
    println!();
    println!("Usage: {} <command> [options] [arguments]", program);
    println!();
    println!("Commands:");
    println!("  create, c    Create a new archive");
    println!("  extract, x   Extract an archive");
    println!("  help         Show this help message");
    println!();
    println!("Create usage:");
    println!(
        "  {} create [options] <archive.q> <file1> [file2] ...",
        program
    );
    println!("  Options:");
    println!("    -c <type>  Compression: store, lz4, zstd (default), lzma");
    println!();
    println!("Extract usage:");
    println!("  {} extract <archive.q> [output_dir]", program);
    println!();
    println!("Examples:");
    println!("  {} create -c lzma archive.q file1.txt file2.txt", program);
    println!("  {} create archive.q *.txt", program);
    println!("  {} extract archive.q output/", program);
}
