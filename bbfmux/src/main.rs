#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use anyhow::{Context, Result, bail};
use bbf::{BBFBuilder, BBFMediaType, BBFReader, format::BBFFooter};
use clap::{Parser, Subcommand};
use memmap2::Mmap;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Input files or directories
    #[arg(value_name = "INPUTS")]
    inputs: Vec<PathBuf>,

    /// Output filename (default: output.bbf)
    #[arg(short, long, default_value = "output.bbf")]
    output: String,

    #[command(subcommand)]
    command: Option<Commands>,

    // --- Muxing Flags ---
    /// Use a text file to define page order (filename:index)
    #[arg(long)]
    order: Option<PathBuf>,

    /// Use a text file to define multiple sections (Name:Target[:Parent])
    #[arg(long)]
    sections: Option<PathBuf>,

    /// Add a single section marker (Name:Target[:Parent])
    #[arg(long)]
    section: Vec<String>,

    /// Add archival metadata (Key:Value)
    #[arg(long)]
    meta: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Display book structure and metadata
    Info { file: PathBuf },
    /// Perform integrity check on assets
    Verify {
        file: PathBuf,
        /// Optional specific asset index to verify.
        /// -1 verifies directory hash only.
        /// Omission verifies everything.
        index: Option<i32>,
    },
    /// Extract content from a BBF file
    Extract {
        file: PathBuf,
        /// Output directory
        #[arg(long, default_value = "./extracted")]
        outdir: PathBuf,
        /// Extract only a specific section
        #[arg(long)]
        section: Option<String>,
        /// Stop extraction when next section title matches this string
        #[arg(long)]
        rangekey: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct PagePlan {
    path: PathBuf,
    filename: String,
    order: i32, // 0 = unspecified, >0 = start, <0 = end
}

struct SectionReq {
    name: String,
    target: String,
    parent: String,
    is_filename: bool,
}

struct MetaReq {
    key: String,
    value: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Info { file }) => cmd_info(file),
        Some(Commands::Verify { file, index }) => cmd_verify(file, *index),
        Some(Commands::Extract {
            file,
            outdir,
            section,
            rangekey,
        }) => cmd_extract(file, outdir, section.as_deref(), rangekey.as_deref()),
        None => cmd_mux(&cli),
    }
}

#[allow(clippy::too_many_lines)]
fn cmd_mux(cli: &Cli) -> Result<()> {
    if cli.inputs.is_empty() {
        bail!("Error: No .bbf input specified.");
    }

    let mut manifest = Vec::new();
    let mut order_map = HashMap::new();

    if let Some(order_path) = &cli.order {
        let content = fs::read_to_string(order_path).context("Failed to read order file")?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((fname, idx_str)) = line.rsplit_once(':') {
                let fname = trim_quotes(fname);
                let idx = idx_str.parse::<i32>().unwrap_or(0);
                order_map.insert(fname, idx);
            } else {
                order_map.insert(trim_quotes(line), 0);
            }
        }
    }

    for input_path in &cli.inputs {
        if input_path.is_dir() {
            for entry in fs::read_dir(input_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    add_to_manifest(&mut manifest, path, &order_map);
                }
            }
        } else {
            add_to_manifest(&mut manifest, input_path.clone(), &order_map);
        }
    }

    manifest.sort_by(compare_pages);

    let mut sec_reqs = Vec::new();

    if let Some(sec_path) = &cli.sections {
        let content = fs::read_to_string(sec_path).context("Failed to read sections file")?;
        for line in content.lines() {
            if !line.trim().is_empty() {
                sec_reqs.push(parse_section_string(line));
            }
        }
    }

    for s_str in &cli.section {
        sec_reqs.push(parse_section_string(s_str));
    }

    let mut meta_reqs = Vec::new();
    for m_str in &cli.meta {
        if let Some((k, v)) = m_str.split_once(':') {
            meta_reqs.push(MetaReq {
                key: trim_quotes(k),
                value: trim_quotes(v),
            });
        }
    }

    let file = File::create(&cli.output).context("Cannot create output file")?;
    let mut builder = BBFBuilder::new(file)?;

    let mut file_to_page_idx = HashMap::new();

    for (i, p) in manifest.iter().enumerate() {
        let input_file =
            File::open(&p.path).with_context(|| format!("Failed to open {}", p.path.display()))?;

        let file_len = input_file.metadata()?.len();

        let ext = p
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let media_type = BBFMediaType::from_extension(&format!(".{ext}"));

        if file_len == 0 {
            builder.add_page(&[], media_type, 0)?;
        } else {
            let mmap = unsafe { Mmap::map(&input_file)? };
            builder.add_page(&mmap, media_type, 0)?;
        }

        file_to_page_idx.insert(p.filename.clone(), i as u32);
    }

    let mut section_name_to_idx = HashMap::new();

    for (i, req) in sec_reqs.iter().enumerate() {
        let page_idx = if req.is_filename {
            if let Some(&idx) = file_to_page_idx.get(&req.target) {
                idx
            } else {
                eprintln!(
                    "Warning: Section target file '{}' not found. Defaulting to page 1.",
                    req.target
                );
                0
            }
        } else {
            req.target.parse::<u32>().unwrap_or(1).saturating_sub(1)
        };

        let parent_idx = if req.parent.is_empty() {
            None
        } else {
            section_name_to_idx.get(&req.parent).copied()
        };

        builder.add_section(&req.name, page_idx, parent_idx);
        section_name_to_idx.insert(req.name.clone(), i as u32);
    }

    for m in meta_reqs {
        builder.add_metadata(&m.key, &m.value);
    }

    builder.finalize()?;
    println!(
        "Successfully created {} ({} pages)",
        cli.output,
        manifest.len()
    );
    Ok(())
}

