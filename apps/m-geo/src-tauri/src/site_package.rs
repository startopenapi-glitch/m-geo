use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::HashSet,
    io::{self, Cursor, Read, Seek, SeekFrom},
};

const MAX_INPUT: usize = 20 * 1024 * 1024;
const MAX_EXPANDED: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 110;
const INVALID_ZIP: &str = "ZIP文件损坏或结构不支持";
const UNSAFE_PATH: &str = "ZIP文件路径不安全";
const TOO_LARGE: &str = "ZIP解压后超过16MB限制";

/// Reads an untrusted ZIP in memory only. Domain/package semantics are checked separately.
pub fn decode_site_package(name: &str, bytes: &[u8]) -> Result<Value, String> {
    decode_package(name, bytes, PackageProfile::Website)
}

/// The same hardened reader, restricted to the historical experiment's three files.
pub fn decode_experiment_package(name: &str, bytes: &[u8]) -> Result<Value, String> {
    decode_package(name, bytes, PackageProfile::Experiment)
}

/// Historical attribution report: the original nine root files, decoded in memory only.
pub fn decode_attribution_package(name: &str, bytes: &[u8]) -> Result<Value, String> {
    decode_package(name, bytes, PackageProfile::Attribution)
}

/// Historical channel operation evidence, decoded in memory only.
pub fn decode_channel_package(name: &str, bytes: &[u8]) -> Result<Value, String> {
    decode_package(name, bytes, PackageProfile::Channel)
}

#[derive(Clone, Copy)]
enum PackageProfile {
    Website,
    Experiment,
    Attribution,
    Channel,
}

fn decode_package(name: &str, bytes: &[u8], profile: PackageProfile) -> Result<Value, String> {
    let file_name = name.rsplit(['/', '\\']).next().unwrap_or("");
    if file_name.is_empty()
        || file_name.chars().count() > 255
        || file_name.chars().any(char::is_control)
        || !file_name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("zip"))
    {
        return Err("请选择文件名有效的ZIP文件".into());
    }
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return Err("请选择非空ZIP文件，最大20MB".into());
    }
    // zip 2 indexes by name, which otherwise hides exact duplicate central entries.
    // Preflight also bounds metadata allocation before ZipArchive reads its entry count.
    let directory = preflight_directory(bytes)?;
    if matches!(profile, PackageProfile::Experiment)
        && (directory.paths.len() != 3
            || directory
                .paths
                .iter()
                .any(|path| !matches!(*path, "report.md" | "evidence.json" | "observations.csv")))
    {
        return Err("实验ZIP必须仅包含report.md、evidence.json和observations.csv".into());
    }
    if matches!(profile, PackageProfile::Attribution)
        && (directory.paths.len() != 9
            || directory.paths.iter().any(|path| {
                !matches!(
                    *path,
                    "summary.json"
                        | "dataset.json"
                        | "input.json"
                        | "orders.csv"
                        | "sources.csv"
                        | "products.csv"
                        | "weeks.csv"
                        | "methodology.txt"
                        | "external-requirements.txt"
                )
            }))
    {
        return Err("归因ZIP必须仅包含原始报告的九个文件".into());
    }
    if matches!(profile, PackageProfile::Channel) {
        let supported = [
            [
                "comparison-report.json",
                "baseline-manifest.json",
                "feed-manifest.json",
                "local-operation-notice.txt",
            ],
            [
                "simulation-report.json",
                "simulated-receipt.json",
                "feed-manifest.json",
                "local-operation-notice.txt",
            ],
            [
                "receipt-check-report.json",
                "simulated-receipt.json",
                "feed-manifest.json",
                "local-operation-notice.txt",
            ],
        ];
        if directory.paths.len() != 4
            || !supported
                .iter()
                .any(|set| directory.paths.iter().all(|path| set.contains(path)))
        {
            return Err("历史通道ZIP必须仅包含比较、模拟或回执核对的四个原始文件".into());
        }
    }
    let constructing = Cell::new(true);
    let reader = MetadataReader {
        cursor: Cursor::new(bytes),
        eocd: directory.eocd,
        zip64: directory.zip64,
        constructing: &constructing,
        seek_start: None,
    };
    // Exclude comment candidates and forbid parsing unselected end-record headers
    // before zip can read their entry counts or allocate metadata from those counts.
    let mut archive = zip::ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(directory.archive_offset),
        },
        reader,
    )
    .map_err(|_| INVALID_ZIP)?;
    constructing.set(false); // Actual entry data must be read byte-for-byte unchanged.
    let paths = directory.paths;
    let central_start = directory.start;
    if archive.len() != paths.len() || archive.central_directory_start() != central_start as u64 {
        return Err(INVALID_ZIP.into());
    }
    let mut declared_total = 0_u64;
    for (index, path) in paths.iter().enumerate() {
        let entry = archive.by_index_raw(index).map_err(|_| INVALID_ZIP)?;
        if entry.name_raw() != path.as_bytes() || entry.name() != *path {
            return Err(UNSAFE_PATH.into());
        }
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
        {
            return Err("ZIP包含加密或不支持的压缩条目".into());
        }
        if entry.is_dir()
            || entry.is_symlink()
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err("ZIP仅支持普通文本文件".into());
        }
        validate_local_header(bytes, central_start, path, &entry)?;
        declared_total = declared_total.checked_add(entry.size()).ok_or(TOO_LARGE)?;
        if declared_total > MAX_EXPANDED {
            return Err(TOO_LARGE.into());
        }
    }
    let mut actual_total = 0_u64;
    let mut files = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let entry = archive.by_index(index).map_err(|_| INVALID_ZIP)?;
        let declared = entry.size();
        let remaining = MAX_EXPANDED - actual_total;
        let mut content = Vec::new();
        // One extra byte distinguishes EOF at the limit from expansion beyond the limit.
        entry
            .take(remaining + 1)
            .read_to_end(&mut content)
            .map_err(|_| INVALID_ZIP)?;
        if content.len() as u64 > remaining {
            return Err(TOO_LARGE.into());
        }
        if content.len() as u64 != declared {
            return Err(INVALID_ZIP.into());
        }
        actual_total += content.len() as u64;
        let content = String::from_utf8(content).map_err(|_| "ZIP文件必须使用UTF-8文本")?;
        if content.contains('\0')
            || (matches!(profile, PackageProfile::Website) && content.contains('\u{feff}'))
        {
            return Err("ZIP文本不能包含BOM或空字符".into());
        }
        files.push(json!({"path": path, "content": content}));
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({
        "source": {
            "fileName": file_name,
            "sha256": hex::encode(Sha256::digest(bytes)),
            "compressedBytes": bytes.len(),
            "uncompressedBytes": actual_total,
            "fileCount": files.len()
        },
        "files": files
    }))
}

