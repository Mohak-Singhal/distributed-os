use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub extension: String,
    pub is_compressed: bool,
    pub is_sparse: bool,
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub entries: Vec<FileEntry>,
    pub count: u32,
    pub total_size: u64,
    pub largest_size: u64,
    pub average_size: u64,
    pub smallest_size: u64,
    pub all_same_extension: bool,
    pub primary_extension: String,
    pub compression_ratio_estimate: f64,
    pub contains_compressed_types: bool,
    pub category: FileCategory,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileCategory {
    Tiny,        // < 1KB
    Small,       // < 10MB
    Medium,      // < 500MB
    Large,       // < 5GB
    Huge,        // < 100GB
    Massive,     // >= 100GB
    ManyTiny,    // 1000+ files, avg < 10KB
    ManySmall,   // 100+ files, avg < 1MB
    Mixed,
}

const COMPRESSED_EXTENSIONS: &[&str] = &[
    "zip", "gz", "bz2", "xz", "zst", "7z", "rar", "tar",
    "jpg", "jpeg", "png", "gif", "webp", "avif", "heic",
    "mp4", "mkv", "avi", "mov", "webm", "mp3", "aac", "flac", "ogg",
    "pdf", "docx", "xlsx", "pptx",
];

pub async fn analyze_files(paths: &[String]) -> FileAnalysis {
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut largest = 0u64;
    let mut smallest = u64::MAX;
    let mut exts = std::collections::HashMap::new();
    let mut compressed_count = 0u32;

    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            collect_dir(path, &mut entries, &mut total, &mut largest, &mut smallest, &mut exts, &mut compressed_count).await;
        } else if path.is_file() {
            collect_file(path, &mut entries, &mut total, &mut largest, &mut smallest, &mut exts, &mut compressed_count).await;
        }
    }

    let count = entries.len() as u32;
    if count == 0 {
        return FileAnalysis {
            entries,
            count: 0,
            total_size: 0,
            largest_size: 0,
            average_size: 0,
            smallest_size: 0,
            all_same_extension: true,
            primary_extension: String::new(),
            compression_ratio_estimate: 1.0,
            contains_compressed_types: false,
            category: FileCategory::Tiny,
        };
    }

    let avg = total / count as u64;
    let primary_ext = exts.iter().max_by_key(|(_, &c)| c).map(|(e, _)| e.clone()).unwrap_or_default();
    let all_same = exts.len() <= 1;

    let category = classify_category(count, total, avg, smallest);
    let contains_compressed = compressed_count > count / 3;
    let ratio_estimate = if contains_compressed { 0.3 } else { 1.0 };

    FileAnalysis {
        entries,
        count,
        total_size: total,
        largest_size: largest,
        average_size: avg,
        smallest_size: if smallest == u64::MAX { 0 } else { smallest },
        all_same_extension: all_same,
        primary_extension: primary_ext,
        compression_ratio_estimate: ratio_estimate,
        contains_compressed_types: contains_compressed,
        category,
    }
}

fn classify_category(count: u32, total: u64, average: u64, smallest: u64) -> FileCategory {
    if count >= 1000 && average < 10_000 { return FileCategory::ManyTiny; }
    if count >= 100 && average < 1_000_000 { return FileCategory::ManySmall; }
    if count == 1 {
        if total < 1024 { return FileCategory::Tiny; }
        if total < 10_000_000 { return FileCategory::Small; }
        if total < 500_000_000 { return FileCategory::Medium; }
        if total < 5_000_000_000 { return FileCategory::Large; }
        if total < 100_000_000_000 { return FileCategory::Huge; }
        return FileCategory::Massive;
    }
    if count > 1 && average < 1024 { return FileCategory::ManyTiny; }
    FileCategory::Mixed
}

async fn collect_dir(
    dir: &Path,
    entries: &mut Vec<FileEntry>,
    total: &mut u64,
    largest: &mut u64,
    smallest: &mut u64,
    exts: &mut std::collections::HashMap<String, u32>,
    compressed_count: &mut u32,
) {
    if let Ok(read) = tokio::fs::read_dir(dir).await {
        let mut entries_handle = read;
        loop {
            match entries_handle.next_entry().await {
                Ok(Some(entry)) => {
                    let path = entry.path();
                    if path.is_dir() {
                        Box::pin(collect_dir(&path, entries, total, largest, smallest, exts, compressed_count)).await;
                    } else {
                        collect_file(&path, entries, total, largest, smallest, exts, compressed_count).await;
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }
}

async fn collect_file(
    path: &Path,
    entries: &mut Vec<FileEntry>,
    total: &mut u64,
    largest: &mut u64,
    smallest: &mut u64,
    exts: &mut std::collections::HashMap<String, u32>,
    compressed_count: &mut u32,
) {
    if let Ok(meta) = path.metadata() {
        let size = meta.len();
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        *total += size;
        *largest = (*largest).max(size);
        *smallest = (*smallest).min(size);
        *exts.entry(ext.clone()).or_insert(0) += 1;

        let is_compressed = COMPRESSED_EXTENSIONS.contains(&ext.as_str());
        if is_compressed { *compressed_count += 1; }

        entries.push(FileEntry {
            path: path.to_string_lossy().to_string(),
            size,
            extension: ext,
            is_compressed,
            is_sparse: false,
        });
    }
}

impl FileCategory {
    pub fn label(&self) -> &'static str {
        match self {
            FileCategory::Tiny => "Tiny (<1KB)",
            FileCategory::Small => "Small (<10MB)",
            FileCategory::Medium => "Medium (<500MB)",
            FileCategory::Large => "Large (<5GB)",
            FileCategory::Huge => "Huge (<100GB)",
            FileCategory::Massive => "Massive (100GB+)",
            FileCategory::ManyTiny => "Many tiny files",
            FileCategory::ManySmall => "Many small files",
            FileCategory::Mixed => "Mixed",
        }
    }
}
