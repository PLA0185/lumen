//! Local immutable content resources. Originals are never moved or deleted.
use crate::{
    commands::AppState,
    db::{now_stamp, Db},
    error::{AppError, AppResult},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use tauri::State;

pub const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ContentAsset {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub data_base64: String,
    pub byte_size: i64,
    pub sha256: String,
    pub created_at: String,
}

pub fn mime_for(name: &str, bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return "image/webp";
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if bytes.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    match Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "md" | "json" | "csv" | "tsv" | "xml" | "html" | "log" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "rtf" => "application/rtf",
        "odt" => "application/vnd.oasis.opendocument.text",
        _ => "application/octet-stream",
    }
}

pub fn decode_asset(asset: &ContentAsset) -> AppResult<Vec<u8>> {
    if asset.data_base64.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
        return Err(AppError::validation("内容资源超过 20 MiB"));
    }
    let bytes = STANDARD
        .decode(&asset.data_base64)
        .map_err(|_| AppError::validation("内容资源编码损坏"))?;
    if bytes.len() > MAX_ASSET_BYTES
        || bytes.len() as i64 != asset.byte_size
        || hex::encode(Sha256::digest(&bytes)) != asset.sha256
    {
        return Err(AppError::validation("内容资源长度或校验和不匹配"));
    }
    if mime_for(&asset.name, &bytes) != asset.mime {
        return Err(AppError::validation("内容资源格式与类型不匹配"));
    }
    Ok(bytes)
}

pub async fn store_bytes(db: &Db, name: &str, bytes: Vec<u8>) -> AppResult<ContentAsset> {
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(AppError::validation("单个文件最多 20 MiB，请分拆大文件"));
    }
    let name = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 255 || name.chars().any(char::is_control) {
        return Err(AppError::validation("文件名应为 1–255 字且不能含控制字符"));
    }
    let asset = ContentAsset {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_owned(),
        mime: mime_for(name, &bytes).into(),
        data_base64: STANDARD.encode(&bytes),
        byte_size: bytes.len() as i64,
        sha256: hex::encode(Sha256::digest(&bytes)),
        created_at: now_stamp(),
    };
    sqlx::query("INSERT INTO content_assets (id,name,mime,data_base64,byte_size,sha256,created_at) VALUES (?,?,?,?,?,?,?)")
        .bind(&asset.id).bind(&asset.name).bind(&asset.mime).bind(&asset.data_base64).bind(asset.byte_size).bind(&asset.sha256).bind(&asset.created_at)
        .execute(db.pool()).await?;
    Ok(asset)
}

pub async fn get_asset(db: &Db, id: &str) -> AppResult<ContentAsset> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("内容资源编号无效"))?;
    let asset: ContentAsset = sqlx::query_as("SELECT * FROM content_assets WHERE id=?")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::not_found("内容资源", id))?;
    decode_asset(&asset)?;
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn imported_resources_survive_source_removal_and_reject_corruption() {
        let dir = std::env::temp_dir().join(format!("lumen-media-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let bytes = b"business workflow\nstep 1".to_vec();
        let first = store_bytes(&db, "folder/work.txt", bytes.clone())
            .await
            .unwrap();
        let second = store_bytes(&db, "work.txt", b"different".to_vec())
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(
            decode_asset(&get_asset(&db, &first.id).await.unwrap()).unwrap(),
            bytes
        );
        sqlx::query("UPDATE content_assets SET data_base64='YQ==' WHERE id=?")
            .bind(&first.id)
            .execute(db.pool())
            .await
            .unwrap();
        assert!(
            get_asset(&db, &first.id).await.is_err(),
            "corruption must be visible"
        );
        assert!(store_bytes(&db, "large.bin", vec![0; MAX_ASSET_BYTES + 1])
            .await
            .is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 2, "failed imports cannot insert partial rows");
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tauri::command]
pub async fn content_asset_import(
    state: State<'_, AppState>,
    name: String,
    data_base64: String,
) -> AppResult<ContentAsset> {
    if data_base64.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
        return Err(AppError::validation("单个文件最多 20 MiB"));
    }
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| AppError::validation("文件编码无效"))?;
    store_bytes(&state.db, &name, bytes).await
}

#[tauri::command]
pub async fn content_asset_import_path(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<ContentAsset> {
    let file = std::fs::File::open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(AppError::validation("请拖入文件，文件夹不能作为内容导入"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_ASSET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    store_bytes(&state.db, &path, bytes).await
}

#[tauri::command]
pub async fn content_asset_get(state: State<'_, AppState>, id: String) -> AppResult<ContentAsset> {
    get_asset(&state.db, &id).await
}

#[tauri::command]
pub async fn content_asset_export(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> AppResult<()> {
    let asset = get_asset(&state.db, &id).await?;
    std::fs::write(path, decode_asset(&asset)?)?;
    Ok(())
}
