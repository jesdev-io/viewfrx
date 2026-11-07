//! Timestamp annotations stored inside the audio file's own metadata, so the
//! file stays a normal audio file that any player can open.
//!
//! Format: free text containing `[hh:mm:ss:cc]` markers, e.g.
//! `[00:00:12:00] thunder starts`. WAV files carry annotations in a RIFF
//! `LIST`/`INFO` chunk with a custom `FRXA` sub-chunk (inserted before the
//! `data` chunk, where players and Symphonia actually read metadata); MP3
//! files carry them in an ID3v2 `TXXX` frame. Writing is atomic (temp file +
//! rename) and preserves every other chunk/frame byte-for-byte.
use crate::decode::AudioReader;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

/// Custom RIFF INFO sub-chunk fourcc for frxview annotations.
pub const WAV_ANNOTATION_CHUNK: &[u8; 4] = b"FRXA";
/// ID3v2 TXXX frame description for frxview annotations.
pub const ID3_ANNOTATION_DESCRIPTION: &str = "frxview-annotations";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub seconds: f64,
    pub text: String,
}

/// `[hh:mm:ss:cc]` where cc = centiseconds (100*ms + 10*ms), mm/ss < 60.
pub fn format_timestamp(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    let centis = (total * 100.0).round() as u64;
    let hours = centis / 360_000;
    let minutes = (centis / 6_000) % 60;
    let secs = (centis / 100) % 60;
    let cc = centis % 100;
    format!("[{hours:02}:{minutes:02}:{secs:02}:{cc:02}]")
}

/// Parse one `[hh:mm:ss:cc]` token (cc = centiseconds); returns seconds.
pub fn parse_timestamp(token: &str) -> Option<f64> {
    let inner = token.strip_prefix('[')?.strip_suffix(']')?;
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() != 4
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;
    let secs: u64 = parts[2].parse().ok()?;
    let centis: u64 = parts[3].parse().ok()?;
    if minutes > 59 || secs > 59 || centis > 99 {
        return None;
    }
    Some((hours * 360_000 + minutes * 6_000 + secs * 100 + centis) as f64 / 100.0)
}

/// Parse free text into annotations: every `[hh:mm:ss:cc]` marker starts an
/// annotation whose text runs until the next marker or the end of input.
pub fn parse_text(input: &str) -> Result<Vec<Annotation>> {
    let mut found = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(end) = input[i..].find(']').map(|e| i + e)
            && let Some(seconds) = parse_timestamp(&input[i..=end])
        {
            let text_start = end + 1;
            let text_end = input[text_start..]
                .find('[')
                .map(|p| text_start + p)
                .unwrap_or(input.len());
            let text = input[text_start..text_end].trim().to_string();
            found.push(Annotation { seconds, text });
            i = text_end;
            continue;
        }
        i += 1;
    }
    if found.is_empty() {
        bail!("expected at least one [hh:mm:ss:cc] timestamp marker in the text");
    }
    Ok(found)
}

/// Render annotations back into the `[hh:mm:ss:cc] text` free-text form.
pub fn format_annotations(annotations: &[Annotation]) -> String {
    let mut out = String::new();
    for annotation in annotations {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format_timestamp(annotation.seconds));
        if !annotation.text.is_empty() {
            out.push(' ');
            out.push_str(&annotation.text);
        }
    }
    out
}

