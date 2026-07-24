#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use anyhow::{Context, Result, bail};
use bbf::{BBFBuilder, BBFMediaType, BBFReader, format::BBFFooter};
use clap::{Parser, Subcommand};
use memmap2::Mmap;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Parser)]
#[command(name = "bbfmux", version, about = "A tool for creating and manipulating Bound Book Format (BBF) files", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Increase logging verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress all logging output
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new BBF file
    Create {
        /// Output filename
        output: PathBuf,

        /// Path to a TOML manifest file defining book structure
        #[arg(short, long)]
        manifest: Option<PathBuf>,

        /// Enable petrification (optimized for streaming)
        #[arg(short, long)]
        petrify: bool,

        /// Input files or directories (overrides/appends to manifest inputs)
        inputs: Vec<PathBuf>,
    },
    /// Extract content from a BBF file
    Extract {
        /// Input BBF file
        input: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "./extracted")]
        out_dir: PathBuf,

        /// Extract only a specific section by title
        #[arg(short, long)]
        section: Option<String>,
    },
    /// Display book structure and metadata
    Info {
        /// Input BBF file
        input: PathBuf,

        /// Output information in JSON format
        #[arg(long)]
        json: bool,
    },
    /// Perform integrity check on assets
    Verify {
        /// Input BBF file
        input: PathBuf,

        /// Optional specific asset index to verify. Omission verifies everything.
        #[arg(short, long)]
        index: Option<i32>,

        /// Output result in JSON format
        #[arg(long)]
        json: bool,
    },
}

#[derive(Deserialize, Debug, Default)]
struct Manifest {
    metadata: Option<HashMap<String, String>>,
    sections: Option<Vec<SectionConfig>>,
    options: Option<ManifestOptions>,
    inputs: Option<Vec<PathBuf>>,
}

#[derive(Deserialize, Debug, Default)]
struct ManifestOptions {
    #[serde(default)]
    petrify: bool,
}

#[derive(Deserialize, Debug)]
struct SectionConfig {
    name: String,
    target: String,
    parent: Option<String>,
}

#[derive(Serialize)]
struct InfoOutput {
    version: u16,
    pages: u64,
    assets: u64,
    petrified: bool,
    sections: Vec<SectionInfo>,
    metadata: HashMap<String, String>,
}

#[derive(Serialize)]
struct SectionInfo {
    title: String,
    start_page: u64,
}

fn setup_logging(verbose: u8, quiet: bool) {
    if quiet {
        return;
    }

    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_logging(cli.verbose, cli.quiet);

    match &cli.command {
        Commands::Create {
            output,
            manifest,
            petrify,
            inputs,
        } => cmd_create(output, manifest.as_deref(), *petrify, inputs),
        Commands::Extract {
            input,
            out_dir,
            section,
        } => cmd_extract(input, out_dir, section.as_deref()),
        Commands::Info { input, json } => cmd_info(input, *json),
        Commands::Verify { input, index, json } => cmd_verify(input, *index, *json),
    }
}

