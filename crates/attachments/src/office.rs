use crate::{REJECTED, Result, validate_image};
use quick_xml::{Reader, events::Event};
use std::{
    collections::HashSet,
    io::{Cursor, Read},
};

const EXPANDED_LIMIT: usize = 32 * 1024 * 1024;

pub fn validate(bytes: &[u8], excel: bool) -> Result<()> {
    if !bytes.starts_with(b"PK\x03\x04")
        || bytes.len() < 22
        || &bytes[bytes.len() - 22..bytes.len() - 18] != b"PK\x05\x06"
        || bytes[bytes.len() - 2..] != [0, 0]
    {
        return Err(REJECTED);
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| REJECTED)?;
    if zip.len() > 256 {
        return Err(REJECTED);
    }
    let mut total = 0;
    let mut seen = HashSet::new();
    let mut main = false;
    let mut content_types = false;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|_| REJECTED)?;
        let name = file.name().map_err(|_| REJECTED)?.to_ascii_lowercase();
        if !seen.insert(name.clone())
            || file.enclosed_name().is_none()
            || name.contains('\\')
            || name.split('/').any(|p| p == "..")
            || file.encrypted()
            || file
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            || ["vba", "activex", "embeddings", "externallinks", "customui"]
                .iter()
                .any(|s| name.contains(s))
            || file.size() > EXPANDED_LIMIT as u64
            || file.size() > file.compressed_size().max(1).saturating_mul(100)
        {
            return Err(REJECTED);
        }
        if file.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        file.by_ref()
            .take((EXPANDED_LIMIT - total + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| REJECTED)?;
        total += data.len();
        if total > EXPANDED_LIMIT {
            return Err(REJECTED);
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            inspect_xml(&data)?;
            if name == "[content_types].xml" {
                let value = std::str::from_utf8(&data).map_err(|_| REJECTED)?;
                let expected = if excel {
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
                } else {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
                };
                content_types = value.contains(expected);
                let lower = value.to_ascii_lowercase();
                if ["macroenabled", "vbaproject", "oleobject", "activex"]
                    .iter()
                    .any(|s| lower.contains(s))
                {
                    return Err(REJECTED);
                }
            }
            if name
                == if excel {
                    "xl/workbook.xml"
                } else {
                    "word/document.xml"
                }
            {
                main = true;
            }
        } else if name.ends_with(".png") {
            validate_image(&data, image::ImageFormat::Png)?;
        } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
            validate_image(&data, image::ImageFormat::Jpeg)?;
        } else {
            return Err(REJECTED);
        }
    }
    if !main || !content_types {
        return Err(REJECTED);
    }
    Ok(())
}

