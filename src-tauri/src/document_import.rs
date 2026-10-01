//! Bounded local document recognition; originals remain immutable resources.
use crate::{
    commands::AppState,
    content_assets::{decode_asset, get_asset, mime_for, ContentAsset, MAX_ASSET_BYTES},
    db::Db,
    error::{AppError, AppResult},
};
use calamine::Reader as _;
use quick_xml::{events::Event, Reader};
use serde::Serialize;
use std::io::{Cursor, Read};
use tauri::State;

const MAX_TEXT: usize = 100_000;
const MAX_EXPANDED: usize = 40 * 1024 * 1024;
const MAX_PAGES: usize = 100;
const MAX_CELLS: usize = 100_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub text: String,
    pub images: Vec<ContentAsset>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct Parsed {
    text: String,
    images: Vec<(String, Vec<u8>)>,
    warnings: Vec<String>,
}

fn parser_error(what: &str, error: impl std::fmt::Display) -> AppError {
    AppError::validation(format!("{what}识别失败：{error}"))
}

fn append(text: &mut String, value: &str) -> AppResult<()> {
    // Four bytes per character is an inexpensive early bound before counting.
    if text.len() + value.len() > MAX_TEXT * 4 {
        return Err(AppError::validation("识别文字超过 100000 字，请拆分文件"));
    }
    text.push_str(value);
    Ok(())
}

fn check_text(text: &str) -> AppResult<()> {
    if text.chars().count() > MAX_TEXT {
        return Err(AppError::validation("识别文字超过 100000 字，请拆分文件"));
    }
    Ok(())
}

/// Preflight both declared and actual decompression, including entries the parser ignores.
fn bounded_archive(bytes: &[u8]) -> AppResult<Vec<(String, Vec<u8>)>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| parser_error("Office 文件", e))?;
    if archive.len() > 2_000 {
        return Err(AppError::validation("Office 文件内部条目超过 2000 个"));
    }
    let mut total = 0usize;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| parser_error("Office 文件", e))?;
        if file.size() > (MAX_EXPANDED - total) as u64 {
            return Err(AppError::validation("Office 文件解压后超过 40 MiB"));
        }
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.by_ref()
            .take((MAX_EXPANDED - total + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|e| parser_error("Office 文件", e))?;
        total += data.len();
        if total > MAX_EXPANDED {
            return Err(AppError::validation("Office 文件解压后超过 40 MiB"));
        }
        entries.push((name, data));
    }
    Ok(entries)
}

fn office_images(
    parsed: &mut Parsed,
    entries: &[(String, Vec<u8>)],
    prefix: &str,
) -> AppResult<()> {
    let mut total = 0;
    for (name, bytes) in entries.iter().filter(|(name, _)| name.starts_with(prefix)) {
        if bytes.is_empty() {
            continue;
        }
        if !mime_for(name, bytes).starts_with("image/") {
            parsed
                .warnings
                .push(format!("内嵌资源 {name} 的格式无法作为图片导入"));
            continue;
        }
        total += bytes.len();
        if parsed.images.len() >= 100 || total > MAX_ASSET_BYTES {
            return Err(AppError::validation(
                "内嵌图片超过 100 张或 20 MiB，请拆分文件",
            ));
        }
        parsed.images.push((
            name.rsplit('/').next().unwrap_or(name).into(),
            bytes.clone(),
        ));
    }
    Ok(())
}

fn word(bytes: &[u8]) -> AppResult<Parsed> {
    let entries = bounded_archive(bytes)?;
    let xml = entries
        .iter()
        .find(|(name, _)| name == "word/document.xml")
        .ok_or_else(|| AppError::validation("DOCX 缺少 word/document.xml"))?;
    let mut reader = Reader::from_reader(xml.1.as_slice());
    let mut parsed = Parsed::default();
    let mut in_text = false;
    let mut depth = 0usize;
    loop {
        match reader.read_event().map_err(|e| parser_error("DOCX", e))? {
            Event::Start(e) => {
                depth += 1;
                if e.local_name().as_ref() == "t" {
                    in_text = true;
                }
            }
            Event::Text(e) if in_text => append(&mut parsed.text, e.as_ref())?,
            Event::GeneralRef(e) if in_text => {
                let entity = format!("&{};", e.as_ref());
                append(
                    &mut parsed.text,
                    &quick_xml::escape::unescape(&entity).map_err(|e| parser_error("DOCX", e))?,
                )?;
            }
            Event::Empty(e) => match e.local_name().as_ref() {
                "tab" => append(&mut parsed.text, "\t")?,
                "br" | "cr" => append(&mut parsed.text, "\n")?,
                _ => (),
            },
            Event::End(e) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| AppError::validation("DOCX XML 结构损坏"))?;
                match e.local_name().as_ref() {
                    "t" => in_text = false,
                    "p" => append(&mut parsed.text, "\n")?,
                    "tc" => {
                        while parsed.text.ends_with('\n') {
                            parsed.text.pop();
                        }
                        append(&mut parsed.text, "\t")?;
                    }
                    "tr" => {
                        while parsed.text.ends_with('\t') {
                            parsed.text.pop();
                        }
                        append(&mut parsed.text, "\n")?;
                    }
                    _ => (),
                }
            }
            Event::DocType(_) => return Err(AppError::validation("DOCX 不允许 XML DTD")),
            Event::Eof => {
                if depth != 0 {
                    return Err(AppError::validation("DOCX XML 未完整结束"));
                }
                break;
            }
            _ => (),
        }
    }
    office_images(&mut parsed, &entries, "word/media/")?;
    parsed
        .warnings
        .push("已提取正文、表格和内嵌图片；页眉、脚注未导入，文本框和原版式不保证还原。".into());
    Ok(parsed)
}

