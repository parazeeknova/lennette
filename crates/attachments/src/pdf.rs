use crate::{REJECTED, Result};
use lopdf::Object;
use std::cell::Cell;
thread_local! { static LOAD: Cell<(usize, usize, bool)> = const { Cell::new((0, 0, false)) }; }

fn filter(id: (u32, u16), object: &mut Object) -> Option<((u32, u16), Object)> {
    let (mut bytes, count, failed) = LOAD.get();
    if failed || count == 0 {
        LOAD.set((bytes, 0, true));
        return None;
    }
    if let Object::Stream(stream) = object
        && stream.dict.has_type(b"ObjStm")
    {
        match stream.decompressed_content_with_limit(bytes.min(1024 * 1024)) {
            Ok(decoded) => bytes = bytes.saturating_sub(decoded.len()),
            Err(_) => {
                LOAD.set((0, 0, true));
                return None;
            }
        }
    }
    LOAD.set((bytes, count - 1, false));
    Some((id, object.clone()))
}

pub fn validate(bytes: &[u8]) -> Result<()> {
    if !bytes.starts_with(b"%PDF-") || !bytes.trim_ascii_end().ends_with(b"%%EOF") {
        return Err(REJECTED);
    }
    let options = lopdf::LoadOptions {
        max_decompressed_size: Some(1024 * 1024),
        strict: true,
        filter: Some(filter),
        ..Default::default()
    };
    LOAD.set((16 * 1024 * 1024, 50_000, false));
    let doc = lopdf::Document::load_mem_with_options(bytes, options).map_err(|_| REJECTED)?;
    if LOAD.get().2 || doc.is_encrypted() || doc.objects.len() > 50_000 {
        return Err(REJECTED);
    }
    let mut budget = 1_000_000usize;
    for object in doc.objects.values() {
        check(object, 0, &mut budget)?;
    }
    Ok(())
}

fn check(object: &Object, depth: usize, budget: &mut usize) -> Result<()> {
    if depth > 64 || *budget == 0 {
        return Err(REJECTED);
    }
    *budget -= 1;
    match object {
        Object::Name(name)
            if [
                b"JavaScript".as_slice(),
                b"Launch",
                b"SubmitForm",
                b"ImportData",
                b"GoToR",
                b"Rendition",
                b"RichMedia",
                b"Filespec",
            ]
            .contains(&name.as_slice()) =>
        {
            return Err(REJECTED);
        }
        Object::Dictionary(dict) => {
            for (key, value) in dict.iter() {
                if [
                    b"JS".as_slice(),
                    b"JavaScript",
                    b"OpenAction",
                    b"AA",
                    b"EmbeddedFiles",
                    b"EF",
                    b"XFA",
                    b"RichMedia",
                ]
                .contains(&key.as_slice())
                {
                    return Err(REJECTED);
                }
                check(value, depth + 1, budget)?;
            }
        }
        Object::Array(values) => {
            for value in values {
                check(value, depth + 1, budget)?;
            }
        }
        Object::Stream(stream) => {
            check(&Object::Dictionary(stream.dict.clone()), depth + 1, budget)?;
        }
        _ => {}
    }
    Ok(())
}