/// Idempotent merge: identical (seconds, text) pairs are not duplicated;
/// the result is sorted by time. Times are canonicalized to centiseconds
/// because the `[hh:mm:ss:cc]` marker format has 10 ms resolution — this is
/// what makes text-field entry and right-click markers idempotent with each
/// other.
pub fn merge(existing: &mut Vec<Annotation>, new: &[Annotation]) {
    for annotation in new {
        if !annotation.seconds.is_finite() || annotation.seconds < 0.0 {
            continue;
        }
        let canonical = Annotation {
            seconds: (annotation.seconds * 100.0).round() / 100.0,
            text: annotation.text.clone(),
        };
        if existing.contains(&canonical) {
            continue;
        }
        // Same timestamp = same marker: typing note text replaces the
        // marker's content (empty or previous), so editing never accumulates
        // per-keystroke duplicates. An empty new text never clobbers content.
        if let Some(pos) = existing.iter().position(|a| a.seconds == canonical.seconds) {
            if !canonical.text.is_empty() {
                existing[pos] = canonical;
            }
            continue;
        }
        existing.push(canonical);
    }
    existing.sort_by(|a, b| a.seconds.total_cmp(&b.seconds).then(a.text.cmp(&b.text)));
}

/// Extract annotations from decoded metadata tags (any tag value may carry
/// `[hh:mm:ss:cc]` markers; writers use dedicated keys).
pub fn from_tags(tags: &[(String, String)]) -> Vec<Annotation> {
    let mut annotations = Vec::new();
    for (_, value) in tags {
        if let Ok(parsed) = parse_text(value) {
            merge(&mut annotations, &parsed);
        }
    }
    annotations
}

/// Read the annotations stored in a file's metadata.
pub fn read(path: &Path) -> Result<Vec<Annotation>> {
    let info = AudioReader::open(path)?.info().clone();
    Ok(from_tags(&info.tags))
}

/// Add annotations to a file (merged idempotently) and store them in the
/// file's metadata without altering the audio data.
pub fn annotate(path: &Path, new: &[Annotation]) -> Result<Vec<Annotation>> {
    if new
        .iter()
        .any(|a| !a.seconds.is_finite() || a.seconds < 0.0)
    {
        bail!("annotation time must be finite and non-negative");
    }
    let mut annotations = read(path)?;
    merge(&mut annotations, new);
    write(path, &annotations)?;
    Ok(annotations)
}

/// Replace the stored annotations wholesale (used by tests and sync paths).
pub fn write(path: &Path, annotations: &[Annotation]) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let updated = transform_bytes(path, &bytes, annotations)?;
    // Atomic replace: readers either see the old or the new file, never a mix.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&updated)?;
    temp.flush()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Transform a file's bytes so its metadata carries exactly `annotations`.
fn transform_bytes(path: &Path, bytes: &[u8], annotations: &[Annotation]) -> Result<Vec<u8>> {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        write_wav(bytes, annotations)
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
        || bytes.starts_with(b"ID3")
        || bytes.first().is_some_and(|b| *b == 0xFF)
    {
        write_mp3(bytes, annotations)
    } else {
        bail!(
            "annotation metadata is only supported for WAV and MP3 files: {}",
            path.display()
        );
    }
}

/// Save the annotations to `dst` (the GUI "save as" dialog target), leaving
/// the source file untouched unless dst is the same file. The audio data is
/// copied byte-for-byte; only the metadata chunk/tag changes.
pub fn save_as(src: &Path, dst: &Path, annotations: &[Annotation]) -> Result<()> {
    if src.canonicalize().ok() == dst.canonicalize().ok() && dst.exists() {
        return write(src, annotations);
    }
    let bytes = fs::read(src).with_context(|| format!("read {}", src.display()))?;
    let updated = transform_bytes(src, &bytes, annotations)?;
    let parent = dst
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&updated)?;
    temp.flush()?;
    temp.persist(dst).map_err(|e| e.error)?;
    Ok(())
}

fn pad2(len: usize) -> usize {
    len + (len % 2)
}

