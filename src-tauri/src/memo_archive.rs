//! Preview and atomically import ChatGPT archives containing memo/flow JSON and local images.
use crate::{
    commands::AppState,
    content_assets::{self, ContentAsset, MAX_ASSET_BYTES},
    db::Db,
    error::{AppError, AppResult},
    memos::{self, FlowStep, MemoDocument, SaveMemoInput},
};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
use tauri::State;

const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 1024;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 500;

#[derive(Debug, Clone)]
struct ArchiveRecord {
    input: SaveMemoInput,
    source_parent: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePreviewRecord {
    title: String,
    category: String,
    kind: String,
    body_md: String,
    steps: Vec<FlowStep>,
    image_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePreview {
    records: Vec<ArchivePreviewRecord>,
    image_count: usize,
    sha256: String,
}

fn safe_archive_path(value: &str) -> AppResult<String> {
    let path = value.replace('\\', "/");
    if path.is_empty() || path.starts_with('/') || path.contains(':') || path.contains('\0') {
        return Err(AppError::validation("压缩包中包含无效文件路径"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(AppError::validation("压缩包中包含越界文件路径，已拒绝读取")),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(AppError::validation("压缩包中包含无效文件路径"));
    }
    Ok(parts.join("/"))
}

fn read_limited<R: Read>(reader: R, limit: usize, message: &str) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::new(crate::error::ErrorCode::Io, error.to_string()))?;
    if bytes.len() > limit {
        return Err(AppError::validation(message));
    }
    Ok(bytes)
}

fn archive_parent(path: &str) -> String {
    path.rsplit_once('/')
        .map_or(String::new(), |(parent, _)| parent.to_owned())
}

fn resolve_archive_path(
    source_parent: &str,
    reference: &str,
    entries: &HashSet<String>,
) -> AppResult<String> {
    let normalized = safe_archive_path(reference)?;
    if !source_parent.is_empty() {
        let candidate = safe_archive_path(&format!("{source_parent}/{normalized}"))?;
        if entries.contains(&candidate) {
            return Ok(candidate);
        }
    }
    if entries.contains(&normalized) {
        return Ok(normalized);
    }
    Err(AppError::validation(format!(
        "压缩包缺少图片「{reference}」，请确认 GPT 同时打包了图片文件"
    )))
}

fn image_reference_regex() -> AppResult<Regex> {
    Regex::new(r#"!\[([^\]\n]*)\]\(([^)\n]+)\)"#)
        .map_err(|_| AppError::internal("图片引用解析器不可用"))
}

fn local_image_target(target: &str) -> Option<&str> {
    let trimmed = target.trim();
    let path = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(['<', '>']);
    if path.starts_with("lumen-asset:") || path.contains("://") || path.starts_with("data:") {
        None
    } else {
        Some(path)
    }
}

fn local_images(text: &str, parent: &str, entries: &HashSet<String>) -> AppResult<HashSet<String>> {
    let regex = image_reference_regex()?;
    let mut paths = HashSet::new();
    for captures in regex.captures_iter(text) {
        let target = &captures[2];
        if let Some(reference) = local_image_target(target) {
            paths.insert(resolve_archive_path(parent, reference, entries)?);
        } else if target.contains("lumen-asset:") {
            return Err(AppError::validation(
                "GPT 压缩包不能只包含 Lumen 本机图片编号；请让 GPT 把图片文件也放进压缩包",
            ));
        }
    }
    Ok(paths)
}

fn rewrite_images(
    text: &str,
    source_parent: &str,
    mapping: &HashMap<String, String>,
) -> AppResult<String> {
    let regex = image_reference_regex()?;
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for captures in regex.captures_iter(text) {
        let whole = captures.get(0).expect("full match");
        let target = &captures[2];
        let Some(reference) = local_image_target(target) else {
            continue;
        };
        let raw_target = target.trim();
        let path_token = raw_target.split_whitespace().next().unwrap_or("");
        let title_suffix = raw_target.get(path_token.len()..).unwrap_or("");
        let normalized = safe_archive_path(reference)?;
        let candidate = if source_parent.is_empty() {
            normalized.clone()
        } else {
            safe_archive_path(&format!("{source_parent}/{normalized}"))?
        };
        let asset = mapping
            .get(&candidate)
            .or_else(|| mapping.get(&normalized))
            .ok_or_else(|| AppError::validation(format!("未能关联图片「{reference}」")))?;
        result.push_str(&text[cursor..whole.start()]);
        result.push_str("![");
        result.push_str(&captures[1]);
        result.push_str("](lumen-asset:");
        result.push_str(asset);
        result.push_str(title_suffix);
        result.push(')');
        cursor = whole.end();
    }
    result.push_str(&text[cursor..]);
    Ok(result)
}

fn json_string(value: &Value, keys: &[&str], default: &str) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or(default)
        .to_owned()
}

fn parse_record(value: &Value, source_path: &str) -> AppResult<ArchiveRecord> {
    let title = json_string(value, &["title"], "").trim().to_owned();
    let kind = json_string(value, &["kind", "type"], "").to_ascii_lowercase();
    if title.is_empty() || !matches!(kind.as_str(), "memo" | "flow") {
        return Err(AppError::validation(
            "JSON 记录必须包含标题，以及 kind 为 memo 或 flow",
        ));
    }
    let body_md = json_string(value, &["bodyMd", "body_md", "content", "summary"], "");
    let raw_steps = value
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if raw_steps.len() > 100 || (kind == "memo" && !raw_steps.is_empty()) {
        return Err(AppError::validation(format!(
            "「{title}」的流程步骤数量或记录类型不匹配"
        )));
    }
    let mut step_ids = HashSet::new();
    let mut steps = Vec::with_capacity(raw_steps.len());
    for (index, step) in raw_steps.iter().enumerate() {
        if !step.is_object() {
            return Err(AppError::validation(format!(
                "「{title}」第 {} 步格式无效",
                index + 1
            )));
        }
        let proposed_id = json_string(step, &["id"], "");
        let id = if !proposed_id.is_empty() && step_ids.insert(proposed_id.clone()) {
            proposed_id
        } else {
            let next = uuid::Uuid::now_v7().to_string();
            step_ids.insert(next.clone());
            next
        };
        let step_title = json_string(step, &["title"], "").trim().to_owned();
        if step_title.is_empty() {
            return Err(AppError::validation(format!(
                "「{title}」第 {} 步缺少标题",
                index + 1
            )));
        }
        steps.push(FlowStep {
            id,
            title: step_title,
            owner: json_string(step, &["owner"], ""),
            detail: json_string(step, &["detail", "content"], ""),
            layout: step
                .get("layout")
                .and_then(|layout| serde_json::from_value(layout.clone()).ok()),
            group: step
                .get("group")
                .and_then(|group| serde_json::from_value(group.clone()).ok()),
        });
    }
    let record = ArchiveRecord {
        input: SaveMemoInput {
            id: None,
            expected_revision: None,
            title,
            category: json_string(value, &["category"], ""),
            kind,
            body_md,
            steps,
        },
        source_parent: archive_parent(source_path),
    };
    Ok(record)
}

fn inspect_archive(bytes: &[u8]) -> AppResult<(Vec<ArchiveRecord>, HashMap<String, usize>)> {
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AppError::validation("压缩包为空或超过 256 MiB"));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        AppError::validation("无法识别该 ZIP。请拖入包含 JSON 记录与图片的 GPT 压缩包")
    })?;
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AppError::validation("压缩包文件数量无效（最多 1024 项）"));
    }
    let mut names = HashSet::new();
    let mut entry_sizes = HashMap::new();
    let mut json_files = Vec::<(String, Vec<u8>)>::new();
    let mut expanded = 0u64;
    let mut total_json_bytes = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| AppError::validation("压缩包目录损坏"))?;
        if entry.is_dir() {
            continue;
        }
        let name = safe_archive_path(entry.name())?;
        if !names.insert(name.clone()) {
            return Err(AppError::validation(format!(
                "压缩包中有重复文件名「{name}」"
            )));
        }
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_UNPACKED_BYTES {
            return Err(AppError::validation("压缩包解压后超过 512 MiB，已拒绝读取"));
        }
        let size =
            usize::try_from(entry.size()).map_err(|_| AppError::validation("压缩包文件过大"))?;
        entry_sizes.insert(name.clone(), size);
        if name.to_ascii_lowercase().ends_with(".json") {
            if size > MAX_JSON_BYTES {
                return Err(AppError::validation("单个 JSON 文件超过 16 MiB"));
            }
            total_json_bytes = total_json_bytes.saturating_add(size);
            if total_json_bytes > MAX_TOTAL_JSON_BYTES {
                return Err(AppError::validation("压缩包内 JSON 总量超过 32 MiB"));
            }
            let content = read_limited(&mut entry, MAX_JSON_BYTES, "JSON 文件超过 16 MiB")?;
            json_files.push((name, content));
        }
    }
    let mut bundled: Option<(String, Value, usize)> = None;
    for (path, raw) in &json_files {
        let Ok(value) = serde_json::from_slice::<Value>(raw) else {
            continue;
        };
        if let Some(array) = value.as_array() {
            let items = array
                .iter()
                .filter(|item| item.get("title").is_some() && item.get("kind").is_some())
                .count();
            if items > 0 {
                let length = value.as_array().map_or(0, Vec::len);
                if bundled
                    .as_ref()
                    .is_none_or(|(_, _, current)| length > *current)
                {
                    bundled = Some((path.clone(), value, length));
                }
            }
        }
    }
    let mut records = Vec::new();
    if let Some((path, value, _)) = bundled {
        let items = value.as_array().expect("array checked above");
        if items.len() > MAX_RECORDS {
            return Err(AppError::validation("压缩包最多导入 500 条记录"));
        }
        for item in items
            .iter()
            .filter(|item| item.get("title").is_some() && item.get("kind").is_some())
        {
            records.push(parse_record(item, &path)?);
        }
    } else {
        for (path, raw) in json_files {
            let Ok(value) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            if value.is_object()
                && value.get("title").is_some()
                && (value.get("kind").is_some() || value.get("type").is_some())
            {
                records.push(parse_record(&value, &path)?);
            }
            if records.len() > MAX_RECORDS {
                return Err(AppError::validation("压缩包最多导入 500 条记录"));
            }
        }
    }
    if records.is_empty() {
        return Err(AppError::validation(
            "压缩包中没有可识别的 Lumen 备忘或流程 JSON",
        ));
    }
    for record in &records {
        memos::validate(&record.input)?;
    }
    Ok((records, entry_sizes))
}