fn cmd_create(
    output: &Path,
    manifest_path: Option<&Path>,
    cli_petrify: bool,
    cli_inputs: &[PathBuf],
) -> Result<()> {
    let manifest = if let Some(path) = manifest_path {
        info!("Reading manifest from {}", path.display());
        let content = fs::read_to_string(path).context("Failed to read manifest file")?;
        toml::from_str(&content).context("Failed to parse TOML manifest")?
    } else {
        Manifest::default()
    };

    let is_petrified = cli_petrify || manifest.options.is_some_and(|o| o.petrify);

    let mut all_inputs = manifest.inputs.unwrap_or_default();
    all_inputs.extend_from_slice(cli_inputs);

    if all_inputs.is_empty() {
        bail!("Error: No input files specified in manifest or CLI.");
    }

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
        .context("Cannot create output file")?;

    let mut builder = BBFBuilder::new(file)?;

    let mut file_to_page_idx = HashMap::new();
    let mut page_idx = 0;

    for input_path in &all_inputs {
        if input_path.is_dir() {
            debug!("Processing directory: {}", input_path.display());
            let mut entries = fs::read_dir(input_path)?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            entries.sort_by_key(std::fs::DirEntry::path);

            for entry in entries {
                if entry.path().is_file() {
                    add_file_to_builder(
                        &mut builder,
                        &entry.path(),
                        &mut file_to_page_idx,
                        &mut page_idx,
                    )?;
                }
            }
        } else {
            add_file_to_builder(
                &mut builder,
                input_path,
                &mut file_to_page_idx,
                &mut page_idx,
            )?;
        }
    }

    let mut section_name_to_idx = HashMap::new();

    if let Some(sections) = manifest.sections {
        for (i, sec) in sections.iter().enumerate() {
            let p_idx = if let Some(&idx) = file_to_page_idx.get(&sec.target) {
                idx
            } else if let Ok(parsed_idx) = sec.target.parse::<u32>() {
                parsed_idx.saturating_sub(1)
            } else {
                warn!(
                    "Section target '{}' not found. Defaulting to page 1.",
                    sec.target
                );
                0
            };

            let parent_idx = sec
                .parent
                .as_ref()
                .and_then(|p| section_name_to_idx.get(p).copied());
            builder.add_section(&sec.name, p_idx, parent_idx);
            section_name_to_idx.insert(sec.name.clone(), i as u32);
        }
    }

    if let Some(meta) = manifest.metadata {
        for (k, v) in meta {
            builder.add_metadata(&k, &v);
        }
    }

    builder.finalize()?;

    if is_petrified {
        info!("Petrifying file in-place...");
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(output)?;
        bbf::builder::petrify(&mut f)?;
        info!("Petrification complete.");
    }

    info!(
        "Successfully created {} ({} pages)",
        output.display(),
        page_idx
    );
    Ok(())
}

fn add_file_to_builder(
    builder: &mut BBFBuilder<File>,
    path: &Path,
    file_map: &mut HashMap<String, u32>,
    page_idx: &mut u32,
) -> Result<()> {
    debug!("Adding file: {}", path.display());
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let len = file.metadata()?.len();

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let media_type = BBFMediaType::from_extension(&format!(".{ext}"));

    if len == 0 {
        builder.add_page(&[], media_type, 0)?;
    } else {
        let mmap = unsafe { Mmap::map(&file)? };
        builder.add_page(&mmap, media_type, 0)?;
    }

    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    file_map.insert(filename, *page_idx);
    *page_idx += 1;
    Ok(())
}

fn cmd_info(path: &Path, json: bool) -> Result<()> {
    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };
    let reader =
        BBFReader::new(&mmap[..]).map_err(|e| anyhow::anyhow!("Error parsing BBF: {e:?}"))?;

    let is_petrified =
        reader.header.footer_offset.get() == std::mem::size_of::<bbf::format::BBFHeader>() as u64;

    let mut info_out = InfoOutput {
        version: reader.header.version.get(),
        pages: reader.footer.page_count.get(),
        assets: reader.footer.asset_count.get(),
        petrified: is_petrified,
        sections: Vec::new(),
        metadata: HashMap::new(),
    };

    for s in reader.sections() {
        let title = reader
            .get_string(s.section_title_offset.get())
            .unwrap_or("???")
            .to_string();
        info_out.sections.push(SectionInfo {
            title,
            start_page: s.section_start_index.get() + 1,
        });
    }

    for m in reader.metadata() {
        let k = reader
            .get_string(m.key_offset.get())
            .unwrap_or("?")
            .to_string();
        let v = reader
            .get_string(m.value_offset.get())
            .unwrap_or("?")
            .to_string();
        info_out.metadata.insert(k, v);
    }

    if json {
        let serialized = serde_json::to_string_pretty(&info_out)?;
        println!("{serialized}");
    } else {
        println!("Bound Book Format (.bbf) Info");
        println!("------------------------------");
        println!("BBF Version: {}", info_out.version);
        println!("Pages:       {}", info_out.pages);
        println!("Assets:      {} (Deduplicated)", info_out.assets);
        println!(
            "Petrified:   {}",
            if info_out.petrified { "Yes" } else { "No" }
        );

        println!("\n[Sections]");
        if info_out.sections.is_empty() {
            println!(" No sections defined.");
        } else {
            for s in &info_out.sections {
                println!(" - {:<20} (Starting Page: {})", s.title, s.start_page);
            }
        }

        println!("\n[Metadata]");
        if info_out.metadata.is_empty() {
            println!(" No metadata found.");
        } else {
            for (k, v) in &info_out.metadata {
                println!(" - {k:<15}:{v}");
            }
        }
    }

    Ok(())
}