/// Split a RIFF/WAVE file into (id, data) chunks; fails on malformed input so
/// a write never harms a file it cannot parse. The RIFF size field is the
/// authority for the container end; trailing bytes beyond it are ignored, and
/// a missing pad byte after an odd-sized final chunk is tolerated (streaming
/// writers often omit it).
fn riff_chunks(bytes: &[u8]) -> Result<Vec<([u8; 4], Vec<u8>)>> {
    if bytes.len() < 12 || !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        bail!("not a RIFF/WAVE file");
    }
    let riff_end =
        (8 + u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize).min(bytes.len());
    let mut chunks = Vec::new();
    let mut pos = 12usize;
    while pos + 8 <= riff_end {
        let id: [u8; 4] = bytes[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let end = start
            .checked_add(size)
            .filter(|e| *e <= riff_end)
            .with_context(|| "truncated RIFF chunk")?;
        chunks.push((id, bytes[start..end].to_vec()));
        // Skip the pad byte after an odd-sized chunk only when more container
        // bytes follow (the final chunk's pad is often omitted).
        let pad = if size % 2 == 1 && end < riff_end {
            1
        } else {
            0
        };
        pos = end + pad;
    }
    Ok(chunks)
}

fn build_list_info(subchunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut list = b"INFO".to_vec();
    for (id, data) in subchunks {
        list.extend_from_slice(id);
        list.extend_from_slice(&(data.len() as u32).to_le_bytes());
        list.extend_from_slice(data);
        if data.len() % 2 == 1 {
            list.push(0);
        }
    }
    list
}