fn excel(bytes: &[u8], modern: bool) -> AppResult<Parsed> {
    let mut parsed = Parsed::default();
    let mut cell_count = 0;
    if modern {
        let entries = bounded_archive(bytes)?;
        // OOXML may relocate the workbook through _rels/.rels. Check every
        // shared string table before Calamine follows those relationships.
        for (_, xml) in entries.iter().filter(|(name, _)| {
            name.rsplit(['/', '\\'])
                .next()
                .is_some_and(|part| part.eq_ignore_ascii_case("sharedStrings.xml"))
        }) {
            bounded_shared_strings(xml)?;
        }
        let mut workbook =
            calamine::Xlsx::new(Cursor::new(bytes)).map_err(|e| parser_error("Excel", e))?;
        let names = workbook.sheet_names().to_vec();
        if names.len() > MAX_PAGES {
            return Err(AppError::validation("Excel 工作表超过 100 张"));
        }
        for name in names {
            append(&mut parsed.text, &format!("工作表：{name}\n"))?;
            // Stream sparse cells; do not allocate an attacker-controlled rectangular range.
            let mut cells = workbook
                .worksheet_cells_reader(&name)
                .map_err(|e| parser_error("Excel", e))?;
            let mut previous_position: Option<(u32, u32)> = None;
            while let Some(cell) = cells.next_cell().map_err(|e| parser_error("Excel", e))? {
                cell_count += 1;
                if cell_count > MAX_CELLS {
                    return Err(AppError::validation("Excel 单元格超过 100000 个"));
                }
                let (row, col) = cell.get_position();
                let tabs = match previous_position {
                    Some((previous_row, previous_col)) if previous_row == row => col
                        .checked_sub(previous_col)
                        .filter(|delta| *delta > 0)
                        .ok_or_else(|| AppError::validation("Excel 单元格顺序损坏"))?,
                    Some((previous_row, _)) => {
                        let lines = row
                            .checked_sub(previous_row)
                            .filter(|delta| *delta > 0)
                            .ok_or_else(|| AppError::validation("Excel 单元格顺序损坏"))?;
                        if lines as usize > MAX_CELLS {
                            return Err(AppError::validation("Excel 空白行范围超过 100000 行"));
                        }
                        append(&mut parsed.text, &"\n".repeat(lines as usize))?;
                        col
                    }
                    None => col,
                };
                if tabs as usize > MAX_CELLS {
                    return Err(AppError::validation("Excel 空白列范围超过 100000 列"));
                }
                append(&mut parsed.text, &"\t".repeat(tabs as usize))?;
                append(
                    &mut parsed.text,
                    &calamine::Data::from(cell.get_value().clone()).to_string(),
                )?;
                previous_position = Some((row, col));
            }
            append(&mut parsed.text, "\n\n")?;
        }
        office_images(&mut parsed, &entries, "xl/media/")?;
    } else {
        bounded_xls(bytes)?;
        let mut workbook =
            calamine::Xls::new(Cursor::new(bytes)).map_err(|e| parser_error("Excel XLS", e))?;
        let names = workbook.sheet_names().to_vec();
        if names.len() > MAX_PAGES {
            return Err(AppError::validation("Excel 工作表超过 100 张"));
        }
        for name in names {
            let range = workbook
                .worksheet_range(&name)
                .map_err(|e| parser_error("Excel XLS", e))?;
            cell_count += range.get_size().0.saturating_mul(range.get_size().1);
            if cell_count > MAX_CELLS {
                return Err(AppError::validation("Excel 单元格范围超过 100000 个"));
            }
            append(&mut parsed.text, &format!("工作表：{name}\n"))?;
            for row in range.rows() {
                let values = row
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\t");
                append(&mut parsed.text, &values)?;
                append(&mut parsed.text, "\n")?;
            }
        }
    }
    parsed
        .warnings
        .push("已提取工作表单元格值；公式、图表、合并单元格和原版式未重建。".into());
    Ok(parsed)
}

