use std::env;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use libcodrop::format::{BlockHeader, BlockType, StreamHeader};
use libcodrop::{compress, decompress, CodropError, CompressionLevel};

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GENERAL_FAILURE: i32 = 1;
pub const EXIT_MALFORMED_STREAM: i32 = 2;
pub const EXIT_UNSUPPORTED_FEATURE: i32 = 3;

fn error_to_exit_code(err: &CodropError) -> i32 {
    match err {
        CodropError::UnsupportedVersion { .. } | CodropError::UnsupportedBlockType(_) => {
            EXIT_UNSUPPORTED_FEATURE
        }
        CodropError::InvalidMagic(_)
        | CodropError::HeaderChecksumMismatch { .. }
        | CodropError::CorruptedHeader(_)
        | CodropError::InvalidBlockType(_)
        | CodropError::InvalidOffset { .. }
        | CodropError::InvalidMatchLength { .. }
        | CodropError::CorruptedEntropyStream(_)
        | CodropError::BlockChecksumMismatch { .. }
        | CodropError::StreamChecksumMismatch { .. }
        | CodropError::UnexpectedEof
        | CodropError::DecompressionBombDetected { .. }
        | CodropError::InvalidWindowSize(_)
        | CodropError::MemoryLimitExceeded { .. } => EXIT_MALFORMED_STREAM,
        CodropError::Io(_) => EXIT_GENERAL_FAILURE,
    }
}

fn print_usage() {
    eprintln!(
        r#"Codrop CLI - Universal Adaptive Compression System (M0: RAW + RLE)

USAGE:
    codrop compress <input> [-o <output>] [--level <fast|balanced|compact|auto>]
    codrop decompress <input.cdp> [-o <output>]
    codrop inspect <file.cdp>

OPTIONS:
    -o, --output <path>    Specify output file path
    --level <name>         Compression level: fast, balanced (default), compact, auto
    -h, --help             Display this help message

EXIT CODES:
    0    Success
    1    General failure / I/O error
    2    Invalid input / malformed Codrop stream
    3    Unsupported feature / format version
"#
    );
}