fn collect_references(
    records: &[ArchiveRecord],
    entries: &HashSet<String>,
) -> AppResult<Vec<Vec<String>>> {
    records
        .iter()
        .map(|record| {
            let mut paths = local_images(&record.input.body_md, &record.source_parent, entries)?;
            for step in &record.input.steps {
                paths.extend(local_images(&step.detail, &record.source_parent, entries)?);
            }
            Ok(paths.into_iter().collect())
        })
        .collect()
}

fn read_archive_file(path: &str) -> AppResult<Vec<u8>> {
    let source = Path::new(path);
    if !source.is_absolute() {
        return Err(AppError::validation("请选择有效的绝对路径 ZIP 文件"));
    }
    let file = File::open(source)?;
    if !file.metadata()?.is_file() {
        return Err(AppError::validation("请选择 ZIP 文件，而不是文件夹"));
    }
    read_limited(file, MAX_ARCHIVE_BYTES, "压缩包超过 256 MiB")
}

fn verify_archive_hash(bytes: &[u8], expected_sha256: &str) -> AppResult<()> {
    if hex::encode(Sha256::digest(bytes)) != expected_sha256.to_ascii_lowercase() {
        return Err(AppError::conflict(
            "ZIP 文件在预览后发生变化。请重新选择压缩包并核对新预览",
        ));
    }
    Ok(())
}