fn inspect_xml(data: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(data);
    let mut depth = 0usize;
    let mut events = 0;
    let mut roots = 0;
    let mut formula: Option<String> = None;
    loop {
        events += 1;
        if events > 1_000_000 || depth > 64 {
            return Err(REJECTED);
        }
        let event = reader.read_event().map_err(|_| REJECTED)?;
        let starts = matches!(&event, Event::Start(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if depth == 0 {
                    roots += 1;
                }
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase();
                if [
                    "oleobject",
                    "object",
                    "altchunk",
                    "control",
                    "externaldata",
                    "instrtext",
                    "fldsimple",
                    "fldchar",
                ]
                .contains(&name.as_str())
                {
                    return Err(REJECTED);
                }
                if formula.is_some() {
                    return Err(REJECTED);
                }
                if name == "f" && starts {
                    formula = Some(String::new());
                }
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| REJECTED)?;
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                        .map_err(|_| REJECTED)?
                        .to_ascii_lowercase();
                    let key = String::from_utf8_lossy(attr.key.local_name().as_ref())
                        .to_ascii_lowercase();
                    if (name == "c" && key == "r" || name == "dimension" && key == "ref")
                        && !value.split(':').all(cell_reference)
                    {
                        return Err(REJECTED);
                    }
                    if name == "row"
                        && key == "r"
                        && !value
                            .parse::<u32>()
                            .is_ok_and(|r| (1..=10_000).contains(&r))
                    {
                        return Err(REJECTED);
                    }
                    if (key == "targetmode" && value == "external")
                        || (key == "target" && (value.contains(':') || value.starts_with("//")))
                    {
                        return Err(REJECTED);
                    }
                }
                // Only Start increases depth; Empty is balanced immediately.
                if starts {
                    depth += 1;
                }
            }
            Event::End(e) => {
                if e.local_name().as_ref() == b"f"
                    && let Some(value) = formula.take()
                {
                    let value: String = value
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if [
                        "webservice(",
                        "hyperlink(",
                        "register.id(",
                        "call(",
                        "rtd(",
                        "|",
                        "[",
                        "file:",
                        "http:",
                        "https:",
                    ]
                    .iter()
                    .any(|s| value.contains(s))
                    {
                        return Err(REJECTED);
                    }
                }
                depth = depth.checked_sub(1).ok_or(REJECTED)?;
            }
            Event::DocType(_) | Event::PI(_) => return Err(REJECTED),
            Event::Text(e) => {
                let text = e
                    .xml10_content()
                    .map_err(|_| REJECTED)?
                    .to_ascii_lowercase();
                if depth == 0 && !text.trim().is_empty() {
                    return Err(REJECTED);
                }
                if let Some(value) = &mut formula {
                    value.push_str(&text);
                }
                if [
                    "webservice(",
                    "hyperlink(",
                    "register.id(",
                    "call(",
                    "cmd|",
                    "powershell|",
                ]
                .iter()
                .any(|s| text.contains(s))
                {
                    return Err(REJECTED);
                }
            }
            Event::GeneralRef(e) => {
                let value = reference(&e)?;
                if depth == 0 {
                    return Err(REJECTED);
                }
                if let Some(text) = &mut formula {
                    text.push(value);
                }
            }
            Event::CData(e) => {
                if let Some(value) = &mut formula {
                    value.push_str(&e.xml10_content().map_err(|_| REJECTED)?);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || roots != 1 {
        return Err(REJECTED);
    }
    Ok(())
}

fn reference(e: &quick_xml::events::BytesRef<'_>) -> Result<char> {
    if let Some(value) = e.resolve_char_ref().map_err(|_| REJECTED)? {
        return Ok(value);
    }
    match e.decode().map_err(|_| REJECTED)?.as_ref() {
        "amp" => Ok('&'),
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        _ => Err(REJECTED),
    }
}

fn cell_reference(value: &str) -> bool {
    let value = value.replace('$', "");
    let split = value
        .bytes()
        .position(|b| b.is_ascii_digit())
        .unwrap_or(value.len());
    if split == 0 || split > 3 {
        return false;
    }
    let mut column = 0u32;
    for byte in value[..split].bytes() {
        if !byte.is_ascii_alphabetic() {
            return false;
        }
        column = column * 26 + u32::from(byte.to_ascii_lowercase() - b'a' + 1);
    }
    column <= 256
        && value[split..]
            .parse::<u32>()
            .is_ok_and(|row| (1..=10_000).contains(&row))
}

pub fn text(bytes: &[u8]) -> Result<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| REJECTED)?;
    let mut data = Vec::new();
    zip.by_name("word/document.xml")
        .map_err(|_| REJECTED)?
        .take(EXPANDED_LIMIT as u64)
        .read_to_end(&mut data)
        .map_err(|_| REJECTED)?;
    let mut reader = Reader::from_reader(data.as_slice());
    let mut output = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(|_| REJECTED)? {
            Event::Start(e) => {
                in_text = e.local_name().as_ref() == b"t";
            }
            Event::End(e) => {
                if e.local_name().as_ref() == b"t" {
                    in_text = false;
                }
                if e.local_name().as_ref() == b"p" {
                    output.push('\n');
                }
            }
            Event::Text(e) if in_text => {
                let text = e.xml10_content().map_err(|_| REJECTED)?;
                output.push_str(&quick_xml::escape::unescape(&text).map_err(|_| REJECTED)?);
            }
            Event::GeneralRef(e) if in_text => output.push(reference(&e)?),
            Event::CData(e) if in_text => {
                output.push_str(&e.xml10_content().map_err(|_| REJECTED)?)
            }
            Event::Eof => break,
            _ => {}
        }
        if output.len() > 256_000 {
            break;
        }
    }
    Ok(output)
}
