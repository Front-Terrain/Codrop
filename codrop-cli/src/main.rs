use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use libcodrop::format::{BlockHeader, BlockType, StreamHeader};
use libcodrop::{
    compress, compress_image, decompress_with_limit, CodropError, CodropImageFormat,
    CompressionLevel, ImageOptions,
};

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GENERAL_FAILURE: i32 = 1;
pub const EXIT_MALFORMED_STREAM: i32 = 2;
pub const EXIT_UNSUPPORTED_FEATURE: i32 = 3;

const CLI_VERSION: &str = "codrop 1.0.0 (CDP1 container v1.0)";

fn error_to_exit_code(err: &CodropError) -> i32 {
    match err {
        CodropError::UnsupportedVersion { .. }
        | CodropError::UnsupportedBlockType(_)
        | CodropError::UnsupportedPrefilter(_)
        | CodropError::UnknownDictionary { .. } => EXIT_UNSUPPORTED_FEATURE,
        CodropError::InvalidMagic(_)
        | CodropError::HeaderChecksumMismatch { .. }
        | CodropError::CorruptedHeader(_)
        | CodropError::InvalidBlockType(_)
        | CodropError::InvalidOffset { .. }
        | CodropError::InvalidMatchLength { .. }
        | CodropError::CorruptedEntropyStream(_)
        | CodropError::CorruptedPrefilterData(_)
        | CodropError::BlockChecksumMismatch { .. }
        | CodropError::StreamChecksumMismatch { .. }
        | CodropError::UnexpectedEof
        | CodropError::DecompressionBombDetected { .. }
        | CodropError::InvalidWindowSize(_)
        | CodropError::MemoryLimitExceeded { .. } => EXIT_MALFORMED_STREAM,
        CodropError::Io(_) | CodropError::ImageError(_) => EXIT_GENERAL_FAILURE,
    }
}

