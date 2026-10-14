//! Portable, round-trip packages for a single memo or business flow.
use crate::{
    commands::AppState,
    content_assets::{self, ContentAsset, MAX_ASSET_BYTES},
    db::Db,
    error::{AppError, AppResult},
    memos::{self, FlowStep, MemoDocument, SaveMemoInput},
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Write},
    path::Path,
};
use tauri::State;

const PACKAGE_FORMAT: &str = "lumen-record";
const PACKAGE_VERSION: u32 = 1;
const MAX_PACKAGE_BYTES: usize = 512 * 1024 * 1024;
const MAX_PACKAGE_ASSETS: usize = 256;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoPackage {
    format: String,
    version: u32,
    record: PackageRecord,
    assets: Vec<PackageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageRecord {
    title: String,
    category: String,
    kind: String,
    body_md: String,
    steps: Vec<FlowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageAsset {
    id: String,
    name: String,
    mime: String,
    sha256: String,
    byte_size: usize,
}

impl From<&MemoDocument> for PackageRecord {
    fn from(doc: &MemoDocument) -> Self {
        Self {
            title: doc.summary.title.clone(),
            category: doc.summary.category.clone(),
            kind: doc.summary.kind.clone(),
            body_md: doc.body_md.clone(),
            steps: doc.steps.clone(),
        }
    }
}

impl PackageRecord {
    fn into_input(self) -> SaveMemoInput {
        SaveMemoInput {
            id: None,
            expected_revision: None,
            title: self.title,
            category: self.category,
            kind: self.kind,
            body_md: self.body_md,
            steps: self.steps,
        }
    }
}

fn asset_ids(record: &PackageRecord) -> AppResult<Vec<String>> {
    let pattern = Regex::new(r"lumen-asset:([0-9a-fA-F-]{36})")
        .map_err(|_| AppError::internal("内容资源引用解析器不可用"))?;
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for text in std::iter::once(record.body_md.as_str())
        .chain(record.steps.iter().map(|step| step.detail.as_str()))
    {
        for capture in pattern.captures_iter(text) {
            let id = capture[1].to_owned();
            uuid::Uuid::parse_str(&id)
                .map_err(|_| AppError::validation("记录包含无效的图片或文件编号"))?;
            if seen.insert(id.to_lowercase()) {
                ids.push(id);
            }
        }
    }
    if ids.len() > MAX_PACKAGE_ASSETS {
        return Err(AppError::validation("一条记录最多打包 256 个图片或文件"));
    }
    Ok(ids)
}

async fn export_package_bytes(db: &Db, doc: &MemoDocument) -> AppResult<Vec<u8>> {
    let record = PackageRecord::from(doc);
    let mut assets = Vec::new();
    let mut asset_bytes = Vec::new();
    let mut total_asset_bytes = 0usize;
    for id in asset_ids(&record)? {
        let asset = content_assets::get_asset(db, &id).await?;
        let bytes = content_assets::decode_asset(&asset)?;
        total_asset_bytes = total_asset_bytes.saturating_add(bytes.len());
        if total_asset_bytes > MAX_PACKAGE_BYTES {
            return Err(AppError::validation("记录引用的图片和文件总量超过 512 MiB"));
        }
        asset_bytes.push((id.clone(), bytes));
        assets.push(PackageAsset {
            id,
            name: asset.name,
            mime: asset.mime,
            sha256: asset.sha256,
            byte_size: asset.byte_size as usize,
        });
    }
    let manifest = MemoPackage {
        format: PACKAGE_FORMAT.into(),
        version: PACKAGE_VERSION,
        record,
        assets,
    };
    let manifest = serde_json::to_vec(&manifest)
        .map_err(|error| AppError::internal(format!("记录包清单序列化失败：{error}")))?;
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(AppError::validation("记录包清单超过 2 MiB"));
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file("manifest.json", options)
        .map_err(|error| AppError::internal(format!("创建记录包失败：{error}")))?;
    archive
        .write_all(&manifest)
        .map_err(|error| AppError::internal(format!("写入记录包清单失败：{error}")))?;
    for (id, bytes) in asset_bytes {
        archive
            .start_file(format!("assets/{id}.bin"), options)
            .map_err(|error| AppError::internal(format!("创建记录包资源失败：{error}")))?;
        archive
            .write_all(&bytes)
            .map_err(|error| AppError::internal(format!("写入记录包资源失败：{error}")))?;
    }
    let bytes = archive
        .finish()
        .map_err(|error| AppError::internal(format!("完成记录包失败：{error}")))?
        .into_inner();
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(AppError::validation("记录包超过 512 MiB"));
    }
    Ok(bytes)
}

fn read_entry<R: Read>(entry: &mut zip::read::ZipFile<'_, R>, limit: usize) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::with_capacity(entry.size().min(limit as u64) as usize);
    entry.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(AppError::validation("记录包中的文件超过允许大小"));
    }
    Ok(bytes)
}

