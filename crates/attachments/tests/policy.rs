use lennette_attachments::{DOCX, XLSX, docx_text, validate};
use std::io::{Cursor, Write};

fn office(excel: bool, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let kind = if excel {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
    } else {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
    };
    let types = format!("<Types><Override ContentType=\"{kind}\"/></Types>");
    writer.start_file("[Content_Types].xml", options).unwrap();
    writer.write_all(types.as_bytes()).unwrap();
    writer
        .start_file(
            if excel {
                "xl/workbook.xml"
            } else {
                "word/document.xml"
            },
            options,
        )
        .unwrap();
    writer.write_all(b"<w:document xmlns:w=\"urn:test\"><w:body><w:p><w:r><w:t>Meeting agenda</w:t></w:r></w:p></w:body></w:document>").unwrap();
    for (name, bytes) in extra {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn only_declared_meeting_types_and_matching_contents_are_accepted() {
    assert!(
        validate(
            "notes.txt",
            Some("text/plain"),
            b"Agenda\nBudget discussion"
        )
        .is_ok()
    );
    for name in [
        "payload.exe",
        "archive.zip",
        "macro.docm",
        "legacy.doc",
        "legacy.xls",
        "script.js",
        "report.pdf.exe",
        "../notes.txt",
        "a\r\n.txt",
    ] {
        assert!(validate(name, None, b"hello").is_err(), "{name}");
    }
    assert!(validate("notes.txt", None, b"MZ executable").is_err());
    assert!(validate("notes.txt", None, b"hello\0binary").is_err());
    assert!(validate("notes.txt", Some("application/pdf"), b"hello").is_err());
    assert!(validate("notes.png", Some("image/png"), b"hello").is_err());
}

#[test]
fn valid_images_reject_trailing_payload_and_excessive_dimensions() {
    let mut data = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(8, 8)
        .write_to(&mut data, image::ImageFormat::Png)
        .unwrap();
    assert!(validate("slide.png", Some("image/png"), data.get_ref()).is_ok());
    data.get_mut().extend_from_slice(b"MZ executable");
    assert!(validate("slide.png", None, data.get_ref()).is_err());
    let mut big = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(5000, 1)
        .write_to(&mut big, image::ImageFormat::Png)
        .unwrap();
    assert!(validate("slide.png", None, big.get_ref()).is_err());
}

#[test]
fn valid_word_and_excel_are_recognized_and_word_text_is_readable() {
    let word = office(false, &[]);
    assert_eq!(validate("agenda.docx", Some(DOCX), &word).unwrap(), DOCX);
    assert!(docx_text(&word).unwrap().contains("Meeting agenda"));
    assert_eq!(
        validate("budget.xlsx", Some(XLSX), &office(true, &[])).unwrap(),
        XLSX
    );
    assert!(validate("archive.zip", None, &word).is_err());
    assert!(validate("budget.xlsx", Some(XLSX), &word).is_err());
}

#[test]
fn office_rejects_macros_embedded_binaries_external_links_and_xml_entities() {
    for (name, body) in [
        ("word/vbaProject.bin", b"macro".as_slice()),
        ("word/embeddings/oleObject1.bin", b"MZ"),
        ("word/activeX/activeX1.xml", b"<control/>"),
        ("word/_rels/document.xml.rels", b"<Relationships><Relationship TargetMode=\"Ex&#116;ernal\" Target=\"file:///etc/passwd\"/></Relationships>"),
        ("word/more.xml", b"<!DOCTYPE r [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><r>&x;</r>"),
        ("word/extra.xml", b"<r><altChunk/></r>"),
        ("word/cells.xml", b"<r><f>WEBSERVICE(1)</f></r>"),
        ("word/encoded.xml", b"<r><f>WEBSER&#86;ICE(1)</f></r>"),
        ("word/field.xml", b"<r><instrText>DDEAUTO</instrText></r>"),
        ("word/dimensions.xml", b"<worksheet><dimension ref=\"A1:XFD1048576\"/></worksheet>"),
        ("../escape.xml", b"<r/>"),
    ] {
        assert!(validate("agenda.docx", None, &office(false, &[(name, body)])).is_err(), "{name}");
    }
}

#[test]
fn office_rejects_compression_bombs_without_expanding_them() {
    let bomb = vec![b'A'; 2 * 1024 * 1024];
    assert!(
        validate(
            "agenda.docx",
            None,
            &office(false, &[("word/bomb.xml", &bomb)])
        )
        .is_err()
    );
}

fn pdf(active: bool) -> Vec<u8> {
    use lopdf::{Document, Object, dictionary};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 },
        ),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    if active {
        doc.add_object(dictionary! { "S" => "JavaScript", "JS" => "app.alert(1)" });
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn pdf_requires_valid_structure_and_rejects_scripts_and_appended_executables() {
    assert!(validate("notes.pdf", None, &pdf(false)).is_ok());
    assert!(validate("notes.pdf", None, &pdf(true)).is_err());
    let mut bytes = pdf(false);
    bytes.extend_from_slice(b"MZ payload");
    assert!(validate("notes.pdf", None, &bytes).is_err());
    assert!(validate("notes.pdf", None, b"%PDF-1.7\n%%EOF").is_err());
}
