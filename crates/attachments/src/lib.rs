//! Bounded structural checks shared by server uploads and desktop imports.
//! These checks complement mandatory server-side antivirus scanning.
mod office;
mod pdf;
use std::io::Cursor;

pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
pub const REJECTED: &str = "Use a safe PNG, JPEG, PDF, DOCX, XLSX, TXT, Markdown or CSV file; active, encrypted and unsupported files are blocked.";
pub type Result<T> = std::result::Result<T, &'static str>;

pub fn declared_type(name: &str) -> Result<&'static str> {
    if name.is_empty()
        || name.len() > 200
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '\u{202e}' | '\u{202d}'))
    {
        return Err(REJECTED);
    }
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        Some("pdf") => Ok("application/pdf"),
        Some("docx") => Ok(DOCX),
        Some("xlsx") => Ok(XLSX),
        Some("txt" | "md" | "csv") => Ok("text/plain"),
        _ => Err(REJECTED),
    }
}

pub fn validate(name: &str, claimed: Option<&str>, bytes: &[u8]) -> Result<&'static str> {
    if bytes.is_empty()
        || bytes.len() > MAX_BYTES
        || name.is_empty()
        || name.len() > 200
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '\u{202e}' | '\u{202d}'))
    {
        return Err(REJECTED);
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .ok_or(REJECTED)?;
    let mime = match extension.as_str() {
        "png" => {
            validate_image(bytes, image::ImageFormat::Png)?;
            "image/png"
        }
        "jpg" | "jpeg" => {
            validate_image(bytes, image::ImageFormat::Jpeg)?;
            "image/jpeg"
        }
        "pdf" => {
            pdf::validate(bytes)?;
            "application/pdf"
        }
        "docx" => {
            office::validate(bytes, false)?;
            DOCX
        }
        "xlsx" => {
            office::validate(bytes, true)?;
            XLSX
        }
        "txt" | "md" | "csv" => {
            let text = std::str::from_utf8(bytes).map_err(|_| REJECTED)?;
            if text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                || bytes.starts_with(b"MZ")
                || bytes.starts_with(b"PK\x03\x04")
                || text
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("<!doctype html")
                || text.trim_start().to_ascii_lowercase().starts_with("<html")
            {
                return Err(REJECTED);
            }
            "text/plain"
        }
        _ => return Err(REJECTED),
    };
    if claimed.is_some_and(|value| value != mime) {
        return Err(REJECTED);
    }
    Ok(mime)
}

pub fn validate_image(bytes: &[u8], expected: image::ImageFormat) -> Result<()> {
    if image::guess_format(bytes).map_err(|_| REJECTED)? != expected {
        return Err(REJECTED);
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), expected);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| REJECTED)?;
    if (expected == image::ImageFormat::Jpeg && !bytes.ends_with(b"\xff\xd9"))
        || (expected == image::ImageFormat::Png && !bytes.ends_with(b"IEND\xaeB`\x82"))
    {
        return Err(REJECTED);
    }
    Ok(())
}

pub fn docx_text(bytes: &[u8]) -> Result<String> {
    office::validate(bytes, false)?;
    office::text(bytes)
}