fn bounded_shared_strings(xml: &[u8]) -> AppResult<()> {
    let mut reader = Reader::from_reader(xml);
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut strings = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| parser_error("Excel 共享文字 XML", e))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if depth == 0 {
                    if root_seen || e.local_name().as_ref() != "sst" {
                        return Err(AppError::validation("Excel 共享文字 XML 根结构损坏"));
                    }
                    root_seen = true;
                    for attribute in e.attributes() {
                        let attribute =
                            attribute.map_err(|e| parser_error("Excel 共享文字属性", e))?;
                        if matches!(attribute.key.as_ref(), "uniqueCount" | "count") {
                            let value = attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_err(|e| parser_error("Excel 共享文字属性", e))?;
                            let count = value.parse::<u64>().map_err(|_| {
                                AppError::validation("Excel 共享文字数量不是有效整数")
                            })?;
                            if count > MAX_CELLS as u64 {
                                return Err(AppError::validation(
                                    "Excel 共享文字数量超过 100000 个",
                                ));
                            }
                        }
                    }
                } else if e.local_name().as_ref() == "si" {
                    if depth != 1 {
                        return Err(AppError::validation("Excel 共享文字 si 结构损坏"));
                    }
                    strings += 1;
                    if strings > MAX_CELLS {
                        return Err(AppError::validation("Excel 实际共享文字数量超过 100000 个"));
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| AppError::validation("Excel 共享文字 XML 结构损坏"))?;
            }
            Event::DocType(_) => return Err(AppError::validation("Excel 共享文字 XML 不允许 DTD")),
            Event::Text(e) if depth == 0 && !e.trim().is_empty() => {
                return Err(AppError::validation("Excel 共享文字 XML 根结构损坏"))
            }
            Event::Eof => {
                if !root_seen || depth != 0 {
                    return Err(AppError::validation("Excel 共享文字 XML 未完整结束"));
                }
                return Ok(());
            }
            _ => (),
        }
    }
}

/// Calamine eagerly expands BIFF cells to ranges. Bound coordinates before it allocates.
/// This reads record headers only; Calamine remains the Excel value/formula parser.
fn bounded_xls(bytes: &[u8]) -> AppResult<()> {
    let mut compound =
        cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|e| parser_error("XLS 容器", e))?;
    let mut stream = if compound.exists("/Workbook") {
        compound.open_stream("/Workbook")?
    } else {
        compound.open_stream("/Book")?
    };
    let mut data = Vec::new();
    stream
        .by_ref()
        .take(MAX_EXPANDED as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > MAX_EXPANDED {
        return Err(AppError::validation("XLS 工作簿数据超过 40 MiB"));
    }
    let mut offset = 0;
    let mut largest_row = 0usize;
    let mut largest_col = 0usize;
    let mut count = 0;
    while offset + 4 <= data.len() {
        let kind = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let len = usize::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]]));
        offset += 4;
        let record = data
            .get(offset..offset + len)
            .ok_or_else(|| AppError::validation("XLS 记录长度损坏"))?;
        offset += len;
        if kind == 0x0200 {
            let (start_row, end_row, end_col) = match record.len() {
                14 => (
                    u32::from_le_bytes(record[..4].try_into().expect("checked length")),
                    u32::from_le_bytes(record[4..8].try_into().expect("checked length")),
                    u32::from(u16::from_le_bytes([record[10], record[11]])),
                ),
                10 => (
                    u32::from(u16::from_le_bytes([record[0], record[1]])),
                    u32::from(u16::from_le_bytes([record[2], record[3]])),
                    u32::from(u16::from_le_bytes([record[6], record[7]])),
                ),
                _ => return Err(AppError::validation("XLS 尺寸记录损坏")),
            };
            if start_row > end_row
                || u64::from(end_row.max(1)) * u64::from(end_col.max(1)) > MAX_CELLS as u64
            {
                return Err(AppError::validation(
                    "XLS 声明的单元格范围超过 100000 个或已损坏",
                ));
            }
        }
        if kind == 0x00fc
            && (record.len() < 8
                || u32::from_le_bytes(record[4..8].try_into().expect("checked length"))
                    > MAX_CELLS as u32)
        {
            return Err(AppError::validation(
                "XLS 共享文字数量超过 100000 个或已损坏",
            ));
        }
        if matches!(
            kind,
            0x0203 | 0x0204 | 0x00d6 | 0x0205 | 0x027e | 0x00fd | 0x00bd | 0x0006
        ) {
            if record.len() < 4 {
                return Err(AppError::validation("XLS 单元格记录损坏"));
            }
            largest_row =
                largest_row.max(usize::from(u16::from_le_bytes([record[0], record[1]])) + 1);
            let col = if kind == 0x00bd { record.len() - 2 } else { 2 };
            largest_col = largest_col
                .max(usize::from(u16::from_le_bytes([record[col], record[col + 1]])) + 1);
            count += 1;
            if largest_row.saturating_mul(largest_col) > MAX_CELLS || count > MAX_CELLS {
                return Err(AppError::validation(
                    "XLS 单元格坐标范围超过 100000 个，请缩小使用范围或另存为 XLSX",
                ));
            }
        }
    }
    Ok(())
}