async fn import_package_bytes(db: &Db, bytes: &[u8]) -> AppResult<MemoDocument> {
    if bytes.is_empty() || bytes.len() > MAX_PACKAGE_BYTES {
        return Err(AppError::validation("记录包为空或超过 512 MiB"));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| AppError::validation("这不是有效的 Lumen 记录包"))?;
    if archive.is_empty() || archive.len() > MAX_PACKAGE_ASSETS + 1 {
        return Err(AppError::validation("记录包文件数量无效"));
    }
    let manifest_bytes = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| AppError::validation("记录包缺少 manifest.json"))?;
        read_entry(&mut entry, MAX_MANIFEST_BYTES)?
    };
    let mut manifest: MemoPackage = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| AppError::validation("记录包清单损坏或版本不受支持"))?;
    if manifest.format != PACKAGE_FORMAT
        || manifest.version != PACKAGE_VERSION
        || manifest.assets.len() > MAX_PACKAGE_ASSETS
        || archive.len() != manifest.assets.len() + 1
    {
        return Err(AppError::validation("记录包格式、版本或资源数量无效"));
    }
    let mut input = manifest.record.clone().into_input();
    memos::validate(&input)?;
    let referenced = asset_ids(&manifest.record)?;
    let referenced_set: HashSet<String> = referenced.iter().map(|id| id.to_lowercase()).collect();
    let packaged_set: HashSet<String> = manifest
        .assets
        .iter()
        .map(|asset| asset.id.to_lowercase())
        .collect();
    if referenced_set != packaged_set || packaged_set.len() != manifest.assets.len() {
        return Err(AppError::validation("记录包图片或文件与正文引用不匹配"));
    }

    let mut prepared = Vec::<(PackageAsset, ContentAsset)>::new();
    let mut total_asset_bytes = 0usize;
    for asset_meta in manifest.assets.drain(..) {
        uuid::Uuid::parse_str(&asset_meta.id)
            .map_err(|_| AppError::validation("记录包含无效的内容资源编号"))?;
        if asset_meta.byte_size > MAX_ASSET_BYTES
            || asset_meta.sha256.len() != 64
            || !asset_meta
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(AppError::validation("记录包内容资源元数据无效"));
        }
        let expected_path = format!("assets/{}.bin", asset_meta.id);
        let mut entry = archive
            .by_name(&expected_path)
            .map_err(|_| AppError::validation("记录包缺少正文引用的图片或文件"))?;
        let raw = read_entry(&mut entry, MAX_ASSET_BYTES)?;
        total_asset_bytes = total_asset_bytes.saturating_add(raw.len());
        if total_asset_bytes > MAX_PACKAGE_BYTES {
            return Err(AppError::validation("记录包解压后的内容超过 512 MiB"));
        }
        if raw.len() != asset_meta.byte_size
            || hex::encode(Sha256::digest(&raw)) != asset_meta.sha256.to_lowercase()
        {
            return Err(AppError::validation("记录包内容资源长度或校验和不匹配"));
        }
        let asset = content_assets::prepare_bytes(&asset_meta.name, raw)?;
        if asset.mime != asset_meta.mime {
            return Err(AppError::validation("记录包内容资源类型与文件不匹配"));
        }
        prepared.push((asset_meta, asset));
    }

    let mut tx = db.pool().begin().await?;
    let mut replacements = HashMap::<String, String>::new();
    for (meta, mut asset) in prepared {
        let existing_by_id = sqlx::query("SELECT id,sha256 FROM content_assets WHERE id=?")
            .bind(&meta.id)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(row) = existing_by_id {
            let existing_id: String = row.try_get("id")?;
            let existing_hash: String = row.try_get("sha256")?;
            if existing_hash == meta.sha256 {
                replacements.insert(meta.id, existing_id);
                continue;
            }
            asset.id = uuid::Uuid::now_v7().to_string();
        } else {
            asset.id = meta.id.clone();
        }
        replacements.insert(meta.id, asset.id.clone());
        sqlx::query("INSERT INTO content_assets (id,name,mime,data_base64,byte_size,sha256,created_at) VALUES (?,?,?,?,?,?,?)")
            .bind(&asset.id).bind(&asset.name).bind(&asset.mime).bind(&asset.data_base64)
            .bind(asset.byte_size).bind(&asset.sha256).bind(&asset.created_at)
            .execute(&mut *tx).await?;
    }

    let rewrite_assets = |text: &str| {
        let mut rewritten = text.to_owned();
        for (old, new) in &replacements {
            if old != new {
                rewritten = rewritten.replace(old, new);
            }
        }
        rewritten
    };
    input.body_md = rewrite_assets(&input.body_md);
    for step in &mut input.steps {
        step.detail = rewrite_assets(&step.detail);
    }
    let document = memos::save_new_in_transaction(&mut tx, input).await?;
    tx.commit().await?;
    Ok(document)
}