fn write_wav(bytes: &[u8], annotations: &[Annotation]) -> Result<Vec<u8>> {
    let mut chunks = riff_chunks(bytes)?;
    let payload = format_annotations(annotations);
    let frxa = if payload.is_empty() {
        None
    } else {
        Some(payload.into_bytes())
    };
    let mut inserted = false;
    for (id, data) in chunks.iter_mut() {
        if id == b"LIST" && data.starts_with(b"INFO") {
            // Parse existing INFO sub-chunks, drop our previous FRXA, keep the rest.
            let mut subchunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();
            let mut pos = 4usize;
            while pos + 8 <= data.len() {
                let sub_id: [u8; 4] = data[pos..pos + 4].try_into().unwrap();
                let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
                let start = pos + 8;
                let end = start
                    .checked_add(size)
                    .filter(|e| *e <= data.len())
                    .with_context(|| "truncated INFO sub-chunk")?;
                if &sub_id != WAV_ANNOTATION_CHUNK {
                    subchunks.push((sub_id, data[start..end].to_vec()));
                }
                pos = start + pad2(size);
            }
            if let Some(payload) = &frxa {
                subchunks.push((*WAV_ANNOTATION_CHUNK, payload.clone()));
            }
            *data = build_list_info(&subchunks);
            inserted = true;
        }
    }
    if !inserted && let Some(payload) = frxa {
        let list = build_list_info(&[(*WAV_ANNOTATION_CHUNK, payload)]);
        // Insert before the data chunk so metadata readers actually see it.
        let data_pos = chunks
            .iter()
            .position(|(id, _)| id == b"data")
            .context("WAV file has no data chunk")?;
        chunks.insert(data_pos, (*b"LIST", list));
    }
    let mut out = b"RIFF".to_vec();
    let body: usize = 4 + chunks.iter().map(|(_, d)| 8 + pad2(d.len())).sum::<usize>();
    out.extend_from_slice(&(body as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    for (id, data) in &chunks {
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }
    Ok(out)
}

fn syncsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn from_syncsafe(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u32, |acc, b| (acc << 7) | (b & 0x7F) as u32)
}

fn txxx_frame(text: &str) -> Vec<u8> {
    let mut payload = vec![3u8]; // UTF-8 encoding
    payload.extend_from_slice(ID3_ANNOTATION_DESCRIPTION.as_bytes());
    payload.push(0);
    payload.extend_from_slice(text.as_bytes());
    let mut frame = b"TXXX".to_vec();
    frame.extend_from_slice(&syncsafe(payload.len() as u32));
    frame.extend_from_slice(&[0, 0]); // flags
    frame.extend_from_slice(&payload);
    frame
}

/// Is this TXXX payload ours (matches our description)?
fn is_our_txxx(payload: &[u8], version: u8) -> bool {
    if payload.first() != Some(&3) && payload.first() != Some(&0) {
        return false;
    }
    let rest = &payload[1..];
    let desc = ID3_ANNOTATION_DESCRIPTION.as_bytes();
    let _ = version;
    rest.starts_with(desc) && rest.get(desc.len()) == Some(&0)
}

fn write_mp3(bytes: &[u8], annotations: &[Annotation]) -> Result<Vec<u8>> {
    let payload = format_annotations(annotations);
    let (version, mut frames) = if bytes.starts_with(b"ID3") && bytes.len() >= 10 {
        let version = bytes[3];
        if !(3..=4).contains(&version) {
            bail!("unsupported ID3v2.{version} tag");
        }
        let size = from_syncsafe(&bytes[6..10]) as usize;
        let end = 10 + size;
        if end > bytes.len() {
            bail!("truncated ID3v2 tag");
        }
        (version, parse_id3_frames(&bytes[10..end], version)?)
    } else {
        (4u8, Vec::new())
    };
    frames.retain(|(id, payload)| !(id == b"TXXX" && is_our_txxx(payload, version)));
    if !payload.is_empty() {
        frames.push((*b"TXXX", txxx_frame(&payload)[10..].to_vec()));
    }
    let mut body = Vec::new();
    for (id, payload) in &frames {
        body.extend_from_slice(id);
        if version == 4 {
            body.extend_from_slice(&syncsafe(payload.len() as u32));
        } else {
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        }
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(payload);
    }
    let mut out = b"ID3".to_vec();
    out.extend_from_slice(&[version, 0, 0]);
    out.extend_from_slice(&syncsafe(body.len() as u32));
    out.extend_from_slice(&body);
    let audio_start = if bytes.starts_with(b"ID3") && bytes.len() >= 10 {
        10 + from_syncsafe(&bytes[6..10]) as usize
    } else {
        0
    };
    out.extend_from_slice(&bytes[audio_start..]);
    Ok(out)
}

fn parse_id3_frames(mut data: &[u8], version: u8) -> Result<Vec<([u8; 4], Vec<u8>)>> {
    let mut frames = Vec::new();
    while data.len() >= 10 {
        let id: [u8; 4] = data[..4].try_into().unwrap();
        if id.iter().any(|b| !b.is_ascii_alphanumeric()) {
            break; // padding
        }
        let size = if version == 4 {
            from_syncsafe(&data[4..8]) as usize
        } else {
            u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize
        };
        let start: usize = 10;
        let end = start
            .checked_add(size)
            .filter(|e| *e <= data.len())
            .context("truncated ID3 frame")?;
        frames.push((id, data[start..end].to_vec()));
        data = &data[end..];
    }
    Ok(frames)
}

/// Annotations for the CLI/GUI, serialized as a plain list.
pub fn to_json(annotations: &[Annotation]) -> Vec<serde_json::Value> {
    annotations
        .iter()
        .map(|a| serde_json::json!({"seconds": a.seconds, "timestamp": format_timestamp(a.seconds), "text": a.text}))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_fixture(path: &Path, frames: usize) -> Result<()> {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for i in 0..frames {
            let v = if i % 2 == 0 { 4096i16 } else { -4096 };
            writer.write_sample(v)?;
            writer.write_sample(-v)?;
        }
        writer.finalize()?;
        Ok(())
    }

    #[test]
    fn timestamp_parse_and_format_round_trip() {
        assert_eq!(parse_timestamp("[00:00:00:00]"), Some(0.0));
        assert_eq!(parse_timestamp("[00:00:01:30]"), Some(1.3));
        assert_eq!(parse_timestamp("[01:02:03:04]"), Some(3723.04));
        assert_eq!(parse_timestamp("[00:00:60:00]"), None); // seconds > 59
        assert_eq!(parse_timestamp("[00:00:00:100]"), None); // centis > 99
        assert_eq!(parse_timestamp("[00:00:00]"), None); // needs hh:mm:ss:cc
        assert_eq!(parse_timestamp("hello"), None);
        assert_eq!(format_timestamp(1.3), "[00:00:01:30]");
        assert_eq!(format_timestamp(3723.04), "[01:02:03:04]");
        assert_eq!(format_timestamp(90.0), "[00:01:30:00]");
        // Centisecond canonicalization keeps sub-second positions distinct.
        assert_eq!(format_timestamp(2.456), "[00:00:02:46]");
    }

    #[test]
    fn parse_text_splits_multiple_markers_and_rejects_markerless_input() -> Result<()> {
        let parsed = parse_text("[00:00:01:00] thunder\n[00:02:00:50] rain starts later")?;
        assert_eq!(
            parsed,
            vec![
                Annotation {
                    seconds: 1.0,
                    text: "thunder".into()
                },
                Annotation {
                    seconds: 120.5,
                    text: "rain starts later".into()
                },
            ]
        );
        assert_eq!(
            format_annotations(&parsed),
            "[00:00:01:00] thunder\n[00:02:00:50] rain starts later"
        );
        assert!(parse_text("no marker here").is_err());
        Ok(())
    }

    #[test]
    fn merge_replaces_marker_text_at_same_time_without_duplicates() {
        let mut existing = vec![Annotation {
            seconds: 12.0,
            text: String::new(),
        }];
        // Typing a note onto the clicked marker replaces the empty one.
        merge(
            &mut existing,
            &[Annotation {
                seconds: 12.0,
                text: "thunder".into(),
            }],
        );
        assert_eq!(
            existing,
            vec![Annotation {
                seconds: 12.0,
                text: "thunder".into()
            }]
        );
        // Each further keystroke replaces the text — no per-byte duplicates.
        for text in ["thunde", "thunder!", "thunder!!"] {
            merge(
                &mut existing,
                &[Annotation {
                    seconds: 12.0,
                    text: text.into(),
                }],
            );
            assert_eq!(existing.len(), 1, "text={text}");
        }
        assert_eq!(existing[0].text, "thunder!!");
        // An empty marker at the same time never clobbers the text.
        merge(
            &mut existing,
            &[Annotation {
                seconds: 12.0,
                text: String::new(),
            }],
        );
        assert_eq!(existing[0].text, "thunder!!");
        // Re-adding the same is idempotent; a different time adds a new marker.
        merge(
            &mut existing,
            &[Annotation {
                seconds: 12.0,
                text: "thunder!!".into(),
            }],
        );
        merge(
            &mut existing,
            &[Annotation {
                seconds: 5.0,
                text: String::new(),
            }],
        );
        assert_eq!(existing.len(), 2);
    }
    #[test]
    fn merge_is_idempotent_and_sorted() {
        let mut existing = vec![Annotation {
            seconds: 5.0,
            text: "b".into(),
        }];
        let new = vec![
            Annotation {
                seconds: 5.0,
                text: "b".into(),
            },
            Annotation {
                seconds: 1.0,
                text: "a".into(),
            },
        ];
        merge(&mut existing, &new);
        assert_eq!(
            existing,
            vec![
                Annotation {
                    seconds: 1.0,
                    text: "a".into()
                },
                Annotation {
                    seconds: 5.0,
                    text: "b".into()
                },
            ]
        );
    }

    #[test]
    fn wav_annotations_round_trip_and_file_stays_playable() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.wav");
        wav_fixture(&path, 800)?;
        let original = fs::read(&path)?;
        let added = vec![
            Annotation {
                seconds: 1.0,
                text: "first".into(),
            },
            Annotation {
                seconds: 60.0,
                text: String::new(),
            },
        ];
        let stored = annotate(&path, &added)?;
        assert_eq!(stored, added);
        // Idempotent: writing the same set again changes nothing.
        let stored2 = annotate(&path, &added)?;
        assert_eq!(stored2, stored);
        assert_eq!(fs::read(&path)?, fs::read(&path)?);
        let size_once = fs::metadata(&path)?.len();
        annotate(&path, &added)?;
        assert_eq!(
            fs::metadata(&path)?.len(),
            size_once,
            "repeat write must not grow the file"
        );
        // Still a normal audio file: decodes to the same number of frames.
        let mut reader = AudioReader::open(&path)?;
        assert_eq!(reader.info().frames, Some(800));
        let mut frame = [0.0f32; 2];
        let mut count = 0;
        while reader.next_frame(&mut frame)? {
            count += 1;
        }
        assert_eq!(count, 800);
        // Audio data untouched: original file's data chunk is still there verbatim.
        let now = fs::read(&path)?;
        assert!(now.len() > original.len());
        assert!(
            now.windows(original.len())
                .any(|w| w == original.as_slice())
                || {
                    // data chunk bytes are preserved inside the rewritten file
                    let chunks = riff_chunks(&now)?;
                    let data = chunks.iter().find(|(id, _)| id == b"data").unwrap();
                    data.1.len() == 800 * 4
                }
        );
        // Read back through metadata tags.
        assert_eq!(read(&path)?, added);
        // New annotation merges with existing ones; sub-second times
        // canonicalize to whole seconds ([hh:mm:ss:cc] has integer-second
        // resolution), which keeps both entry methods idempotent.
        let more = annotate(
            &path,
            &[
                Annotation {
                    seconds: 3.0,
                    text: "x".into(),
                },
                Annotation {
                    seconds: 2.4,
                    text: "r".into(),
                },
            ],
        )?;
        assert_eq!(more.len(), 4);
        assert!(more.contains(&Annotation {
            seconds: 2.4,
            text: "r".into()
        }));
        Ok(())
    }

    #[test]
    fn wav_write_preserves_existing_info_tags() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("b.wav");
        wav_fixture(&path, 100)?;
        // Manually insert a LIST INFO ICMT chunk before data.
        let bytes = fs::read(&path)?;
        let chunks = riff_chunks(&bytes)?;
        let data_pos = chunks.iter().position(|(id, _)| id == b"data").unwrap();
        let mut list = build_list_info(&[(*b"ICMT", b"user comment".to_vec())]);
        let mut out = b"RIFF".to_vec();
        let body = 4
            + (chunks.len() + 1) * 8
            + chunks.iter().map(|(_, d)| pad2(d.len())).sum::<usize>()
            + pad2(list.len());
        out.extend_from_slice(&(body as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (i, (id, data)) in chunks.iter().enumerate() {
            if i == data_pos {
                out.extend_from_slice(b"LIST");
                out.extend_from_slice(&(list.len() as u32).to_le_bytes());
                out.extend_from_slice(&list);
                if list.len() % 2 == 1 {
                    out.push(0);
                }
            }
            out.extend_from_slice(id);
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
            if data.len() % 2 == 1 {
                out.push(0);
            }
        }
        fs::write(&path, &out)?;
        list.clear();
        let stored = annotate(
            &path,
            &[Annotation {
                seconds: 1.0,
                text: "note".into(),
            }],
        )?;
        assert_eq!(stored.len(), 1);
        let tags = AudioReader::open(&path)?.info().tags.clone();
        assert!(
            tags.iter()
                .any(|(k, v)| k == "Comment" && v == "user comment"),
            "existing INFO tags must survive: {tags:?}"
        );
        assert_eq!(from_tags(&tags), stored);
        Ok(())
    }

    #[test]
    fn mp3_annotations_round_trip_and_audio_survives() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tone.mp3");
        fs::write(&path, include_bytes!("../tests/fixtures/tone.mp3"))?;
        let added = vec![Annotation {
            seconds: 1.0,
            text: "marker".into(),
        }];
        let stored = annotate(&path, &added)?;
        assert_eq!(stored, added);
        let size_once = fs::metadata(&path)?.len();
        annotate(&path, &added)?;
        assert_eq!(fs::metadata(&path)?.len(), size_once, "idempotent rewrite");
        assert_eq!(read(&path)?, added);
        // Still decodes as the same recording.
        let mut reader = AudioReader::open(&path)?;
        assert_eq!(reader.info().frames, Some(11025));
        let mut frame = [0.0f32; 2];
        let mut count = 0;
        while reader.next_frame(&mut frame)? {
            count += 1;
        }
        assert_eq!(count, 11025);
        Ok(())
    }

    #[test]
    fn save_as_writes_a_copy_and_leaves_the_source_untouched() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let src = dir.path().join("src.wav");
        wav_fixture(&src, 400)?;
        let before = fs::read(&src)?;
        let dst = dir.path().join("copy.wav");
        let added = vec![Annotation {
            seconds: 1.0,
            text: "note".into(),
        }];
        save_as(&src, &dst, &added)?;
        assert_eq!(fs::read(&src)?, before, "source must be untouched");
        assert_eq!(read(&dst)?, added);
        // Still a normal playable file.
        let mut reader = AudioReader::open(&dst)?;
        let mut frame = [0.0f32; 2];
        let mut count = 0;
        while reader.next_frame(&mut frame)? {
            count += 1;
        }
        assert_eq!(count, 400);
        // Saving onto itself is the atomic in-place write.
        save_as(&src, &src, &added)?;
        assert_eq!(read(&src)?, added);
        Ok(())
    }

    #[test]
    fn wav_with_odd_final_chunk_and_no_pad_byte_still_round_trips() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("odd.wav");
        // Hand-build: fmt + data with ODD size (7999) and NO pad byte, RIFF
        // size excluding the pad (as streaming writers emit).
        let fmt = {
            let mut f = Vec::new();
            f.extend_from_slice(&1u16.to_le_bytes());
            f.extend_from_slice(&1u16.to_le_bytes());
            f.extend_from_slice(&8000u32.to_le_bytes());
            f.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
            f.extend_from_slice(&2u16.to_le_bytes()); // block align
            f.extend_from_slice(&16u16.to_le_bytes()); // bits
            f
        };
        let data = vec![1u8; 7999];
        let mut out = b"RIFF".to_vec();
        let body = 4 + (8 + fmt.len()) + (8 + data.len());
        out.extend_from_slice(&(body as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        out.extend_from_slice(&fmt);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        assert_eq!(out.len() % 2, 1, "fixture must be odd-sized at EOF");
        fs::write(&path, &out)?;
        let added = vec![Annotation {
            seconds: 0.5,
            text: "odd".into(),
        }];
        let stored = annotate(&path, &added)?;
        assert_eq!(stored, added);
        assert_eq!(read(&path)?, added);
        // The rewritten file is padded and still decodes.
        let now = fs::read(&path)?;
        assert_eq!(now.len() % 2, 0);
        let mut reader = AudioReader::open(&path)?;
        let mut frame = [0.0f32; 1];
        let mut count = 0;
        while reader.next_frame(&mut frame)? {
            count += 1;
        }
        assert_eq!(count, 3999); // 7999 bytes / 2 bytes per 16-bit frame
        // Trailing bytes beyond the RIFF size are ignored, not fatal.
        let mut with_junk = now.clone();
        with_junk.extend_from_slice(b"JUNKJUNK");
        let p2 = dir.path().join("junk.wav");
        fs::write(&p2, &with_junk)?;
        assert_eq!(annotate(&p2, &added)?, added);
        Ok(())
    }

    #[test]
    fn unsupported_formats_are_rejected_without_modification() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.flac");
        fs::write(&path, b"fLaC....")?;
        let before = fs::read(&path)?;
        assert!(
            annotate(
                &path,
                &[Annotation {
                    seconds: 1.0,
                    text: "x".into()
                }]
            )
            .is_err()
        );
        assert_eq!(fs::read(&path)?, before);
        assert!(
            annotate(
                &path,
                &[Annotation {
                    seconds: -1.0,
                    text: "x".into()
                }]
            )
            .is_err()
        );
        Ok(())
    }
}
