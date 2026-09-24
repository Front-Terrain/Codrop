use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use libcodrop::format::{BlockHeader, BlockType, StreamHeader};
use libcodrop::streaming::{Decoder, DecoderOptions};
use libcodrop::{CompressionLevel, compress, decompress};

fn print_usage() {
    eprintln!(
        r#"Codrop CLI - Universal Adaptive Compression System (v0.1.0)

USAGE:
    codrop compress <input> [-o <output>] [--level <fast|balanced|compact|max|auto>]
    codrop decompress <input> [-o <output>]
    codrop inspect <file.cdp>
    codrop test <file.cdp>
    codrop benchmark <path>

OPTIONS:
    -o, --output <path>    Specify output file path
    --level <name>         Compression level: fast, balanced (default), compact, max, auto
    -h, --help             Display this help message
"#
    );
}

fn parse_level(s: &str) -> CompressionLevel {
    match s.to_lowercase().as_str() {
        "fast" => CompressionLevel::Fast,
        "balanced" => CompressionLevel::Balanced,
        "compact" => CompressionLevel::Compact,
        "max" => CompressionLevel::Max,
        "auto" => CompressionLevel::Auto,
        _ => {
            eprintln!("Unknown level '{}', defaulting to 'balanced'", s);
            CompressionLevel::Balanced
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "compress" => cmd_compress(&args[2..])?,
        "decompress" => cmd_decompress(&args[2..])?,
        "inspect" => cmd_inspect(&args[2..])?,
        "test" => cmd_test(&args[2..])?,
        "benchmark" => cmd_benchmark(&args[2..])?,
        "-h" | "--help" => print_usage(),
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn cmd_compress(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Usage: codrop compress <input> [-o <output>] [--level <level>]");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[0]);
    let mut output_path = None;
    let mut level = CompressionLevel::Balanced;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if i + 1 < args.len() {
                    output_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--level" => {
                if i + 1 < args.len() {
                    level = parse_level(&args[i + 1]);
                    i += 1;
                }
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

    let data = fs::read(&input_path)?;
    let start = Instant::now();
    let compressed = compress(&data, level)?;
    let elapsed = start.elapsed();

    fs::write(&out_file, &compressed)?;

    let ratio = (compressed.len() as f64) / (data.len().max(1) as f64);
    let speed_mb = (data.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

    println!("Compressed: {} -> {}", input_path.display(), out_file.display());
    println!("  Original size:   {} bytes", data.len());
    println!("  Compressed size: {} bytes", compressed.len());
    println!("  Ratio:           {:.4} ({:.2}% savings)", ratio, (1.0 - ratio) * 100.0);
    println!("  Time elapsed:    {:.3} ms ({:.2} MB/s)", elapsed.as_secs_f64() * 1000.0, speed_mb);

    Ok(())
}

fn cmd_decompress(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Usage: codrop decompress <input.cdp> [-o <output>]");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[0]);
    let mut output_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if i + 1 < args.len() {
                    output_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let out_file = output_path.unwrap_or_else(|| {
        let mut p = input_path.clone();
        if p.extension().map_or(false, |ext| ext == "cdp") {
            p.set_extension("out");
        } else {
            p.set_extension("decompressed");
        }
        p
    });

    let compressed = fs::read(&input_path)?;
    let start = Instant::now();
    let decompressed = decompress(&compressed)?;
    let elapsed = start.elapsed();

    fs::write(&out_file, &decompressed)?;

    let speed_mb = (decompressed.len() as f64) / (1_000_000.0 * elapsed.as_secs_f64().max(0.000001));

    println!("Decompressed: {} -> {}", input_path.display(), out_file.display());
    println!("  Compressed size:   {} bytes", compressed.len());
    println!("  Decompressed size: {} bytes", decompressed.len());
    println!("  Time elapsed:      {:.3} ms ({:.2} MB/s)", elapsed.as_secs_f64() * 1000.0, speed_mb);

    Ok(())
}

fn cmd_inspect(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Usage: codrop inspect <file.cdp>");
        std::process::exit(1);
    }

    let path = &args[0];
    let mut file = File::open(path)?;
    let header = StreamHeader::read_from(&mut file)?;

    println!("--- Codrop Container Inspection: {} ---", path);
    println!("Format Version:       {}.{}", header.version_major, header.version_minor);
    println!("Negotiated Window:    {} KB ({} bytes)", header.window_size / 1024, header.window_size);
    println!("Uncompressed Size:    {:?}", header.uncompressed_size);
    println!("Stream Checksum Flag: {}", header.flags.has_stream_checksum);
    println!("Independent Blocks:   {}", header.flags.independent_blocks);
    println!("--- Blocks ---");

    let mut block_idx = 0;
    loop {
        let block_header = match BlockHeader::read_from(&mut file) {
            Ok(h) => h,
            Err(e) => {
                println!("Reached end of stream / {:?}", e);
                break;
            }
        };

        if block_header.block_type == BlockType::EndOfStream {
            println!("Block {:03}: [END_OF_STREAM]", block_idx);
            break;
        }

        println!(
            "Block {:03}: Type: {:?} | Compressed: {} B | Uncompressed: {} B | Checksum: {:08X?} | Last: {}",
            block_idx,
            block_header.block_type,
            block_header.compressed_size,
            block_header.uncompressed_size,
            block_header.checksum,
            block_header.is_last
        );

        // Skip payload
        let mut sink = vec![0u8; block_header.compressed_size as usize];
        file.read_exact(&mut sink)?;
        block_idx += 1;
    }

    Ok(())
}

fn cmd_test(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        eprintln!("Usage: codrop test <file.cdp>");
        std::process::exit(1);
    }

    let path = &args[0];
    let file = File::open(path)?;
    let mut decoder = Decoder::new(file, DecoderOptions::default())?;

    let mut sink = io::sink();
    let total = decoder.decompress_to(&mut sink)?;

    println!("SUCCESS: Stream '{}' is valid and verified intact.", path);
    println!("Total validated decompressed bytes: {}", total);

    Ok(())
}

fn cmd_benchmark(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let target = if args.is_empty() { "." } else { &args[0] };
    let path = Path::new(target);

    let mut files = Vec::new();
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_file() && p.extension().map_or(true, |ext| ext != "cdp") {
                files.push(p);
            }
        }
    }

    if files.is_empty() {
        println!("No benchmark files found at '{}'", target);
        return Ok(());
    }

    println!("==========================================================================================");
    println!("Codrop Performance Benchmark Suite");
    println!("==========================================================================================");
    println!(
        "{:<25} | {:<8} | {:>10} | {:>10} | {:>8} | {:>10} | {:>10}",
        "File", "Level", "Orig (B)", "Comp (B)", "Ratio", "Comp MB/s", "Decomp MB/s"
    );
    println!("------------------------------------------------------------------------------------------");

    let levels = [CompressionLevel::Fast, CompressionLevel::Balanced, CompressionLevel::Auto];

    for file_path in &files {
        let data = match fs::read(file_path) {
            Ok(d) if !d.is_empty() => d,
            _ => continue,
        };

        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let short_name = if file_name.len() > 25 { &file_name[..25] } else { &file_name };

        for &level in &levels {
            let level_name = format!("{:?}", level);

            // Compress
            let start_c = Instant::now();
            let compressed = compress(&data, level)?;
            let elapsed_c = start_c.elapsed();

            // Decompress
            let start_d = Instant::now();
            let decompressed = decompress(&compressed)?;
            let elapsed_d = start_d.elapsed();

            assert_eq!(data, decompressed, "Decompression verification failed!");

            let ratio = (compressed.len() as f64) / (data.len() as f64);
            let c_speed = (data.len() as f64) / (1_000_000.0 * elapsed_c.as_secs_f64().max(0.000001));
            let d_speed = (data.len() as f64) / (1_000_000.0 * elapsed_d.as_secs_f64().max(0.000001));

            println!(
                "{:<25} | {:<8} | {:>10} | {:>10} | {:>7.2}% | {:>10.2} | {:>10.2}",
                short_name,
                level_name,
                data.len(),
                compressed.len(),
                ratio * 100.0,
                c_speed,
                d_speed
            );
        }
    }
    println!("==========================================================================================");

    Ok(())
}