fn preview_from_bytes(bytes: &[u8]) -> AppResult<ArchivePreview> {
    let (records, sizes) = inspect_archive(bytes)?;
    let paths: HashSet<String> = sizes.keys().cloned().collect();
    let references = collect_references(&records, &paths)?;
    let mut all = HashSet::new();
    let mut previews = Vec::with_capacity(records.len());
    for (record, refs) in records.into_iter().zip(references) {
        for path in &refs {
            let size = sizes.get(path).copied().unwrap_or(usize::MAX);
            if size > MAX_ASSET_BYTES {
                return Err(AppError::validation(format!("图片「{path}」超过 20 MiB")));
            }
            all.insert(path.clone());
        }
        previews.push(ArchivePreviewRecord {
            title: record.input.title,
            category: record.input.category,
            kind: record.input.kind,
            body_md: record.input.body_md,
            steps: record.input.steps,
            image_count: refs.len(),
        });
    }
    Ok(ArchivePreview {
        records: previews,
        image_count: all.len(),
        sha256: hex::encode(Sha256::digest(bytes)),
    })
}

#[tauri::command]
pub async fn memo_archive_preview(path: String) -> AppResult<ArchivePreview> {
    let bytes = read_archive_file(&path)?;
    preview_from_bytes(&bytes)
}