fn pdf(bytes: &[u8]) -> AppResult<Parsed> {
    let document = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_EXPANDED),
            ..Default::default()
        },
    )
    .map_err(|e| parser_error("PDF", e))?;
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() > MAX_PAGES {
        return Err(AppError::validation("PDF 必须包含 1–100 页"));
    }
    let mut parsed = Parsed::default();
    let mut ocr_pages = Vec::new();
    let mut texts = std::collections::BTreeMap::new();
    let mut expanded = 0;
    for (number, page) in &pages {
        let content = document
            .get_page_content_with_limit(*page, MAX_EXPANDED - expanded)
            .map_err(|e| parser_error("PDF 内容", e))?;
        expanded += content.len();
        let text = document
            .extract_text_with_limit(&[*number], MAX_EXPANDED)
            .map_err(|e| parser_error("PDF 文字", e))?;
        if text.trim().is_empty() {
            let operations = lopdf::content::Content::decode(&content)
                .map_err(|e| parser_error("PDF 页面", e))?;
            let blank = operations.operations.iter().all(|operation| {
                matches!(
                    operation.operator.as_str(),
                    "q" | "Q"
                        | "cm"
                        | "BT"
                        | "ET"
                        | "Tf"
                        | "Td"
                        | "TD"
                        | "Tm"
                        | "T*"
                        | "Tc"
                        | "Tw"
                        | "Tz"
                        | "TL"
                        | "Tr"
                        | "Ts"
                        | "w"
                        | "J"
                        | "j"
                        | "M"
                        | "d"
                        | "ri"
                        | "i"
                        | "gs"
                        | "CS"
                        | "cs"
                        | "SC"
                        | "SCN"
                        | "sc"
                        | "scn"
                        | "G"
                        | "g"
                        | "RG"
                        | "rg"
                        | "K"
                        | "k"
                        | "m"
                        | "l"
                        | "c"
                        | "v"
                        | "y"
                        | "h"
                        | "re"
                        | "n"
                        | "W"
                        | "W*"
                )
            });
            if blank {
                parsed
                    .warnings
                    .push(format!("第 {number} 页为空白页，没有可提取内容。"));
            } else {
                ocr_pages.push(*number);
            }
        }
        check_text(&text)?;
        texts.insert(*number, text);
    }
    let recognized = if ocr_pages.is_empty() {
        Vec::new()
    } else {
        native_ocr(bytes, Some(&ocr_pages))?
    };
    for (number, text) in ocr_pages.iter().zip(recognized) {
        if text.trim().is_empty() {
            parsed.warnings.push(format!(
                "第 {number} 页 OCR 未识别到文字；请核对原页图片及识别语言。"
            ));
        }
        texts.insert(*number, text);
    }
    if texts.values().all(|text| text.trim().is_empty()) {
        return Err(AppError::validation(
            "PDF 未提取或识别到文字；请检查原文件内容、图片清晰度及 OCR 语言。原文件已保留",
        ));
    }
    for (number, text) in texts {
        append(&mut parsed.text, &format!("第 {number} 页\n{text}\n"))?;
    }
    if !ocr_pages.is_empty() {
        parsed
            .warnings
            .push("无文字层的页面使用 Windows 本机 OCR；请核对识别结果。".into());
    }
    parsed
        .warnings
        .push("PDF 原文件已保留；版式与内嵌图片未拆分。".into());
    Ok(parsed)
}

fn parse(asset: &ContentAsset) -> AppResult<Parsed> {
    let bytes = decode_asset(asset)?;
    let parsed = match asset.mime.as_str() {
        "text/plain" => Parsed {
            text: std::str::from_utf8(&bytes)
                .map_err(|_| AppError::validation("文本不是 UTF-8 编码"))?
                .trim_start_matches('\u{feff}')
                .into(),
            ..Default::default()
        },
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => word(&bytes)?,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => excel(&bytes, true)?,
        "application/vnd.ms-excel" => excel(&bytes, false)?,
        "application/pdf" => pdf(&bytes)?,
        mime if mime.starts_with("image/") => Parsed {
            text: native_ocr(&bytes, None)?.join("\n"),
            warnings: vec!["使用 Windows 本机 OCR；请核对识别结果。".into()],
            ..Default::default()
        },
        "application/msword" => {
            return Err(AppError::validation(
                "旧版 DOC 本机识别尚未实现，请另存为 DOCX 后导入；原文件已保留",
            ))
        }
        _ => {
            return Err(AppError::validation(
                "此格式的本机识别尚未实现；原文件已保留",
            ))
        }
    };
    check_text(&parsed.text)?;
    Ok(parsed)
}