struct Directory<'a> {
    start: usize,
    eocd: usize,
    zip64: Option<usize>,
    archive_offset: u64,
    paths: Vec<&'a str>,
}

/// A borrowed constructor view that hides the ZIP comment and guards end-record parsing.
/// Central metadata is byte-exact: marker-shaped CRC/extra-field bytes are never rewritten.
/// zip 2.6.1 seeks to a candidate, then FixedSizeBlock::parse reads its 22/56-byte header.
/// Rejecting that read prevents count allocation while leaving search windows unchanged.
struct MetadataReader<'a, 'b> {
    cursor: Cursor<&'a [u8]>,
    eocd: usize,
    zip64: Option<usize>,
    constructing: &'b Cell<bool>,
    seek_start: Option<usize>,
}

impl Read for MetadataReader<'_, '_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if !self.constructing.get() {
            return self.cursor.read(output);
        }
        let start = self.cursor.position() as usize;
        let from_seek = self.seek_start.take() == Some(start);
        let signature = self.cursor.get_ref().get(start..start + 4);
        if from_seek
            && ((output.len() == 22 && signature == Some(b"PK\x05\x06") && start != self.eocd)
                || (output.len() == 56
                    && signature == Some(b"PK\x06\x06")
                    && Some(start) != self.zip64))
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, INVALID_ZIP));
        }
        let remaining = (self.eocd + 22).saturating_sub(start);
        let allowed = output.len().min(remaining);
        let count = self.cursor.read(&mut output[..allowed])?;
        // The constructor sees an empty comment, without changing any central fields.
        for position in [self.eocd + 20, self.eocd + 21] {
            if (start..start + count).contains(&position) {
                output[position - start] = 0;
            }
        }
        Ok(count)
    }
}