async fn import_archive_bytes(db: &Db, bytes: &[u8]) -> AppResult<Vec<MemoDocument>> {
    let (mut records, sizes) = inspect_archive(bytes)?;
    let paths: HashSet<String> = sizes.keys().cloned().collect();
    let references = collect_references(&records, &paths)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| AppError::validation("压缩包目录损坏"))?;
    let all_references: HashSet<String> = references.iter().flatten().cloned().collect();
    let mut assets_by_path = HashMap::<String, ContentAsset>::new();
    for path in all_references {
        let mut entry = archive
            .by_name(&path)
            .map_err(|_| AppError::validation(format!("压缩包缺少图片「{path}」")))?;
        let raw = read_limited(&mut entry, MAX_ASSET_BYTES, "图片超过 20 MiB")?;
        let asset = content_assets::prepare_bytes(&path, raw)?;
        if !asset.mime.starts_with("image/") {
            return Err(AppError::validation(format!(
                "「{path}」不是可识别的图片文件"
            )));
        }
        assets_by_path.insert(path, asset);
    }
    let mut tx = db.pool().begin().await?;
    let mut ids = HashMap::new();
    for (path, asset) in &assets_by_path {
        sqlx::query("INSERT INTO content_assets (id,name,mime,data_base64,byte_size,sha256,created_at) VALUES (?,?,?,?,?,?,?)")
            .bind(&asset.id).bind(&asset.name).bind(&asset.mime).bind(&asset.data_base64)
            .bind(asset.byte_size).bind(&asset.sha256).bind(&asset.created_at)
            .execute(&mut *tx).await?;
        ids.insert(path.clone(), asset.id.clone());
    }
    let mut imported = Vec::with_capacity(records.len());
    for (record, refs) in records.iter_mut().zip(references) {
        let mut resolved = HashMap::new();
        for path in refs {
            if let Some(id) = ids.get(&path) {
                resolved.insert(path, id.clone());
            }
        }
        record.input.body_md =
            rewrite_images(&record.input.body_md, &record.source_parent, &resolved)?;
        for step in &mut record.input.steps {
            step.detail = rewrite_images(&step.detail, &record.source_parent, &resolved)?;
        }
        imported.push(memos::save_new_in_transaction(&mut tx, record.input.clone()).await?);
    }
    tx.commit().await?;
    Ok(imported)
}

#[tauri::command]
pub async fn memo_archive_import(
    state: State<'_, AppState>,
    path: String,
    expected_sha256: String,
) -> AppResult<Vec<MemoDocument>> {
    let bytes = read_archive_file(&path)?;
    verify_archive_hash(&bytes, &expected_sha256)?;
    import_archive_bytes(&state.db, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_archive() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let records = serde_json::json!([
            {"kind":"flow","title":"流程","category":"测试","bodyMd":"背景","steps":[{"title":"步骤一","owner":"","detail":"核对\n\n![原图](images/step.png)"}]},
            {"kind":"memo","title":"备忘","category":"测试","bodyMd":"![参考图](images/step.png)","steps":[]}
        ]);
        zip.start_file("all.json", options).unwrap();
        zip.write_all(records.to_string().as_bytes()).unwrap();
        zip.start_file("images/step.png", options).unwrap();
        zip.write_all(b"\x89PNG\r\n\x1a\narchive-image").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn previews_batch_records_and_finds_local_images_without_saving() {
        let preview = preview_from_bytes(&sample_archive()).unwrap();
        assert_eq!(preview.records.len(), 2);
        assert_eq!(preview.records[0].kind, "flow");
        assert_eq!(preview.records[0].steps[0].title, "步骤一");
        assert_eq!(preview.records[0].image_count, 1);
        assert_eq!(preview.image_count, 1, "shared image is counted once");
        assert_eq!(preview.sha256.len(), 64);
    }

    #[tokio::test]
    async fn importing_archive_saves_all_records_and_keeps_images_with_their_references() {
        let dir = std::env::temp_dir().join(format!("lumen-archive-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let imported = import_archive_bytes(&db, &sample_archive()).await.unwrap();
        assert_eq!(imported.len(), 2);
        assert!(imported[0].steps[0].detail.contains("lumen-asset:"));
        assert!(imported[1].body_md.contains("lumen-asset:"));
        let images: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(images, 1);
        assert_eq!(memos::list_impl(&db, "", false).await.unwrap().len(), 2);
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_zip_path_traversal_and_missing_images() {
        assert!(safe_archive_path("../outside.png").is_err());
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("record.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"kind":"memo","title":"missing","bodyMd":"![image](images/gone.png)"}"#)
            .unwrap();
        assert!(preview_from_bytes(&zip.finish().unwrap().into_inner()).is_err());
    }

    #[test]
    fn import_requires_the_same_archive_that_was_previewed() {
        let bytes = sample_archive();
        let digest = hex::encode(Sha256::digest(&bytes));
        verify_archive_hash(&bytes, &digest).unwrap();
        assert!(verify_archive_hash(&bytes, &"0".repeat(64)).is_err());
    }
}