fn cmd_verify(path: &Path, user_index: Option<i32>, json: bool) -> Result<()> {
    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };
    let reader =
        BBFReader::new(&mmap[..]).map_err(|e| anyhow::anyhow!("Error parsing BBF: {e:?}"))?;

    let data = &mmap[..];
    let meta_start = reader.footer.string_pool_offset.get() as usize;
    let meta_size = data.len() - size_of::<BBFFooter>() - meta_start;

    if meta_start + meta_size > data.len() {
        bail!("File corrupted: Table offsets invalid");
    }

    let calc_index_hash = xxh3_64(&data[meta_start..meta_start + meta_size]);
    let dir_ok = calc_index_hash == reader.footer.footer_hash.get();

    let assets = reader.assets();
    let target_index = user_index.unwrap_or(-2);

    let check_asset = |idx: usize| -> bool {
        let asset = &assets[idx];
        let start = asset.file_offset.get() as usize;
        let len = asset.file_size.get() as usize;

        if start + len > data.len() {
            if !json {
                eprintln!(" [!!] Asset {idx} CORRUPT (Out of bounds)");
            }
            return false;
        }

        let slice = &data[start..start + len];
        let hash = xxhash_rust::xxh3::xxh3_128(slice);
        let hash_lo = (hash & 0xFFFF_FFFF_FFFF_FFFF) as u64;
        let hash_hi = (hash >> 64) as u64;

        if hash_lo != asset.asset_hash[0].get() || hash_hi != asset.asset_hash[1].get() {
            if !json {
                eprintln!(" [!!] Asset {idx} CORRUPT");
            }
            return false;
        }
        true
    };

    let all_assets_ok = if target_index >= 0 {
        check_asset(target_index as usize)
    } else if target_index == -1 {
        true
    } else {
        (0..assets.len())
            .into_par_iter()
            .map(check_asset)
            .reduce(|| true, |a, b| a && b)
    };

    let success = dir_ok && all_assets_ok;

    if json {
        let out = serde_json::json!({
            "directory_ok": dir_ok,
            "assets_ok": all_assets_ok,
            "overall_success": success
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Directory Hash: {}", if dir_ok { "OK" } else { "CORRUPT" });
        if target_index != -1 {
            println!("Assets: {}", if all_assets_ok { "OK" } else { "CORRUPT" });
        }
    }

    if success {
        Ok(())
    } else {
        bail!("Integrity checks failed.")
    }
}

fn cmd_extract(path: &Path, outdir: &Path, section_filter: Option<&str>) -> Result<()> {
    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };
    let reader =
        BBFReader::new(&mmap[..]).map_err(|e| anyhow::anyhow!("Error parsing BBF: {e:?}"))?;

    fs::create_dir_all(outdir)?;

    let pages = reader.pages();
    let sections = reader.sections();

    let mut start_idx = 0;
    let mut end_idx = pages.len() as u64;
    let mut section_name = "Full Book";

    if let Some(filter) = section_filter {
        let mut found = false;
        for (i, s) in sections.iter().enumerate() {
            let title = reader
                .get_string(s.section_title_offset.get())
                .unwrap_or("");
            if title == filter {
                start_idx = s.section_start_index.get();
                section_name = title;

                end_idx = sections.get(i + 1).map_or(pages.len() as u64, |next_s| {
                    next_s.section_start_index.get()
                });

                found = true;
                break;
            }
        }
        if !found {
            bail!("Section '{filter}' not found.");
        }
    }

    info!(
        "Extracting: {} (Pages {} to {}) to {}",
        section_name,
        start_idx + 1,
        end_idx,
        outdir.display()
    );

    let data = &mmap[..];
    for i in start_idx..end_idx {
        let page = &pages[i as usize];
        let asset = &reader.assets()[page.asset_index.get() as usize];
        let ext = BBFMediaType::from(asset.type_).as_extension();

        let out_name = format!("p{}{}", i + 1, ext);
        let out_path = outdir.join(out_name);

        let offset = asset.file_offset.get() as usize;
        let len = asset.file_size.get() as usize;

        if offset + len > data.len() {
            warn!("Page {i} out of bounds, skipping.");
            continue;
        }

        let mut f = File::create(out_path)?;
        f.write_all(&data[offset..offset + len])?;
    }

    info!("Extraction complete.");
    Ok(())
}