fn parse_level(s: &str) -> CompressionLevel {
    match s.to_lowercase().as_str() {
        "fast" => CompressionLevel::Fast,
        "balanced" => CompressionLevel::Balanced,
        "compact" => CompressionLevel::Compact,
        "auto" => CompressionLevel::Auto,
        _ => {
            eprintln!("Warning: Unknown level '{}', defaulting to 'balanced'", s);
            CompressionLevel::Balanced
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(EXIT_GENERAL_FAILURE);
    }

    let result = match args[1].as_str() {
        "compress" => cmd_compress(&args[2..]),
        "decompress" => cmd_decompress(&args[2..]),
        "inspect" => cmd_inspect(&args[2..]),
        "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => {
            eprintln!("Error: Unknown command '{}'", other);
            print_usage();
            Err(CodropError::Io(format!("Unknown command '{}'", other)))
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        let code = error_to_exit_code(&err);
        process::exit(code);
    }
}

fn cmd_compress(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop compress <input> [-o <output>] [--level <level>]");
        return Err(CodropError::Io("Missing input file argument".into()));
    }

    let input_path = PathBuf::from(&args[0]);
    let mut output_path = None;
    let mut level = CompressionLevel::Balanced;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" if i + 1 < args.len() => {
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--level" if i + 1 < args.len() => {
                level = parse_level(&args[i + 1]);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let out_file = output_path.unwrap_or_else(|| {
        let mut p = input_path.clone();
        p.set_extension("cdp");
        p
    });

    let data = fs::read(&input_path).map_err(|e| CodropError::Io(e.to_string()))?;
    let start = Instant::now();
    let compressed = compress(&data, level)?;
    let elapsed = start.elapsed();

    fs::write(&out_file, &compressed).map_err(|e| CodropError::Io(e.to_string()))?;

    let ratio = (compressed.len() as f64) / (data.len().max(1) as f64);
    let speed_mb = (data.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

    println!(
        "Compressed: {} -> {}",
        input_path.display(),
        out_file.display()
    );
    println!("  Original size:   {} bytes", data.len());
    println!("  Compressed size: {} bytes", compressed.len());
    println!(
        "  Ratio:           {:.4} ({:.2}% savings)",
        ratio,
        (1.0 - ratio) * 100.0
    );
    println!(
        "  Time elapsed:    {:.3} ms ({:.2} MB/s)",
        elapsed.as_secs_f64() * 1000.0,
        speed_mb
    );

    Ok(())
}

fn cmd_decompress(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop decompress <input.cdp> [-o <output>]");
        return Err(CodropError::Io("Missing input file argument".into()));
    }

    let input_path = PathBuf::from(&args[0]);
    let mut output_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" if i + 1 < args.len() => {
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let out_file = output_path.unwrap_or_else(|| {
        let mut p = input_path.clone();
        if p.extension().is_some_and(|ext| ext == "cdp") {
            p.set_extension("out");
        } else {
            p.set_extension("decompressed");
        }
        p
    });

    let compressed = fs::read(&input_path).map_err(|e| CodropError::Io(e.to_string()))?;
    let start = Instant::now();
    let decompressed = decompress(&compressed)?;
    let elapsed = start.elapsed();

    fs::write(&out_file, &decompressed).map_err(|e| CodropError::Io(e.to_string()))?;

    let speed_mb =
        (decompressed.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

    println!(
        "Decompressed: {} -> {}",
        input_path.display(),
        out_file.display()
    );
    println!("  Compressed size:   {} bytes", compressed.len());
    println!("  Decompressed size: {} bytes", decompressed.len());
    println!(
        "  Time elapsed:      {:.3} ms ({:.2} MB/s)",
        elapsed.as_secs_f64() * 1000.0,
        speed_mb
    );

    Ok(())
}

fn cmd_inspect(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop inspect <file.cdp>");
        return Err(CodropError::Io("Missing file path".into()));
    }

    let path_str = &args[0];
    let path = Path::new(path_str);
    let mut file = File::open(path).map_err(|e| CodropError::Io(e.to_string()))?;
    let header = StreamHeader::read_from(&mut file)?;

    let mut total_blocks = 0usize;
    let mut raw_blocks = 0usize;
    let mut rle_blocks = 0usize;
    let mut other_blocks = 0usize;
    let mut total_uncompressed_bytes = 0u64;
    let mut total_compressed_payload_bytes = 0u64;

    loop {
        let block_header = match BlockHeader::read_from(&mut file) {
            Ok(h) => h,
            Err(CodropError::UnexpectedEof) => break,
            Err(e) => return Err(e),
        };

        if block_header.block_type == BlockType::EndOfStream {
            break;
        }

        total_blocks += 1;
        match block_header.block_type {
            BlockType::Raw => raw_blocks += 1,
            BlockType::Rle => rle_blocks += 1,
            _ => other_blocks += 1,
        }

        total_uncompressed_bytes += block_header.uncompressed_size as u64;
        total_compressed_payload_bytes += block_header.compressed_size as u64;

        // Skip payload
        file.seek(SeekFrom::Current(block_header.compressed_size as i64))
            .map_err(|e| CodropError::Io(e.to_string()))?;
    }

    let file_size = fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(total_compressed_payload_bytes);

    let original_size = header.uncompressed_size.unwrap_or(total_uncompressed_bytes);
    let ratio = (file_size as f64) / (original_size.max(1) as f64);
    let savings_pct = (1.0 - ratio) * 100.0;

    println!("Format: CDP1");
    println!("Version: {}.{}", header.version_major, header.version_minor);
    println!("Blocks: {}", total_blocks);
    println!("RAW blocks: {}", raw_blocks);
    println!("RLE blocks: {}", rle_blocks);
    if other_blocks > 0 {
        println!("Other blocks: {}", other_blocks);
    }
    println!("Original size: {} bytes", original_size);
    println!("Compressed size: {} bytes", file_size);
    println!("Ratio: {:.4} ({:.2}% savings)", ratio, savings_pct);

    Ok(())
}