pub fn extract_local(asset: &ContentAsset) -> AppResult<String> {
    Ok(parse(asset)?.text)
}

pub async fn prepare_extraction(asset: ContentAsset) -> AppResult<Extraction> {
    let parsed = tokio::task::spawn_blocking(move || parse(&asset))
        .await
        .map_err(|e| AppError::internal(format!("本机文件识别失败：{e}")))??;
    let mut images = Vec::new();
    for (name, bytes) in parsed.images {
        images.push(crate::content_assets::prepare_bytes(&name, bytes)?);
    }
    Ok(Extraction {
        text: parsed.text,
        images,
        warnings: parsed.warnings,
    })
}

pub async fn extract_asset(db: &Db, asset: ContentAsset) -> AppResult<Extraction> {
    let extracted = prepare_extraction(asset).await?;
    crate::content_assets::persist_assets(db, &extracted.images).await?;
    Ok(extracted)
}

#[tauri::command]
pub async fn content_asset_extract(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Extraction> {
    let asset = get_asset(&state.db, &id).await?;
    extract_asset(&state.db, asset).await
}

#[cfg(not(windows))]
fn native_ocr(_bytes: &[u8], _pages: Option<&[u32]>) -> AppResult<Vec<String>> {
    Err(AppError::validation(
        "本机图片/扫描 PDF OCR 需要 Windows；请提供带文字层的 PDF 或文本文件",
    ))
}

#[cfg(windows)]
fn native_ocr(bytes: &[u8], pages: Option<&[u32]>) -> AppResult<Vec<String>> {
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat},
        Media::Ocr::OcrEngine,
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
        Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    };
    // Called by spawn_blocking, independent of Tauri's UI COM apartment.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|e| parser_error("Windows OCR 初始化", e))?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let run = || -> windows::core::Result<Vec<String>> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.get()?;
        writer.DetachStream()?;
        stream.Seek(0)?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let recognize = |stream: &InMemoryRandomAccessStream| -> windows::core::Result<String> {
            stream.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(stream)?.get()?;
            let width = decoder.PixelWidth()?;
            let height = decoder.PixelHeight()?;
            if width > OcrEngine::MaxImageDimension()?
                || height > OcrEngine::MaxImageDimension()?
                || u64::from(width) * u64::from(height) > 16_000_000
            {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "图片尺寸超过 OCR 限制，请缩小后导入",
                ));
            }
            let bitmap = decoder
                .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore)?
                .get()?;
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let mut text = String::new();
            for line in result.Lines()? {
                text.push_str(&line.Text()?.to_string_lossy());
                text.push('\n');
            }
            Ok(text)
        };
        if let Some(pages) = pages {
            let pdf = PdfDocument::LoadFromStreamAsync(&stream)?.get()?;
            let mut texts = Vec::new();
            for number in pages {
                let page = pdf.GetPage(number - 1)?;
                let size = page.Size()?;
                let scale = (2000.0 / size.Width.max(size.Height)).min(2.0);
                let options = PdfPageRenderOptions::new()?;
                options.SetDestinationWidth((size.Width * scale).max(1.0) as u32)?;
                options.SetDestinationHeight((size.Height * scale).max(1.0) as u32)?;
                let raster = InMemoryRandomAccessStream::new()?;
                page.RenderWithOptionsToStreamAsync(&raster, &options)?
                    .get()?;
                texts.push(recognize(&raster)?);
                page.Close()?;
            }
            Ok(texts)
        } else {
            Ok(vec![recognize(&stream)?])
        }
    };
    let texts = run().map_err(|e| parser_error("Windows OCR（请检查已安装识别语言）", e))?;
    if pages.is_none() && texts.iter().all(|text| text.trim().is_empty()) {
        return Err(AppError::validation(
            "Windows OCR 未识别到文字；请检查图片清晰度、识别语言或提供文字文件。原文件已保留",
        ));
    }
    Ok(texts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content_assets::store_bytes;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};

    pub(crate) fn asset(name: &str, bytes: &[u8]) -> ContentAsset {
        ContentAsset {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.into(),
            mime: crate::content_assets::mime_for(name, bytes).into(),
            data_base64: STANDARD.encode(bytes),
            byte_size: bytes.len() as i64,
            sha256: hex::encode(Sha256::digest(bytes)),
            created_at: crate::db::now_stamp(),
        }
    }

    fn package(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, value) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(value.as_bytes()).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    pub(crate) fn docx() -> Vec<u8> {
        package(&[(
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>订单核对 &amp; 发货</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>SKU</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>数量</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        )])
    }

    #[test]
    fn actual_word_xml_preserves_paragraphs_tables_and_entities() {
        let text = extract_local(&asset("orders.docx", &docx())).unwrap();
        assert!(text.contains("订单核对 & 发货\n"));
        assert!(text.contains("SKU\t数量"), "{text:?}");
    }

    fn xlsx() -> Vec<u8> {
        package(&[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="订单" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:B2"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>SKU</t></is></c><c r="B1" t="inlineStr"><is><t>数量</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>ABC</t></is></c><c r="B2"><v>12</v></c></row></sheetData></worksheet>"#,
            ),
        ])
    }

    #[test]
    fn actual_excel_workbook_returns_sheet_names_and_cells() {
        let text = extract_local(&asset("orders.xlsx", &xlsx())).unwrap();
        assert!(text.contains("订单"));
        assert!(text.contains("SKU\t数量"));
        assert!(text.contains("ABC\t12"));
    }

    #[test]
    fn sparse_excel_columns_do_not_shift_values_under_wrong_headers() {
        let entries = bounded_archive(&xlsx()).unwrap();
        let strings: Vec<_> = entries.into_iter().map(|(name, bytes)| {
            let value = String::from_utf8(bytes).unwrap()
                .replace("<c r=\"B1\" t=\"inlineStr\"><is><t>数量</t></is></c>", "<c r=\"B1\" t=\"inlineStr\"><is><t>Seller</t></is></c><c r=\"C1\" t=\"inlineStr\"><is><t>数量</t></is></c>")
                .replace("<c r=\"B2\"><v>12</v></c>", "<c r=\"C2\"><v>12</v></c>");
            (name, value)
        }).collect();
        let references: Vec<_> = strings
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let text = extract_local(&asset("sparse.xlsx", &package(&references))).unwrap();
        assert!(text.contains("SKU\tSeller\t数量"));
        assert!(text.contains("ABC\t\t12"), "{text:?}");
    }

    fn pdf() -> Vec<u8> {
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::with_version("1.5");
        let pages = doc.new_object_id();
        let font = doc.add_object(
            dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
        );
        let resources = doc.add_object(dictionary! {"Font" => dictionary! {"F1" => font}});
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 18 Tf 50 700 Td (Ship order ABC) Tj ET".to_vec(),
        ));
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "Contents" => content, "Resources" => resources, "MediaBox" => vec![0.into(),0.into(),595.into(),842.into()]});
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
            ),
        );
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn actual_pdf_embedded_text_is_read() {
        assert!(extract_local(&asset("orders.pdf", &pdf()))
            .unwrap()
            .contains("Ship order ABC"));
    }

    fn pdf_with_blank_page() -> Vec<u8> {
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::load_mem(&pdf()).unwrap();
        let first = *doc.get_pages().get(&1).unwrap();
        let parent = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let empty = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let blank = doc.add_object(dictionary! {"Type" => "Page", "Parent" => parent, "Contents" => empty, "Resources" => dictionary! {}, "MediaBox" => vec![0.into(),0.into(),595.into(),842.into()]});
        doc.objects.insert(parent, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => vec![first.into(), blank.into()], "Count" => 2}));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn pdf_text_with_intentional_empty_second_page_preserves_useful_content() {
        let parsed = parse(&asset("text-and-blank.pdf", &pdf_with_blank_page())).unwrap();
        assert!(parsed.text.contains("Ship order ABC"));
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("第 2 页") && warning.contains("空白")));
    }

    fn xlsx_shared_strings(unique_count: &str) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in bounded_archive(&xlsx()).unwrap() {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive
            .start_file(
                "xl/sharedStrings.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" uniqueCount="{unique_count}"><si><t>SKU</t></si></sst>"#).as_bytes()).unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn relocated_workbook_shared_strings_are_bounded_before_allocation() {
        fn relocated(count: &str, backslashes: bool) -> Vec<u8> {
            let entries = bounded_archive(&xlsx_shared_strings(count)).unwrap();
            let renamed: Vec<(String, String)> = entries
                .iter()
                .map(|(name, bytes)| {
                    (
                        if backslashes {
                            name.replace("xl/", "custom/").replace('/', "\\")
                        } else {
                            name.replace("xl/", "custom/")
                        },
                        String::from_utf8(bytes.clone())
                            .unwrap()
                            .replace("xl/", "custom/"),
                    )
                })
                .collect();
            let references: Vec<(&str, &str)> = renamed
                .iter()
                .map(|(name, xml)| (name.as_str(), xml.as_str()))
                .collect();
            package(&references)
        }
        for backslashes in [false, true] {
            assert!(
                extract_local(&asset("valid-custom.xlsx", &relocated("1", backslashes)))
                    .unwrap()
                    .contains("ABC")
            );
            let error = extract_local(&asset("custom.xlsx", &relocated("100001", backslashes)))
                .unwrap_err();
            assert!(error.message.contains("100000"), "{error}");
        }
    }

    #[test]
    fn shared_strings_declared_count_is_bounded_before_calamine_allocates() {
        for count in ["100001", "18446744073709551615"] {
            let error =
                extract_local(&asset("count.xlsx", &xlsx_shared_strings(count))).unwrap_err();
            assert!(error.message.contains("100000"), "{error}");
        }
        let actual = format!("<sst>{}</sst>", "<si><t>x</t></si>".repeat(100001));
        assert!(bounded_shared_strings(actual.as_bytes())
            .unwrap_err()
            .message
            .contains("实际共享文字"));
    }

    fn xls(oversized: bool) -> Vec<u8> {
        fn record(target: &mut Vec<u8>, kind: u16, bytes: &[u8]) {
            target.extend_from_slice(&kind.to_le_bytes());
            target.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            target.extend_from_slice(bytes);
        }
        let mut data = Vec::new();
        let bof = [0x00, 0x06, 0x05, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        record(&mut data, 0x0809, &bof);
        record(&mut data, 0x0042, &1200u16.to_le_bytes());
        let boundsheet_offset = data.len() + 4;
        record(
            &mut data,
            0x0085,
            &[0, 0, 0, 0, 0, 0, 5, 0, b'O', b'r', b'd', b'e', b'r'],
        );
        record(&mut data, 0x000a, &[]);
        let sheet_offset = data.len() as u32;
        data[boundsheet_offset..boundsheet_offset + 4].copy_from_slice(&sheet_offset.to_le_bytes());
        let mut sheet_bof = bof;
        sheet_bof[2] = 0x10;
        record(&mut data, 0x0809, &sheet_bof);
        let rows = if oversized { 65_536u32 } else { 1 };
        let cols = if oversized { 256u16 } else { 2 };
        let mut dimensions = vec![0; 14];
        dimensions[4..8].copy_from_slice(&rows.to_le_bytes());
        dimensions[10..12].copy_from_slice(&cols.to_le_bytes());
        record(&mut data, 0x0200, &dimensions);
        let mut number = vec![0; 6];
        number.extend_from_slice(&12f64.to_le_bytes());
        record(&mut data, 0x0203, &number);
        record(
            &mut data,
            0x0204,
            &[0, 0, 1, 0, 0, 0, 3, 0, 0, b'S', b'K', b'U'],
        );
        record(&mut data, 0x000a, &[]);
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        compound
            .create_stream("/Workbook")
            .unwrap()
            .write_all(&data)
            .unwrap();
        compound.into_inner().into_inner()
    }

    #[test]
    fn actual_legacy_excel_and_preallocation_dimension_limit() {
        let text = extract_local(&asset("legacy.xls", &xls(false))).unwrap();
        assert!(text.contains("工作表：Order"));
        assert!(text.contains("12\tSKU"), "{text:?}");
        assert!(extract_local(&asset("large.xls", &xls(true)))
            .unwrap_err()
            .message
            .contains("100000"));
    }

    #[test]
    fn malformed_documents_legacy_word_and_output_overflow_fail_explicitly() {
        assert!(extract_local(&asset("broken.docx", b"PKbroken")).is_err());
        assert!(extract_local(&asset("broken.pdf", b"%PDF-broken")).is_err());
        assert!(extract_local(&asset("old.doc", b"legacy"))
            .unwrap_err()
            .message
            .contains("DOC"));
        assert!(extract_local(&asset("large.txt", "中".repeat(100_001).as_bytes())).is_err());
    }

    #[test]
    fn archive_expansion_and_sparse_excel_output_are_bounded() {
        let large = "a".repeat(MAX_EXPANDED + 1);
        let zip = package(&[("word/document.xml", &large)]);
        assert!(bounded_archive(&zip)
            .unwrap_err()
            .message
            .contains("40 MiB"));
        let mut invalid = asset("orders.docx", &docx());
        invalid.sha256 = "invalid".into();
        assert!(extract_local(&invalid).is_err());
    }

    #[tokio::test]
    async fn embedded_word_images_are_real_resources_and_original_stays_immutable() {
        let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jB9kAAAAASUVORK5CYII=").unwrap();
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"<w:document xmlns:w='urn:word'><w:p><w:r><w:t>Image example</w:t></w:r></w:p></w:document>").unwrap();
        archive
            .start_file(
                "word/media/image1.png",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(&png).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let dir = std::env::temp_dir().join(format!("lumen-extract-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let original = store_bytes(&db, "with-image.docx", bytes.clone())
            .await
            .unwrap();
        let extracted = extract_asset(&db, original.clone()).await.unwrap();
        assert_eq!(extracted.images.len(), 1);
        assert_eq!(
            decode_asset(&get_asset(&db, &extracted.images[0].id).await.unwrap()).unwrap(),
            png
        );
        assert_eq!(
            decode_asset(&get_asset(&db, &original.id).await.unwrap()).unwrap(),
            bytes
        );
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    fn printed_fixture() -> (Vec<u8>, Vec<u8>, u32, u32) {
        // Synthetic fixture: System.Drawing Bitmap(500,120), white background,
        // DrawString("ORDER 123", Font("Arial",40), black,10,10), PNG.
        // A printed font exercises ordinary OCR rather than ambiguous pixel glyphs.
        let bytes = STANDARD.decode("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Y0TNMXrp0p9ZXq8tzrW47V2W2ECXZ+kYywscST0LBDp+n7hAVw1np+EOqUOdH1DNRjjVbFVdzsEy/WvGQdW1t17r/Gvx2bss4nw51al+XTtG/yzNut+/nQAWa7F1PeA5BQZ6o0E6NZ0H8hvW6FDfN5V/YycUWtc8hr0hqmuMwnbO2t6ounbXa1VELfqax7DvXRYRYsP9G2uF+d66PlLzLM+hXk4d1gJ4NqGBLg/azsFtEs3tGMOJdAMffmykcd/2UFZ1WT9r0KhvfN+qLsvnIgTV0lgb1/V3IZ75zRCTL3lB9y3nWtw8da3rvYAB8WIDvXHYvJuEetsffzs/BIVC876tTVfV5bVm6to7dfms17y4Wvb2LotzoMtfuLyeOUG+OC3M53UdoIDwQJeN53NYAndMh7n1DT0uFFrXtl1ffdanSen1sl5b1WW5nwiRtehr+13fgzhXq4F+UijKZ3HqHhr7dJ1NAlIlBHqrYew3DdkQpg60bgjmj48073v0MqPq2lurdi3Wl5+DqstxvaYE19Jcr9HeZfELdHWO7M/EhtYam+5D3P/n2DwnwBPLCfSD/Enva8x24VYjOIapGdwEh0KjWY7vWdW11qia85vquKfq8l0vu/hammvnOckyEWhTz/6X3rn0GIOamv+80Xyp0Pt+G5fYGuAkeYF+GDaP/refZoO9jUHzeKSbg29TEI13OI+uy320C2hIquvb0E39oGuZvqWulb3LImqbfv4NZ2p3GGryqqH9rAC/Q26gH3rfrnfGUofNCIXei0zrBUbX5TnW7jG+rvvRbtK6lrX76pjeuywegS6u4T2MNfW+qVuG+74DTyg/0L94vZWv/hT9T1Io9O5XTqbr8hjtgLSIq6s12vXqWuRybprbuywijI3h+Z/my4rjmKpJ3NNonLoHwLWcFug3zWY5HDtBfpMXCr1m9TifrmtteKzTjWddtnGFQJ/buyyipqnwvGKg/1//G7vnMw3UcXqgfzcOdw4yAADKpQIdAACsIdABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAKINABACiAQAcAoAACHQCAAgh0AAAK+B+/QcaC9YWpiQAAAABJRU5ErkJggg==").unwrap();
        let mut decoder = png::Decoder::new(Cursor::new(&bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buffer).unwrap();
        let channels = frame.color_type.samples();
        let pixels = buffer[..frame.buffer_size()]
            .chunks(channels)
            .map(|pixel| pixel[0])
            .collect();
        (bytes, pixels, frame.width, frame.height)
    }
    #[cfg(windows)]
    #[test]
    fn native_windows_ocr_reads_generated_png_and_scanned_pdf() {
        use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.unwrap();
        struct TestApartment;
        impl Drop for TestApartment {
            fn drop(&mut self) {
                unsafe { RoUninitialize() };
            }
        }
        let _apartment = TestApartment;
        let (png, pixels, width, height) = printed_fixture();
        let language_count = windows::Media::Ocr::OcrEngine::AvailableRecognizerLanguages()
            .unwrap()
            .Size()
            .unwrap();
        if language_count == 0 {
            let error = extract_local(&asset("printed.png", &png)).unwrap_err();
            assert!(error.message.contains("Windows OCR") && error.message.contains("语言"));
            eprintln!("本机没有安装 OCR 识别语言：已验证明确错误路径；本轮未验证识别文字。");
            return;
        }
        let image_text = extract_local(&asset("printed.png", &png)).unwrap();
        assert!(image_text.contains("ORDER"), "{image_text:?}");
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::with_version("1.5");
        let pages = doc.new_object_id();
        let raster = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => i64::from(width), "Height" => i64::from(height), "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8}, pixels));
        let resources = doc.add_object(dictionary! {"XObject" => dictionary! {"Im1" => raster}});
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("q {width} 0 0 {height} 0 0 cm /Im1 Do Q").into_bytes(),
        ));
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "Contents" => content, "Resources" => resources, "MediaBox" => vec![0.into(),0.into(),i64::from(width).into(),i64::from(height).into()]});
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
            ),
        );
        let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let pdf_text = extract_local(&asset("scanned.pdf", &bytes)).unwrap();
        assert!(pdf_text.contains("ORDER"), "{pdf_text:?}");
    }
}