fn cmd_info(path: &Path) -> Result<()> {
    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };

    let reader = BBFReader::new(&mmap[..])
        .map_err(|e| anyhow::anyhow!("Error: Failed to parse BBF. {e:?}"))?;

    println!("Bound Book Format (.bbf) Info");
    println!("------------------------------");
    println!("BBF Version: {}", reader.header.version);
    println!("Pages:       {}", reader.footer.page_count.get());
    println!(
        "Assets:      {} (Deduplicated)",
        reader.footer.asset_count.get()
    );

    println!("\n[Sections]");
    let sections = reader.sections();
    if sections.is_empty() {
        println!(" No sections defined.");
    } else {
        for s in sections {
            let title = reader
                .get_string(s.section_title_offset.get())
                .unwrap_or("???");
            println!(
                " - {:<20} (Starting Page: {})",
                title,
                s.section_start_index.get() + 1
            );
        }
    }

    println!("\n[Metadata]");
    let metadata = reader.metadata();
    if metadata.is_empty() {
        println!(" No metadata found.");
    } else {
        for m in metadata {
            let k = reader.get_string(m.key_offset.get()).unwrap_or("?");
            let v = reader.get_string(m.value_offset.get()).unwrap_or("?");
            println!(" - {k:<15}:{v}");
        }
    }
    println!();
    Ok(())
}

fn cmd_verify(path: &Path, user_index: Option<i32>) -> Result<()> {
    let target_index = user_index.unwrap_or(-2);

    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };

    let reader = BBFReader::new(&mmap[..])
        .map_err(|e| anyhow::anyhow!("Error: Failed to parse BBF. {e:?}"))?;

    let data = &mmap[..];

    let meta_start = reader.footer.string_pool_offset.get() as usize;
    let meta_size = data.len() - size_of::<BBFFooter>() - meta_start;

    if meta_start + meta_size > data.len() {
        bail!("File corrupted: Table offsets invalid");
    }

    let calc_index_hash = xxh3_64(&data[meta_start..meta_start + meta_size]);
    let dir_ok = calc_index_hash == reader.footer.footer_hash.get();

    if target_index == -1 {
        println!("Directory Hash: {}", if dir_ok { "OK" } else { "CORRUPT" });
        return if dir_ok {
            Ok(())
        } else {
            bail!("Directory hash mismatch")
        };
    }

    println!("Verifying integrity using XXH3 (Parallel)...");
    if !dir_ok {
        eprintln!(
            " [!!] Directory Hash CORRUPT (Wanted: {}, Got: {})",
            reader.footer.footer_hash.get(),
            calc_index_hash
        );
    }

    let assets = reader.assets();
    let check_asset = |idx: usize| -> bool {
        let asset = &assets[idx];
        let start = asset.file_offset.get() as usize;
        let len = asset.file_size.get() as usize;

        if start + len > data.len() {
            eprintln!(" [!!] Asset {idx} CORRUPT (Out of bounds)");
            return false;
        }

        let slice = &data[start..start + len];
        let hash = xxhash_rust::xxh3::xxh3_128(slice);
        let hash_lo = (hash & 0xFFFF_FFFF_FFFF_FFFF) as u64;
        let hash_hi = (hash >> 64) as u64;
        if hash_lo != asset.asset_hash[0].get() || hash_hi != asset.asset_hash[1].get() {
            eprintln!(" [!!] Asset {idx} CORRUPT");
            return false;
        }
        true
    };

    let all_assets_ok = if target_index >= 0 {
        check_asset(target_index as usize)
    } else {
        (0..assets.len())
            .into_par_iter()
            .map(check_asset)
            .reduce(|| true, |a, b| a && b)
    };

    if all_assets_ok && dir_ok {
        println!("All integrity checks passed.");
        Ok(())
    } else {
        bail!("Integrity checks failed.");
    }
}