impl Seek for MetadataReader<'_, '_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if !self.constructing.get() {
            return self.cursor.seek(position);
        }
        let end = (self.eocd + 22) as u64;
        let target = match position {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => end.checked_add_signed(offset),
            SeekFrom::Current(offset) => self.cursor.position().checked_add_signed(offset),
        }
        .filter(|&offset| offset <= end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, INVALID_ZIP))?;
        self.seek_start = matches!(position, SeekFrom::Start(_)).then_some(target as usize);
        self.cursor.set_position(target);
        Ok(target)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let data = bytes.get(offset..offset + 2).ok_or(INVALID_ZIP)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let data = bytes.get(offset..offset + 4).ok_or(INVALID_ZIP)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let data = bytes.get(offset..offset + 8).ok_or(INVALID_ZIP)?;
    Ok(u64::from_le_bytes(
        data.try_into().map_err(|_| INVALID_ZIP)?,
    ))
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Validates the original central directory, including records hidden by a name-indexed reader.
fn preflight_directory(bytes: &[u8]) -> Result<Directory<'_>, String> {
    let end = (bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&offset| {
            bytes.get(offset..offset + 4) == Some(b"PK\x05\x06")
                && read_u16(bytes, offset + 20)
                    .is_ok_and(|len| offset + 22 + len as usize == bytes.len())
        })
        .ok_or(INVALID_ZIP)?;
    if read_u16(bytes, end + 4)? != 0 || read_u16(bytes, end + 6)? != 0 {
        return Err(INVALID_ZIP.into());
    }
    let has_zip64 = end >= 20 && bytes.get(end - 20..end - 16) == Some(b"PK\x06\x07");
    // Match zip 2's may_be_zip64 decision exactly; otherwise its classic count could
    // disagree with the ZIP64 count constrained by this preflight.
    let zip64_hint =
        read_u16(bytes, end + 10)? == u16::MAX || read_u32(bytes, end + 16)? == u32::MAX;
    if has_zip64 != zip64_hint {
        return Err(INVALID_ZIP.into());
    }
    let (central_end, count, central_size, relative_start, zip64) = if has_zip64 {
        let locator = end - 20;
        if read_u32(bytes, locator + 4)? != 0 || read_u32(bytes, locator + 16)? != 1 {
            return Err(INVALID_ZIP.into());
        }
        let record = (0..locator.saturating_sub(55))
            .rev()
            .find(|&offset| {
                bytes.get(offset..offset + 4) == Some(b"PK\x06\x06")
                    && read_u64(bytes, offset + 4).is_ok_and(|size| {
                        size >= 44
                            && (offset as u64)
                                .checked_add(12)
                                .and_then(|n| n.checked_add(size))
                                == Some(locator as u64)
                    })
            })
            .ok_or(INVALID_ZIP)?;
        if read_u32(bytes, record + 16)? != 0
            || read_u32(bytes, record + 20)? != 0
            || read_u64(bytes, record + 24)? != read_u64(bytes, record + 32)?
            || read_u64(bytes, locator + 8)? > record as u64
        {
            return Err(INVALID_ZIP.into());
        }
        (
            record,
            read_u64(bytes, record + 32)?,
            read_u64(bytes, record + 40)?,
            read_u64(bytes, record + 48)?,
            Some(record),
        )
    } else {
        if read_u16(bytes, end + 8)? != read_u16(bytes, end + 10)? {
            return Err(INVALID_ZIP.into());
        }
        (
            end,
            read_u16(bytes, end + 10)? as u64,
            read_u32(bytes, end + 12)? as u64,
            read_u32(bytes, end + 16)? as u64,
            None,
        )
    };
    if count > MAX_ENTRIES as u64 {
        return Err("ZIP条目超过110个限制".into());
    }
    let central_size = usize::try_from(central_size).map_err(|_| INVALID_ZIP)?;
    let central_start = central_end.checked_sub(central_size).ok_or(INVALID_ZIP)?;
    if relative_start > central_start as u64 {
        return Err(INVALID_ZIP.into());
    }
    let archive_offset = central_start as u64 - relative_start;
    if zip64.is_some_and(|record| {
        read_u64(bytes, end - 12)
            .ok()
            .and_then(|relative| relative.checked_add(archive_offset))
            != Some(record as u64)
    }) {
        return Err(INVALID_ZIP.into());
    }
    let mut offset = central_start;
    let mut paths = Vec::with_capacity(count as usize);
    let mut seen = HashSet::with_capacity(count as usize);
    for _ in 0..count {
        if offset + 46 > central_end || bytes.get(offset..offset + 4) != Some(b"PK\x01\x02") {
            return Err(INVALID_ZIP.into());
        }
        let name_len = read_u16(bytes, offset + 28)? as usize;
        let next = offset
            + 46
            + name_len
            + read_u16(bytes, offset + 30)? as usize
            + read_u16(bytes, offset + 32)? as usize;
        if next > central_end || read_u16(bytes, offset + 34)? != 0 {
            return Err(INVALID_ZIP.into());
        }
        if read_u16(bytes, offset + 8)? & 0x41 != 0 {
            return Err("ZIP包含加密或不支持的压缩条目".into());
        }
        let attributes = read_u32(bytes, offset + 38)?;
        if attributes & 0x10 != 0 || !matches!((attributes >> 16) & 0o170000, 0 | 0o100000) {
            return Err("ZIP仅支持普通文本文件".into());
        }
        let path = std::str::from_utf8(&bytes[offset + 46..offset + 46 + name_len])
            .map_err(|_| UNSAFE_PATH)?;
        if !safe_path(path) {
            return Err(UNSAFE_PATH.into());
        }
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err("ZIP包含重复文件路径".into());
        }
        paths.push(path);
        offset = next;
    }
    if offset != central_end {
        return Err(INVALID_ZIP.into());
    }
    Ok(Directory {
        start: central_start,
        eocd: end,
        zip64,
        archive_offset,
        paths,
    })
}