fn write_package(path: &str, bytes: &[u8]) -> AppResult<()> {
    let destination = Path::new(path);
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err(AppError::validation("记录包导出路径无效"));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::validation("记录包导出文件名无效"))?;
    let temporary =
        destination.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::now_v7()));
    let result = (|| -> AppResult<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[tauri::command]
pub async fn memo_package_export(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> AppResult<u64> {
    let document = memos::get_impl(&state.db, &id).await?;
    let bytes = export_package_bytes(&state.db, &document).await?;
    write_package(&path, &bytes)?;
    Ok(bytes.len() as u64)
}

#[tauri::command]
pub async fn memo_package_import(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<MemoDocument> {
    let path = Path::new(&path);
    if !path.is_absolute() {
        return Err(AppError::validation("记录包导入路径无效"));
    }
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(AppError::validation("请选择一个 Lumen 记录包文件"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PACKAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    import_package_bytes(&state.db, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memos::{FlowGroup, FlowStep};

    #[tokio::test]
    async fn memo_and_flow_packages_round_trip_content_and_attachments() {
        let dir =
            std::env::temp_dir().join(format!("lumen-record-package-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let image = content_assets::store_bytes(&db, "步骤截图.png", b"fake png bytes".to_vec())
            .await
            .unwrap();
        let flow = memos::save_impl(
            &db,
            SaveMemoInput {
                id: None,
                expected_revision: None,
                title: "出货流程".into(),
                category: "物流".into(),
                kind: "flow".into(),
                body_md: format!("适用说明。\n\n![总览](lumen-asset:{})", image.id),
                steps: vec![FlowStep {
                    id: "step-1".into(),
                    title: "核对货件".into(),
                    owner: "运营".into(),
                    detail: format!("核对编号。\n\n![对应截图](lumen-asset:{})", image.id),
                    layout: Some(crate::memos::FlowStepLayout {
                        width: 560.0,
                        min_height: 220.0,
                        x: Some(12.0),
                        y: Some(18.0),
                    }),
                    group: Some(FlowGroup {
                        id: "shipping".into(),
                        title: "物流".into(),
                        path: vec!["出货".into(), "核对".into()],
                    }),
                }],
            },
        )
        .await
        .unwrap();
        let package = export_package_bytes(&db, &flow).await.unwrap();
        let imported = import_package_bytes(&db, &package).await.unwrap();
        assert_ne!(imported.summary.id, flow.summary.id);
        assert_eq!(imported.summary.title, flow.summary.title);
        assert_eq!(imported.summary.category, flow.summary.category);
        assert_eq!(imported.summary.kind, flow.summary.kind);
        assert_eq!(imported.body_md, flow.body_md);
        assert_eq!(
            serde_json::to_value(&imported.steps).unwrap(),
            serde_json::to_value(&flow.steps).unwrap()
        );
        assert_eq!(
            content_assets::decode_asset(&content_assets::get_asset(&db, &image.id).await.unwrap())
                .unwrap(),
            b"fake png bytes"
        );

        let memo = memos::save_impl(
            &db,
            SaveMemoInput {
                id: None,
                expected_revision: None,
                title: "业务备忘".into(),
                category: "运营".into(),
                kind: "memo".into(),
                body_md: "## 注意事项\n\n保留原格式。".into(),
                steps: vec![],
            },
        )
        .await
        .unwrap();
        let memo_package = export_package_bytes(&db, &memo).await.unwrap();
        let imported_memo = import_package_bytes(&db, &memo_package).await.unwrap();
        assert_eq!(imported_memo.summary.kind, "memo");
        assert_eq!(imported_memo.summary.title, memo.summary.title);
        assert_eq!(imported_memo.body_md, memo.body_md);
        assert!(imported_memo.steps.is_empty());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