fn cmd_extract(
    path: &Path,
    outdir: &Path,
    section_filter: Option<&str>,
    range_key: Option<&str>,
) -> Result<()> {
    let file = File::open(path).context("Failed to open BBF")?;
    let mmap = unsafe { Mmap::map(&file).context("Failed to mmap BBF")? };

    let reader = BBFReader::new(&mmap[..])
        .map_err(|e| anyhow::anyhow!("Error: Failed to parse BBF. {e:?}"))?;

    fs::create_dir_all(outdir)?;

    let pages = reader.pages();
    let sections = reader.sections();

    let mut start_idx: u64 = 0;
    let mut end_idx: u64 = pages.len() as u64;

    let mut section_name_found = "Full Book";

    if let Some(filter) = section_filter {
        let mut found = false;
        for (i, s) in sections.iter().enumerate() {
            let title = reader
                .get_string(s.section_title_offset.get())
                .unwrap_or("");
            if title == filter {
                start_idx = s.section_start_index.get();
                section_name_found = title;

                end_idx = pages.len() as u64;

                for next_s in sections.iter().skip(i + 1) {
                    let next_title = reader
                        .get_string(next_s.section_title_offset.get())
                        .unwrap_or("");

                    if let Some(rk) = range_key {
                        if !rk.is_empty() && next_title.contains(rk) {
                            end_idx = next_s.section_start_index.get();
                            break;
                        }
                        if rk.is_empty() && next_s.section_start_index.get() > start_idx {
                            end_idx = next_s.section_start_index.get();
                            break;
                        }
                    } else if next_s.section_start_index.get() > start_idx {
                        end_idx = next_s.section_start_index.get();
                        break;
                    }
                }
                found = true;
                break;
            }
        }
        if !found {
            bail!("Section '{filter}' not found.");
        }
    }

    println!(
        "Extracting: {} (Pages {} to {})",
        section_name_found,
        start_idx + 1,
        end_idx
    );

    let data = &mmap[..];

    for i in start_idx..end_idx {
        if i as usize >= pages.len() {
            break;
        }

        let page = &pages[i as usize];
        let asset = &reader.assets()[page.asset_index.get() as usize];

        let ext = BBFMediaType::from(asset.type_).as_extension();

        let out_name = format!("p{}{}", i + 1, ext);
        let out_path = outdir.join(out_name);

        let file_offset = asset.file_offset.get() as usize;
        let file_len = asset.file_size.get() as usize;

        if file_offset + file_len > data.len() {
            eprintln!("Warning: Page {i} out of bounds, skipping.");
            continue;
        }

        let mut f = File::create(out_path)?;
        f.write_all(&data[file_offset..file_offset + file_len])?;
    }

    println!("Done.");
    Ok(())
}

fn add_to_manifest(manifest: &mut Vec<PagePlan>, path: PathBuf, order_map: &HashMap<String, i32>) {
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let order = *order_map.get(&filename).unwrap_or(&0);
    manifest.push(PagePlan {
        path,
        filename,
        order,
    });
}

fn parse_section_string(s: &str) -> SectionReq {
    let mut parts: Vec<&str> = Vec::new();
    for part in s.split(':') {
        parts.push(part);
    }

    let name = trim_quotes(parts.first().unwrap_or(&""));
    let target = trim_quotes(parts.get(1).unwrap_or(&"1"));
    let parent = trim_quotes(parts.get(2).unwrap_or(&""));

    let is_filename = !target.chars().all(char::is_numeric);

    SectionReq {
        name,
        target,
        parent,
        is_filename,
    }
}

fn trim_quotes(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn compare_pages(a: &PagePlan, b: &PagePlan) -> Ordering {
    match (a.order, b.order) {
        (x, y) if x > 0 && y > 0 => x.cmp(&y),

        (x, y) if x > 0 && y <= 0 => Ordering::Less,
        (x, y) if x <= 0 && y > 0 => Ordering::Greater,

        (0, 0) => a.filename.cmp(&b.filename),

        (0, y) if y < 0 => Ordering::Less,
        (x, 0) if x < 0 => Ordering::Greater,

        (x, y) => x.cmp(&y),
    }
}