fn print_usage() {
    eprintln!(
        r#"Codrop CLI - Universal Adaptive Compression System (RAW, RLE, LZF, LZH, LZA, Prefilters)

USAGE:
    codrop compress <input> [-o <output>] [-l|--level <fast|balanced|compact|auto>]
    codrop decompress <input.cdp> [-o <output>] [--max-size <bytes>]
    codrop image [compress] <input> [-o <output>] [-q|--quality <1-100>] [-f|--format <webp|png|jpeg|auto>]
    codrop inspect <file.cdp>
    codrop --version | -V

OPTIONS:
    -o, --output <path>    Specify output file path (use '-' for stdout)
    -l, --level <name>     Compression level: fast, balanced (default), compact, auto
    -q, --quality <1-100>  Image quality factor (default 85 for visually lossless 75-90% savings)
    -f, --format <name>    Image format: webp (default), png, jpeg, auto
    --max-size <bytes>     Maximum decompressed size limit in bytes (decompression bomb protection)
    -V, --version          Display version information
    -h, --help             Display this help message

NOTES:
    Use '-' as the input path to read from standard input.
    When streaming binary to stdout ('-o -'), diagnostic logs are written to stderr.

EXIT CODES:
    0    Success
    1    General failure / I/O error
    2    Invalid input / malformed Codrop stream / checksum error
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
        "image" => cmd_image(&args[2..]),
        "inspect" => cmd_inspect(&args[2..]),
        "-V" | "--version" => {
            println!("{}", CLI_VERSION);
            Ok(())
        }
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

fn cmd_image(args: &[String]) -> Result<(), CodropError> {
    let subargs = if !args.is_empty() && args[0] == "compress" {
        &args[1..]
    } else {
        args
    };

    if subargs.is_empty() {
        eprintln!(
            "Usage: codrop image [compress] <input> [-o <output>] [-q|--quality <1-100>] [-f|--format <webp|png|jpeg|auto>]"
        );
        return Err(CodropError::Io("Missing image input file argument".into()));
    }

    let input_arg = &subargs[0];
    let mut output_path = None;
    let mut quality = 85u8;
    let mut format = CodropImageFormat::Auto;

    let mut i = 1;
    while i < subargs.len() {
        match subargs[i].as_str() {
            "-o" | "--output" if i + 1 < subargs.len() => {
                output_path = Some(subargs[i + 1].clone());
                i += 1;
            }
            "-q" | "--quality" if i + 1 < subargs.len() => {
                if let Ok(q) = subargs[i + 1].parse::<u8>() {
                    quality = q.clamp(1, 100);
                }
                i += 1;
            }
            "-f" | "--format" if i + 1 < subargs.len() => {
                match subargs[i + 1].to_lowercase().as_str() {
                    "webp" => format = CodropImageFormat::WebP,
                    "png" => format = CodropImageFormat::Png,
                    "jpeg" | "jpg" => format = CodropImageFormat::Jpeg,
                    _ => format = CodropImageFormat::Auto,
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let is_stdin = input_arg == "-";
    let is_stdout = output_path.as_deref() == Some("-") || (is_stdin && output_path.is_none());

    let data = if is_stdin {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CodropError::Io(format!("stdin read failed: {e}")))?;
        buf
    } else {
        fs::read(input_arg)
            .map_err(|e| CodropError::Io(format!("failed to read '{input_arg}': {e}")))?
    };

    let start = Instant::now();
    let compressed = compress_image(&data, ImageOptions { format, quality })?;
    let elapsed = start.elapsed();

    if is_stdout {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&compressed)
            .map_err(|e| CodropError::Io(format!("stdout write failed: {e}")))?;
        stdout.flush().map_err(|e| CodropError::Io(e.to_string()))?;
    } else {
        let out_file = output_path.map(PathBuf::from).unwrap_or_else(|| {
            let mut p = PathBuf::from(input_arg);
            let ext = match format {
                CodropImageFormat::Auto | CodropImageFormat::WebP => "webp",
                CodropImageFormat::Png => "png",
                CodropImageFormat::Jpeg => "jpg",
            };
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("compressed");
            p.set_file_name(format!("{}_compressed.{}", stem, ext));
            p
        });

        fs::write(&out_file, &compressed).map_err(|e| {
            CodropError::Io(format!("failed to write '{}': {e}", out_file.display()))
        })?;

        let orig_len = data.len().max(1) as f64;
        let comp_len = compressed.len() as f64;
        let ratio = comp_len / orig_len;
        let savings_pct = (1.0 - ratio) * 100.0;
        let speed_mb = (data.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

        eprintln!(
            "Compressed Image: {} -> {}\n  Original size:   {} bytes\n  Compressed size: {} bytes\n  Savings:         {:.2}%\n  Time elapsed:    {:.3?} ({:.2} MB/s)",
            input_arg,
            out_file.display(),
            data.len(),
            compressed.len(),
            savings_pct,
            elapsed,
            speed_mb
        );
    }

    Ok(())
}

fn cmd_compress(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop compress <input> [-o <output>] [-l|--level <level>]");
        return Err(CodropError::Io("Missing input file argument".into()));
    }

    let input_arg = &args[0];
    let mut output_path = None;
    let mut level = CompressionLevel::Balanced;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" if i + 1 < args.len() => {
                output_path = Some(args[i + 1].clone());
                i += 1;
            }
            "-l" | "--level" if i + 1 < args.len() => {
                level = parse_level(&args[i + 1]);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let is_stdin = input_arg == "-";
    let is_stdout = output_path.as_deref() == Some("-") || (is_stdin && output_path.is_none());

    let data = if is_stdin {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CodropError::Io(format!("stdin read failed: {e}")))?;
        buf
    } else {
        fs::read(input_arg)
            .map_err(|e| CodropError::Io(format!("failed to read '{input_arg}': {e}")))?
    };

    let start = Instant::now();
    let compressed = compress(&data, level)?;
    let elapsed = start.elapsed();

    if is_stdout {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&compressed)
            .map_err(|e| CodropError::Io(format!("stdout write failed: {e}")))?;
        stdout.flush().map_err(|e| CodropError::Io(e.to_string()))?;
    } else {
        let out_file = output_path.map(PathBuf::from).unwrap_or_else(|| {
            let mut p = PathBuf::from(input_arg);
            p.set_extension("cdp");
            p
        });
        fs::write(&out_file, &compressed).map_err(|e| {
            CodropError::Io(format!("failed to write '{}': {e}", out_file.display()))
        })?;

        let ratio = (compressed.len() as f64) / (data.len().max(1) as f64);
        let speed_mb = (data.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

        println!("Compressed: {} -> {}", input_arg, out_file.display());
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
    }

    Ok(())
}

fn cmd_decompress(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop decompress <input.cdp> [-o <output>] [--max-size <bytes>]");
        return Err(CodropError::Io("Missing input file argument".into()));
    }

    let input_arg = &args[0];
    let mut output_path = None;
    let mut max_size = 1024 * 1024 * 1024; // 1 GB default

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" if i + 1 < args.len() => {
                output_path = Some(args[i + 1].clone());
                i += 1;
            }
            "--max-size" if i + 1 < args.len() => {
                if let Ok(limit) = args[i + 1].parse::<u64>() {
                    max_size = limit;
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let is_stdin = input_arg == "-";
    let is_stdout = output_path.as_deref() == Some("-") || (is_stdin && output_path.is_none());

    let compressed = if is_stdin {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CodropError::Io(format!("stdin read failed: {e}")))?;
        buf
    } else {
        fs::read(input_arg)
            .map_err(|e| CodropError::Io(format!("failed to read '{input_arg}': {e}")))?
    };

    let start = Instant::now();
    let decompressed = decompress_with_limit(&compressed, max_size)?;
    let elapsed = start.elapsed();

    if is_stdout {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&decompressed)
            .map_err(|e| CodropError::Io(format!("stdout write failed: {e}")))?;
        stdout.flush().map_err(|e| CodropError::Io(e.to_string()))?;
    } else {
        let out_file = output_path.map(PathBuf::from).unwrap_or_else(|| {
            let mut p = PathBuf::from(input_arg);
            if p.extension().is_some_and(|ext| ext == "cdp") {
                p.set_extension("out");
            } else {
                p.set_extension("decompressed");
            }
            p
        });

        fs::write(&out_file, &decompressed).map_err(|e| {
            CodropError::Io(format!("failed to write '{}': {e}", out_file.display()))
        })?;

        let speed_mb =
            (decompressed.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

        println!("Decompressed: {} -> {}", input_arg, out_file.display());
        println!("  Compressed size:   {} bytes", compressed.len());
        println!("  Decompressed size: {} bytes", decompressed.len());
        println!(
            "  Time elapsed:      {:.3} ms ({:.2} MB/s)",
            elapsed.as_secs_f64() * 1000.0,
            speed_mb
        );
    }

    Ok(())
}

fn cmd_inspect(args: &[String]) -> Result<(), CodropError> {
    if args.is_empty() {
        eprintln!("Usage: codrop inspect <file.cdp>");
        return Err(CodropError::Io("Missing file path".into()));
    }

    let path_str = &args[0];
    let path = Path::new(path_str);
    let mut file = File::open(path)
        .map_err(|e| CodropError::Io(format!("failed to open '{path_str}': {e}")))?;
    let header = StreamHeader::read_from(&mut file)?;

    let mut total_blocks = 0usize;
    let mut raw_blocks = 0usize;
    let mut rle_blocks = 0usize;
    let mut lzf_blocks = 0usize;
    let mut lzh_blocks = 0usize;
    let mut lza_blocks = 0usize;
    let mut prefilter_blocks = 0usize;
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
            BlockType::Lzf => lzf_blocks += 1,
            BlockType::Lzh => lzh_blocks += 1,
            BlockType::Lza => lza_blocks += 1,
            BlockType::TextPrefilter => prefilter_blocks += 1,
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
    println!("Total Blocks: {}", total_blocks);
    println!("  RAW blocks:         {}", raw_blocks);
    println!("  RLE blocks:         {}", rle_blocks);
    println!("  LZF blocks:         {}", lzf_blocks);
    println!("  LZH blocks:         {}", lzh_blocks);
    println!("  LZA blocks:         {}", lza_blocks);
    println!("  Prefilter blocks:   {}", prefilter_blocks);
    if other_blocks > 0 {
        println!("  Other blocks:       {}", other_blocks);
    }
    println!("Original size:   {} bytes", original_size);
    println!("Compressed size: {} bytes", file_size);
    println!(
        "Overall ratio:   {:.4} ({:.2}% savings)",
        ratio, savings_pct
    );

    Ok(())
}