fn validate_local_header<R: Read>(
    bytes: &[u8],
    central_start: usize,
    path: &str,
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<(), String> {
    let header = usize::try_from(entry.header_start()).map_err(|_| INVALID_ZIP)?;
    if header + 30 > central_start || bytes.get(header..header + 4) != Some(b"PK\x03\x04") {
        return Err(INVALID_ZIP.into());
    }
    let name_len = read_u16(bytes, header + 26)? as usize;
    let data_start = header + 30 + name_len + read_u16(bytes, header + 28)? as usize;
    if data_start > central_start
        || bytes.get(header + 30..header + 30 + name_len) != Some(path.as_bytes())
        || entry.data_start() != data_start as u64
        || (data_start as u64)
            .checked_add(entry.compressed_size())
            .is_none_or(|end| end > central_start as u64)
    {
        return Err(INVALID_ZIP.into());
    }
    let flags = read_u16(bytes, header + 6)?;
    let method = read_u16(bytes, header + 8)?;
    if flags & 0x41 != 0
        || !matches!(method, 0 | 8)
        || (method == 0) != (entry.compression() == zip::CompressionMethod::Stored)
    {
        return Err("ZIP包含加密或不支持的压缩条目".into());
    }
    // With a data descriptor these local fields may be placeholders; the central CRC is
    // still checked by the decompressor at EOF. ZIP64 size sentinels are handled by zip.
    if flags & 8 == 0 {
        let compressed = read_u32(bytes, header + 18)?;
        let expanded = read_u32(bytes, header + 22)?;
        if read_u32(bytes, header + 14)? != entry.crc32()
            || (compressed != u32::MAX && compressed as u64 != entry.compressed_size())
            || (expanded != u32::MAX && expanded as u64 != entry.size())
        {
            return Err(INVALID_ZIP.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn archive(files: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn central_offsets(bytes: &[u8]) -> Vec<usize> {
        bytes
            .windows(4)
            .enumerate()
            .filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i))
            .collect()
    }

    fn patch_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn patch_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn channel_entries<'a>(report: &'a str, second: &'a str) -> Vec<(&'a str, &'a [u8])> {
        vec![
            (report, b"{}"),
            (second, b"{}"),
            ("feed-manifest.json", b"{}"),
            (
                "local-operation-notice.txt",
                "合法\u{feff}原文\n".as_bytes(),
            ),
        ]
    }

    #[test]
    fn channel_zip_accepts_only_the_three_exact_sets_and_preserves_text() {
        for (report, second) in [
            ("comparison-report.json", "baseline-manifest.json"),
            ("simulation-report.json", "simulated-receipt.json"),
            ("receipt-check-report.json", "simulated-receipt.json"),
        ] {
            let entries = channel_entries(report, second);
            for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
                let bytes = archive(&entries, method);
                let decoded = decode_channel_package("/private/channel.ZIP", &bytes).unwrap();
                assert_eq!(decoded["source"]["fileName"], "channel.ZIP");
                assert_eq!(decoded["source"]["fileCount"], 4);
                assert_eq!(
                    decoded["source"]["sha256"],
                    hex::encode(Sha256::digest(&bytes))
                );
                assert_eq!(
                    decoded["files"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["path"] == "local-operation-notice.txt")
                        .unwrap()["content"],
                    "合法\u{feff}原文\n"
                );
            }
        }
    }

    #[test]
    fn channel_zip_rejects_mixed_extra_original_feed_and_unsafe_inputs() {
        let valid = channel_entries("comparison-report.json", "baseline-manifest.json");
        for entries in [
            valid[..3].to_vec(),
            [valid.clone(), vec![("extra.txt", b"private".as_slice())]].concat(),
            channel_entries("comparison-report.json", "simulated-receipt.json"),
            channel_entries("feed.json", "baseline-manifest.json"),
            channel_entries("Comparison-report.json", "baseline-manifest.json"),
            channel_entries("../comparison-report.json", "baseline-manifest.json"),
            vec![
                ("feed-manifest.json", b"{}".as_slice()),
                ("feed.xml", b"<xml/>".as_slice()),
                ("validation-report.json", b"{}".as_slice()),
                ("local-operation-notice.txt", b"notice".as_slice()),
            ],
        ] {
            assert!(decode_channel_package(
                "channel.zip",
                &archive(&entries, CompressionMethod::Stored)
            )
            .is_err());
        }
        for content in [b"\xff".as_slice(), b"private\0text".as_slice()] {
            let mut entries = valid.clone();
            entries[0].1 = content;
            assert!(decode_channel_package(
                "channel.zip",
                &archive(&entries, CompressionMethod::Stored)
            )
            .is_err());
        }
        let mut duplicate = archive(
            &[
                ("comparison-report.json", b"{}".as_slice()),
                ("comparison-report.jsox", b"{}".as_slice()),
                ("feed-manifest.json", b"{}".as_slice()),
                ("local-operation-notice.txt", b"notice".as_slice()),
            ],
            CompressionMethod::Stored,
        );
        let original_name = b"comparison-report.jsox";
        for offset in 0..=duplicate.len() - original_name.len() {
            if &duplicate[offset..offset + original_name.len()] == original_name {
                duplicate[offset + original_name.len() - 1] = b'n';
            }
        }
        assert!(decode_channel_package("channel.zip", &duplicate).is_err());
        let bytes = archive(&valid, CompressionMethod::Stored);
        let central = central_offsets(&bytes)[0];
        for (offset, replacement) in [(6, 1u16), (central + 8, 0x40), (8, 12), (central + 10, 12)] {
            let mut invalid = bytes.clone();
            patch_u16(&mut invalid, offset, replacement);
            assert!(decode_channel_package("channel.zip", &invalid).is_err());
        }
        let mut crc = bytes.clone();
        crc[30 + valid[0].0.len()] ^= 1;
        assert!(decode_channel_package("channel.zip", &crc).is_err());
        assert!(decode_channel_package("channel.zip", &bytes[..bytes.len() - 1]).is_err());
        assert!(decode_channel_package("private\n.zip", &bytes).is_err());
        assert!(decode_channel_package("channel.zip", &vec![0; MAX_INPUT + 1]).is_err());
        let mut symlink = bytes.clone();
        patch_u32(&mut symlink, central + 38, 0o120644 << 16);
        assert!(decode_channel_package("channel.zip", &symlink).is_err());
        let mut expansion = bytes;
        patch_u32(&mut expansion, central + 24, MAX_EXPANDED as u32 + 1);
        assert!(decode_channel_package("channel.zip", &expansion).is_err());
    }

    #[test]
    fn attribution_zip_exact_files_text_and_structure_policy() {
        let entries = [
            ("summary.json", b"{}".as_slice()),
            ("dataset.json", b"{}".as_slice()),
            ("input.json", b"{}".as_slice()),
            ("orders.csv", b"orders".as_slice()),
            ("sources.csv", b"sources".as_slice()),
            ("products.csv", b"products".as_slice()),
            ("weeks.csv", b"weeks".as_slice()),
            ("methodology.txt", "合法\u{feff}原文\n".as_bytes()),
            ("external-requirements.txt", b"requirements".as_slice()),
        ];
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let original = archive(&entries, method);
            let decoded = decode_attribution_package("attribution.zip", &original).unwrap();
            assert_eq!(decoded["source"]["fileCount"], 9);
            assert_eq!(decoded["source"]["compressedBytes"], original.len());
            assert_eq!(
                decoded["source"]["uncompressedBytes"],
                entries.iter().map(|(_, b)| b.len()).sum::<usize>()
            );
            assert_eq!(decoded["files"][3]["content"], "合法\u{feff}原文\n");
            assert!(decode_site_package("site.zip", &original).is_err());
            assert!(decode_experiment_package("experiment.zip", &original).is_err());
            let mut crc = original.clone();
            crc[42] ^= 1;
            assert!(decode_attribution_package("attribution.zip", &crc).is_err());
            let mut local = original.clone();
            local[30] = b'X';
            assert!(decode_attribution_package("attribution.zip", &local).is_err());
            let mut mismatch = original.clone();
            let central = central_offsets(&mismatch)[0];
            patch_u32(&mut mismatch, central + 24, 1);
            assert!(decode_attribution_package("attribution.zip", &mismatch).is_err());
            assert!(
                decode_attribution_package("attribution.zip", &original[..original.len() - 1])
                    .is_err()
            );
        }
        for files in [
            entries[..8].to_vec(),
            [entries.to_vec(), vec![("extra.txt", b"x".as_slice())]].concat(),
            vec![
                ("input.json", b"{}".as_slice()),
                ("README.txt", b"template".as_slice()),
            ],
        ] {
            assert!(decode_attribution_package(
                "attribution.zip",
                &archive(&files, CompressionMethod::Stored)
            )
            .is_err());
        }
        for (path, content) in [
            ("SUMMARY.json", b"{}".as_slice()),
            ("../summary.json", b"{}".as_slice()),
            ("summary.json", b"private\0text".as_slice()),
            ("summary.json", b"\xff".as_slice()),
        ] {
            let mut files = entries.to_vec();
            files[0] = (path, content);
            assert!(decode_attribution_package(
                "attribution.zip",
                &archive(&files, CompressionMethod::Stored)
            )
            .is_err());
        }
        let mut duplicate = entries.to_vec();
        duplicate[1] = ("xummary.json", b"{}".as_slice());
        let mut bytes = archive(&duplicate, CompressionMethod::Stored);
        for offset in 0..bytes.len() - 12 {
            if &bytes[offset..offset + 12] == b"xummary.json" {
                bytes[offset..offset + 12].copy_from_slice(b"summary.json");
            }
        }
        assert!(decode_attribution_package("attribution.zip", &bytes).is_err());
    }

    #[test]
    fn experiment_zip_exact_files_and_text_policy() {
        let content = "  合法\u{feff}原文\n".as_bytes();
        let entries = [
            ("report.md", b"report".as_slice()),
            ("evidence.json", content),
            ("observations.csv", b"csv".as_slice()),
        ];
        let original = archive(&entries, CompressionMethod::Stored);
        let decoded = decode_experiment_package("experiment.zip", &original).unwrap();
        assert_eq!(decoded["files"][0]["content"], "  合法\u{feff}原文\n");
        assert!(decode_site_package("site.zip", &original).is_err());
        for files in [
            entries[..2].to_vec(),
            [entries.to_vec(), vec![("extra.txt", b"x".as_slice())]].concat(),
            vec![("REPORT.md", b"x".as_slice()), entries[1], entries[2]],
            vec![("../report.md", b"x".as_slice()), entries[1], entries[2]],
            vec![
                ("report.md", b"private\0text".as_slice()),
                entries[1],
                entries[2],
            ],
            vec![("report.md", b"\xff".as_slice()), entries[1], entries[2]],
        ] {
            assert!(decode_experiment_package(
                "experiment.zip",
                &archive(&files, CompressionMethod::Stored)
            )
            .is_err());
        }
        // ZipWriter normally forbids duplicate names; patch equal-length names in both headers.
        for replacement in [b"report.md".as_slice(), b"REPORT.md".as_slice()] {
            let mut duplicate = archive(
                &[
                    ("report.md", b"a"),
                    ("xportx.md", b"b"),
                    ("observations.csv", b"c"),
                ],
                CompressionMethod::Stored,
            );
            for offset in 0..duplicate.len() - 9 {
                if &duplicate[offset..offset + 9] == b"xportx.md" {
                    duplicate[offset..offset + 9].copy_from_slice(replacement);
                }
            }
            assert!(decode_experiment_package("experiment.zip", &duplicate).is_err());
        }
        let mut crc = original.clone();
        crc[39] ^= 1;
        assert!(decode_experiment_package("experiment.zip", &crc).is_err());
        let mut local = original.clone();
        local[30] = b'X';
        assert!(decode_experiment_package("experiment.zip", &local).is_err());
        let mut mismatch = original.clone();
        let central = central_offsets(&mismatch)[0];
        patch_u32(&mut mismatch, central + 24, 1);
        assert!(decode_experiment_package("experiment.zip", &mismatch).is_err());
        assert!(
            decode_experiment_package("experiment.zip", &original[..original.len() - 1]).is_err()
        );
    }

    #[test]
    fn roundtrips_current_site_writer_with_chinese_escaped_content_and_sorted_paths() {
        let package = json!({"files": [
            {"path": "z.html", "content": "<h1>虚构 &amp; \"文本\"</h1>\n"},
            {"path": "data/catalog.json", "content": "{\"名称\":\"测试\\n产品\"}"}
        ]});
        let bytes = crate::files::site_zip(&package).unwrap();
        let decoded = decode_site_package("/private/example/交付包.ZIP", &bytes).unwrap();
        assert_eq!(
            decoded["files"],
            json!([package["files"][1], package["files"][0]])
        );
        assert_eq!(
            decoded["source"],
            json!({
                "fileName": "交付包.ZIP", "sha256": hex::encode(Sha256::digest(&bytes)),
                "compressedBytes": bytes.len(), "uncompressedBytes": 58, "fileCount": 2
            })
        );
        assert_eq!(decoded.as_object().unwrap().len(), 2);
    }

    #[test]
    fn accepts_stored_empty_text_and_safe_relative_paths() {
        let bytes = archive(&[("a_B-1.2/file.txt", b"")], CompressionMethod::Stored);
        let decoded = decode_site_package("C:\\exports\\site.zip", &bytes).unwrap();
        assert_eq!(decoded["source"]["fileName"], "site.zip");
        assert_eq!(decoded["source"]["uncompressedBytes"], 0);
        assert_eq!(
            decoded["files"],
            json!([{"path":"a_B-1.2/file.txt", "content":""}])
        );
    }

    #[test]
    fn rejects_invalid_input_and_invalid_file_names_without_leaking_them() {
        let bytes = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        for name in ["", "not-zip.txt", "private\n.zip", "dir/", "dir\\"] {
            assert!(decode_site_package(name, &bytes).is_err(), "{name:?}");
        }
        let too_long = format!("{}.zip", "中".repeat(252));
        assert!(decode_site_package(&too_long, &bytes).is_err());
        let valid_max = format!("{}.zip", "中".repeat(251));
        assert!(decode_site_package(&valid_max, &bytes).is_ok());
        for invalid in [b"".as_slice(), b"not a ZIP".as_slice()] {
            assert!(decode_site_package("site.zip", invalid).is_err());
        }
        assert!(decode_site_package("site.zip", &vec![0; 20 * 1024 * 1024 + 1]).is_err());
    }

    #[test]
    fn rejects_unsafe_and_non_ascii_paths() {
        for path in [
            "../x.txt",
            "/x.txt",
            "a\\b.txt",
            "a:b.txt",
            "a//b.txt",
            "./x.txt",
            "a/../x.txt",
            "a/./x.txt",
            "a\n.txt",
            "a\0.txt",
            "路径.txt",
            "a b.txt",
        ] {
            let bytes = archive(&[(path, b"ok")], CompressionMethod::Stored);
            assert!(decode_site_package("site.zip", &bytes).is_err(), "{path:?}");
        }
    }

    #[test]
    fn rejects_exact_duplicate_paths_even_when_zip_library_would_coalesce_them() {
        let mut bytes = archive(
            &[("a.txt", b"a"), ("b.txt", b"b")],
            CompressionMethod::Stored,
        );
        for offset in 0..bytes.len() - 5 {
            if &bytes[offset..offset + 5] == b"b.txt" {
                bytes[offset] = b'a';
            }
        }
        assert!(decode_site_package("site.zip", &bytes).is_err());
    }

    #[test]
    fn rejects_case_insensitive_duplicate_paths() {
        let bytes = archive(
            &[("A.txt", b"a"), ("a.txt", b"b")],
            CompressionMethod::Stored,
        );
        assert!(decode_site_package("site.zip", &bytes).is_err());
    }

    #[test]
    fn rejects_directory_symlink_and_special_file_entries() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_directory("dir", SimpleFileOptions::default())
            .unwrap();
        assert!(decode_site_package("site.zip", &writer.finish().unwrap().into_inner()).is_err());
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink("link.txt", "a.txt", SimpleFileOptions::default())
            .unwrap();
        assert!(decode_site_package("site.zip", &writer.finish().unwrap().into_inner()).is_err());
        for mode in [0o010644_u32, 0o020644, 0o060644, 0o140644, 0o040755] {
            let mut bytes = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
            let header = central_offsets(&bytes)[0];
            bytes[header + 5] = 3; // Unix host; external attributes carry the file type.
            patch_u32(&mut bytes, header + 38, mode << 16);
            assert!(decode_site_package("site.zip", &bytes).is_err(), "{mode:o}");
        }
    }

    #[test]
    fn rejects_encrypted_and_unsupported_compression_entries() {
        let original = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let header = central_offsets(&original)[0];
        let mut encrypted = original.clone();
        patch_u16(&mut encrypted, 6, 1);
        patch_u16(&mut encrypted, header + 8, 1);
        assert!(decode_site_package("site.zip", &encrypted).is_err());
        let mut unsupported = original;
        patch_u16(&mut unsupported, 8, 12);
        patch_u16(&mut unsupported, header + 10, 12);
        assert!(decode_site_package("site.zip", &unsupported).is_err());
    }

    #[test]
    fn rejects_invalid_utf8_name_and_invalid_or_non_text_contents() {
        let mut invalid_name = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let header = central_offsets(&invalid_name)[0];
        invalid_name[30] = 0xff;
        invalid_name[header + 46] = 0xff;
        assert!(decode_site_package("site.zip", &invalid_name).is_err());
        for content in [
            b"\xff".as_slice(),
            b"\xef\xbb\xbftext".as_slice(),
            b"private\0sentinel".as_slice(),
        ] {
            let bytes = archive(&[("a.txt", content)], CompressionMethod::Stored);
            let error = decode_site_package("site.zip", &bytes).unwrap_err();
            assert!(!error.contains("private"));
        }
    }

    #[test]
    fn rejects_crc_damage_truncation_and_declared_size_mismatch() {
        let original = archive(&[("a.txt", b"private-sentinel")], CompressionMethod::Stored);
        let header = central_offsets(&original)[0];
        let mut corrupt = original.clone();
        corrupt[35] ^= 1;
        assert!(decode_site_package("site.zip", &corrupt).is_err());
        for cut in [1, 12, original.len() / 2] {
            assert!(decode_site_package("site.zip", &original[..original.len() - cut]).is_err());
        }
        let mut mismatched = original;
        patch_u32(&mut mismatched, header + 24, 1);
        assert!(decode_site_package("site.zip", &mismatched).is_err());
    }

    #[test]
    fn bounds_raw_entry_count_and_accepts_exact_entry_limit() {
        let names: Vec<_> = (0..111).map(|n| format!("{n}.txt")).collect();
        let files: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), b"".as_slice()))
            .collect();
        let too_many = archive(&files, CompressionMethod::Stored);
        assert!(decode_site_package("site.zip", &too_many).is_err());
        let boundary = archive(&files[..110], CompressionMethod::Stored);
        assert_eq!(
            decode_site_package("site.zip", &boundary).unwrap()["source"]["fileCount"],
            110
        );
    }

    #[test]
    fn bounds_declared_total_expansion_and_accepts_exact_expanded_limit() {
        let text = vec![b'x'; 8 * 1024 * 1024];
        let boundary = archive(
            &[("a.txt", &text), ("b.txt", &text)],
            CompressionMethod::Deflated,
        );
        assert_eq!(
            decode_site_package("site.zip", &boundary).unwrap()["source"]["uncompressedBytes"],
            16 * 1024 * 1024
        );
        let mut oversized = boundary;
        let headers = central_offsets(&oversized);
        patch_u32(&mut oversized, headers[1] + 24, 8 * 1024 * 1024 + 1);
        assert!(decode_site_package("site.zip", &oversized).is_err());
    }

    #[test]
    fn rejects_actual_compressed_expansion_when_declared_size_lies() {
        let text = vec![b'x'; 16 * 1024 * 1024 + 1];
        let mut bytes = archive(&[("a.txt", &text)], CompressionMethod::Deflated);
        assert!(bytes.len() < 100_000);
        let header = central_offsets(&bytes)[0];
        patch_u32(&mut bytes, header + 24, 1);
        patch_u32(&mut bytes, 22, 1);
        assert!(decode_site_package("site.zip", &bytes).is_err());
    }

    #[test]
    fn accepts_empty_zip_for_domain_classification_and_rejects_forged_directory_counts() {
        let bytes = archive(&[], CompressionMethod::Stored);
        let decoded = decode_site_package("empty.zip", &bytes).unwrap();
        assert_eq!(decoded["files"], json!([]));
        assert_eq!(decoded["source"]["fileCount"], 0);
        let mut forged = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let end = forged.len() - 22;
        patch_u16(&mut forged, end + 8, 111);
        patch_u16(&mut forged, end + 10, 111);
        assert!(decode_site_package("site.zip", &forged).is_err());
    }

    #[test]
    fn supports_bounded_zip64_archives_without_large_fixture_allocations() {
        let original = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let end = original.len() - 22;
        let central = central_offsets(&original)[0];
        let mut bytes = original[..end].to_vec();
        let mut zip64_end = [0_u8; 56];
        zip64_end[..4].copy_from_slice(b"PK\x06\x06");
        zip64_end[4..12].copy_from_slice(&44_u64.to_le_bytes());
        zip64_end[12..14].copy_from_slice(&45_u16.to_le_bytes());
        zip64_end[14..16].copy_from_slice(&45_u16.to_le_bytes());
        zip64_end[24..32].copy_from_slice(&1_u64.to_le_bytes());
        zip64_end[32..40].copy_from_slice(&1_u64.to_le_bytes());
        zip64_end[40..48].copy_from_slice(&((end - central) as u64).to_le_bytes());
        zip64_end[48..56].copy_from_slice(&(central as u64).to_le_bytes());
        bytes.extend_from_slice(&zip64_end);
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&(end as u64).to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        let mut classic = original[end..].to_vec();
        patch_u16(&mut classic, 8, u16::MAX);
        patch_u16(&mut classic, 10, u16::MAX);
        patch_u32(&mut classic, 12, u32::MAX);
        patch_u32(&mut classic, 16, u32::MAX);
        bytes.extend_from_slice(&classic);
        assert_eq!(
            decode_site_package("site.zip", &bytes).unwrap()["files"],
            json!([{"path":"a.txt","content":"ok"}])
        );
        let classic_end = bytes.len() - 22;
        let mut divergent = bytes.clone();
        patch_u16(&mut divergent, classic_end + 8, 1);
        patch_u16(&mut divergent, classic_end + 10, 1);
        patch_u32(&mut divergent, classic_end + 16, central as u32);
        assert!(preflight_directory(&divergent).is_err());
        let mut wrong_locator = bytes;
        wrong_locator[classic_end - 12..classic_end - 4]
            .copy_from_slice(&((end + 1) as u64).to_le_bytes());
        assert!(preflight_directory(&wrong_locator).is_err());
    }

    #[test]
    fn rejects_ambiguous_local_names_and_corrupt_empty_file_crc() {
        let mut local_name = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        local_name[30] = 0xff;
        assert!(decode_site_package("site.zip", &local_name).is_err());
        let mut empty_crc = archive(&[("a.txt", b"")], CompressionMethod::Stored);
        let header = central_offsets(&empty_crc)[0];
        patch_u32(&mut empty_crc, 14, 1);
        patch_u32(&mut empty_crc, header + 16, 1);
        assert!(decode_site_package("site.zip", &empty_crc).is_err());
    }

    #[test]
    fn rejects_central_strong_encryption_and_special_modes_from_other_zip_hosts() {
        let original = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let header = central_offsets(&original)[0];
        let mut strong_encryption = original.clone();
        patch_u16(&mut strong_encryption, header + 8, 0x40);
        assert!(decode_site_package("site.zip", &strong_encryption).is_err());
        let mut other_host_symlink = original;
        other_host_symlink[header + 5] = 19; // macOS host code, not normalized as Unix by zip 2.
        patch_u32(&mut other_host_symlink, header + 38, 0o120644 << 16);
        assert!(decode_site_package("site.zip", &other_host_symlink).is_err());
    }

    #[test]
    fn bounds_original_zip_bytes_even_when_expanded_text_is_tiny() {
        let zip = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let mut boundary = vec![b'X'; 20 * 1024 * 1024 - zip.len()];
        boundary.extend_from_slice(&zip);
        assert_eq!(
            decode_site_package("site.zip", &boundary).unwrap()["source"]["compressedBytes"],
            20 * 1024 * 1024
        );
        boundary.insert(0, b'X');
        assert!(decode_site_package("site.zip", &boundary).is_err());
    }

    #[test]
    fn uses_preflight_eocd_instead_of_another_candidate_inside_a_valid_comment() {
        let mut bytes = archive(&[("a.txt", b"ok")], CompressionMethod::Stored);
        let end = bytes.len() - 22;
        patch_u16(&mut bytes, end + 20, 23);
        let mut fake = [0_u8; 22];
        fake[..4].copy_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&fake);
        bytes.push(b'X'); // The fake candidate comment ends before EOF, accepted by zip 2.
        assert_eq!(preflight_directory(&bytes).unwrap().paths.len(), 1);
        let decoded = decode_site_package("site.zip", &bytes).unwrap();
        assert_eq!(decoded["files"], json!([{"path":"a.txt","content":"ok"}]));
        assert_eq!(
            decoded["source"]["sha256"],
            hex::encode(Sha256::digest(&bytes))
        );
    }

    #[test]
    fn constructor_preserves_metadata_and_blocks_unselected_candidate_header_reads() {
        let mut bytes = vec![b'a'; 230];
        for (position, marker) in [
            (0, b"PK\x05\x06"),
            (60, b"PK\x06\x06"),
            (120, b"PK\x06\x06"),
            (198, b"PK\x05\x06"),
        ] {
            bytes[position..position + 4].copy_from_slice(marker);
        }
        patch_u16(&mut bytes, 218, 10);
        for chunk_size in [1, 2, 3, 4, 7, 32] {
            let constructing = Cell::new(true);
            let mut reader = MetadataReader {
                cursor: Cursor::new(bytes.as_slice()),
                eocd: 198,
                zip64: Some(120),
                constructing: &constructing,
                seek_start: None,
            };
            let mut visible = Vec::new();
            loop {
                let mut chunk = vec![0; chunk_size];
                let count = reader.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                visible.extend_from_slice(&chunk[..count]);
            }
            let mut expected = bytes[..220].to_vec();
            patch_u16(&mut expected, 218, 0);
            assert_eq!(visible, expected); // CRC and extra-field marker bytes are untouched.
            for (candidate, header_len) in [(0, 22), (60, 56)] {
                reader.seek(SeekFrom::Start(candidate)).unwrap();
                assert!(reader.read_exact(&mut vec![0; header_len]).is_err());
            }
            reader.seek(SeekFrom::Start(120)).unwrap();
            reader.read_exact(&mut [0; 56]).unwrap();
            reader.seek(SeekFrom::Start(198)).unwrap();
            reader.read_exact(&mut [0; 22]).unwrap();
            constructing.set(false);
            reader.seek(SeekFrom::Start(0)).unwrap();
            let mut restored = Vec::new();
            reader.read_to_end(&mut restored).unwrap();
            assert_eq!(restored, bytes);
        }
    }

    #[test]
    fn preserves_valid_central_crc_bytes_that_look_like_end_record_signatures() {
        for (content, marker) in [
            ("fictional-crc-27-Nw~r", b"PK\x05\x06"),
            ("fictional-crc-98-seMf", b"PK\x06\x06"),
        ] {
            let bytes = archive(&[("a.txt", content.as_bytes())], CompressionMethod::Stored);
            let central = central_offsets(&bytes)[0];
            assert_eq!(&bytes[central + 16..central + 20], marker);
            let decoded = decode_site_package("site.zip", &bytes).unwrap();
            assert_eq!(decoded["files"][0]["content"], content);
            assert_eq!(
                decoded["source"]["sha256"],
                hex::encode(Sha256::digest(&bytes))
            );
        }
    }
}
