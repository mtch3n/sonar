//! The text inside PDF, Word, Excel, PowerPoint and OpenDocument files.
//!
//! These parsers run on files from anywhere, so each file is read on its own
//! thread with a deadline: a panic in a parser is caught, and a file that takes
//! too long is given up on instead of stalling the scan.

use std::{
    fmt,
    fs::{self, File},
    io::{BufReader, Read},
    panic::{self, AssertUnwindSafe},
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use calamine::{DataType, Reader, Xlsx};
use pdf_extract::{ConvertToFmt, Document, PlainTextOutput};
use quick_xml::{escape::resolve_predefined_entity, events::Event};
use zip::ZipArchive;

pub(crate) const EXTS: &[&str] = &[
    "pdf", "docx", "xlsx", "xls", "xlsb", "pptx", "odt", "ods", "odp",
];

/// Larger files are skipped. Only the start of their text is kept, and a document
/// this big is mostly images, fonts or embedded media, while PDFs are loaded whole
/// into memory to be read, so bigger files would cost much and add little.
const MAX_SIZE: u64 = 20 * 1024 * 1024;
/// How long reading one file may take. Documents that are read to the default text
/// limit finish in well under a second; with a high limit, what was read by then is kept.
const TIME: Duration = Duration::from_secs(5);
/// How much of an OpenDocument manifest is read to look for encryption.
const MANIFEST_LIMIT: u64 = 64 * 1024;

/// Up to `limit` bytes of a document's text, or `None` when it's too big, broken,
/// encrypted or empty.
pub(crate) fn read(path: &Path, ext: &str, limit: usize) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_SIZE {
        return None;
    }
    let (path, ext) = (path.to_owned(), ext.to_owned());
    let (tx, rx) = mpsc::channel();
    let deadline = Instant::now() + TIME;
    // The text read before a parser panics is kept. A thread stuck in a parser
    // past the deadline is left behind.
    thread::Builder::new()
        .name("sonar-documents".into())
        .spawn(move || {
            let mut text = Text::new(limit, deadline);
            let _ = panic::catch_unwind(AssertUnwindSafe(|| extract(&path, &ext, &mut text)));
            let _ = tx.send(text.finish());
        })
        .ok()?;
    rx.recv_timeout(TIME + Duration::from_secs(1))
        .ok()
        .flatten()
}

fn extract(path: &Path, ext: &str, text: &mut Text) {
    match ext {
        "pdf" => pdf(path, text),
        "docx" => zipped(path, text, |zip, text| {
            xml(zip.by_name("word/document.xml").ok()?, &WORD, text);
            Some(())
        }),
        "pptx" => zipped(path, text, |zip, text| {
            let mut slides: Vec<(u32, String)> = zip
                .file_names()
                .filter_map(|name| {
                    let number = name
                        .strip_prefix("ppt/slides/slide")?
                        .strip_suffix(".xml")?
                        .parse()
                        .ok()?;
                    Some((number, name.to_owned()))
                })
                .collect();
            slides.sort();
            for (_, name) in slides {
                xml(zip.by_name(&name).ok()?, &SLIDES, text);
                text.push("\n");
            }
            Some(())
        }),
        "odt" | "ods" | "odp" => zipped(path, text, |zip, text| {
            // An encrypted file's content is still in the zip, as noise.
            let mut manifest = String::new();
            zip.by_name("META-INF/manifest.xml")
                .ok()?
                .take(MANIFEST_LIMIT)
                .read_to_string(&mut manifest)
                .ok()?;
            if manifest.contains("encryption-data") {
                return None;
            }
            xml(zip.by_name("content.xml").ok()?, &OPEN_DOCUMENT, text);
            Some(())
        }),
        "xlsx" => sheets(path, text),
        "xls" | "xlsb" => old_sheets(path, text),
        _ => {}
    }
}

/// The text a document is read into, which stops taking more once it's full or the
/// deadline has passed.
struct Text {
    text: String,
    limit: usize,
    full: bool,
    deadline: Instant,
}

impl Text {
    fn new(limit: usize, deadline: Instant) -> Text {
        Text {
            text: String::new(),
            limit,
            full: false,
            deadline,
        }
    }

    /// Adds `s`, and returns whether there is room for more.
    fn push(&mut self, s: &str) -> bool {
        if self.full || Instant::now() > self.deadline {
            return false;
        }
        let room = self.limit - self.text.len();
        if s.len() <= room {
            self.text.push_str(s);
            return true;
        }
        let mut cut = room;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        self.text.push_str(&s[..cut]);
        self.full = true;
        false
    }

    /// The text with blank lines and the space around lines taken out.
    fn finish(self) -> Option<String> {
        let text = self
            .text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        (!text.is_empty()).then_some(text)
    }
}

impl fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.push(s) {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

/// Lets pdf-extract write into `Text`, so it stops once the text is full.
impl<'a> ConvertToFmt for &'a mut Text {
    type Writer = &'a mut Text;
    fn convert(self) -> &'a mut Text {
        self
    }
}

fn pdf(path: &Path, text: &mut Text) {
    let Ok(mut doc) = Document::load(path) else {
        return;
    };
    // Many PDFs are encrypted only to set permissions, with an empty password.
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return;
    }
    // Stops with an error once the text is full; what was read by then is kept.
    let _ = pdf_extract::output_doc(&doc, &mut PlainTextOutput::new(text));
}

fn zipped(
    path: &Path,
    text: &mut Text,
    read: impl FnOnce(&mut ZipArchive<BufReader<File>>, &mut Text) -> Option<()>,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    if let Ok(mut zip) = ZipArchive::new(BufReader::new(file)) {
        read(&mut zip, text);
    }
}

/// Spreadsheet cells, a row per line.
/// Sheets of older Excel files, `.xls` and `.xlsb`, a row per line. These are read
/// whole, as the format has no cell-by-cell reader.
fn old_sheets(path: &Path, text: &mut Text) {
    let Ok(mut book) = calamine::open_workbook_auto(path) else {
        return;
    };
    for name in book.sheet_names() {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .filter_map(|cell| cell.as_string())
                .filter(|cell| !cell.is_empty())
                .collect();
            if !cells.is_empty() && (!text.push(&cells.join("\t")) || !text.push("\n")) {
                return;
            }
        }
    }
}

fn sheets(path: &Path, text: &mut Text) {
    let Ok(mut book) = calamine::open_workbook::<Xlsx<_>, _>(path) else {
        return;
    };
    for name in book.sheet_names() {
        // Cells are read one at a time, since a sheet read whole is as big as its
        // last row and column, whatever lies between.
        let Ok(mut cells) = book.worksheet_cells_reader(&name) else {
            continue;
        };
        let mut row = None;
        while let Ok(Some(cell)) = cells.next_cell() {
            let Some(value) = cell.get_value().as_string() else {
                continue;
            };
            let at = cell.get_position().0;
            let gap = if row == Some(at) { "\t" } else { "\n" };
            row = Some(at);
            if !text.push(gap) || !text.push(&value) {
                return;
            }
        }
        text.push("\n");
    }
}

/// Which elements of a document's XML hold its text, and which end its paragraphs,
/// table cells and rows.
struct Markup {
    /// The element text is in, or `None` when all text counts.
    text: Option<&'static [u8]>,
    paragraphs: &'static [&'static [u8]],
    cell: &'static [u8],
    row: &'static [u8],
    spaces: &'static [&'static [u8]],
    breaks: &'static [&'static [u8]],
}

const WORD: Markup = Markup {
    text: Some(b"w:t"),
    paragraphs: &[b"w:p"],
    cell: b"w:tc",
    row: b"w:tr",
    spaces: &[b"w:tab"],
    breaks: &[b"w:br", b"w:cr"],
};

const SLIDES: Markup = Markup {
    text: Some(b"a:t"),
    paragraphs: &[b"a:p"],
    cell: b"a:tc",
    row: b"a:tr",
    spaces: &[],
    breaks: &[b"a:br"],
};

const OPEN_DOCUMENT: Markup = Markup {
    text: None,
    paragraphs: &[b"text:p", b"text:h"],
    cell: b"table:table-cell",
    row: b"table:table-row",
    spaces: &[b"text:s", b"text:tab"],
    breaks: &[b"text:line-break"],
};

/// Reads the text out of `xml`: a line per paragraph, and a line per table row with
/// its cells split by tabs.
fn xml(xml: impl Read, markup: &Markup, text: &mut Text) {
    let mut reader = quick_xml::Reader::from_reader(BufReader::new(xml));
    let mut buf = Vec::new();
    let mut in_text = 0;
    let mut in_cell = 0;
    loop {
        let more = match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                if Some(name.as_ref()) == markup.text {
                    in_text += 1;
                } else if name.as_ref() == markup.cell {
                    in_cell += 1;
                }
                true
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name = name.as_ref();
                if Some(name) == markup.text {
                    in_text -= 1;
                    true
                } else if markup.paragraphs.contains(&name) {
                    text.push(if in_cell > 0 { " " } else { "\n" })
                } else if name == markup.cell {
                    in_cell -= 1;
                    text.push("\t")
                } else if name == markup.row {
                    text.push("\n")
                } else {
                    true
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if markup.spaces.contains(&name.as_ref()) {
                    text.push(" ")
                } else if markup.breaks.contains(&name.as_ref()) {
                    text.push(if in_cell > 0 { " " } else { "\n" })
                } else {
                    true
                }
            }
            Ok(Event::Text(e)) if in_text > 0 || markup.text.is_none() => {
                e.decode().is_ok_and(|s| text.push(&s))
            }
            Ok(Event::CData(e)) if in_text > 0 || markup.text.is_none() => {
                e.decode().is_ok_and(|s| text.push(&s))
            }
            Ok(Event::GeneralRef(e)) if in_text > 0 || markup.text.is_none() => {
                let c = match e.resolve_char_ref() {
                    Ok(Some(c)) => Some(c),
                    _ => e
                        .decode()
                        .ok()
                        .and_then(|name| resolve_predefined_entity(&name))
                        .and_then(|s| s.chars().next()),
                };
                c.is_none_or(|c| text.push(c.encode_utf8(&mut [0; 4])))
            }
            Ok(Event::Eof) | Err(_) => false,
            Ok(_) => true,
        };
        if !more {
            return;
        }
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::PathBuf};

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::text::DEFAULT_TEXT_LIMIT;

    fn zip(dir: &Path, name: &str, entries: &[(&str, &str)]) -> PathBuf {
        let path = dir.join(name);
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        for (name, content) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    /// A one-page PDF with a line of Helvetica for each of `lines`.
    fn pdf(lines: &[&str]) -> Vec<u8> {
        let content: String = lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("BT /F1 12 Tf 72 {} Td ({line}) Tj ET\n", 720 - 20 * i))
            .collect();
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
                .to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    fn word(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        )
    }

    fn slide(paragraphs: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    }

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;

    fn open_document(dir: &Path, name: &str, manifest: &str, body: &str) -> PathBuf {
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><office:body>{body}</office:body></office:document-content>"#
        );
        zip(
            dir,
            name,
            &[
                ("META-INF/manifest.xml", manifest),
                ("content.xml", &content),
            ],
        )
    }

    #[test]
    fn reads_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        fs::write(
            &path,
            pdf(&["Quarterly budget review", "Travel and lodging"]),
        )
        .unwrap();
        assert_eq!(
            read(&path, "pdf", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Quarterly budget review\nTravel and lodging")
        );
    }

    #[test]
    fn reads_word() {
        let dir = tempfile::tempdir().unwrap();
        let body = word(
            "<w:p><w:r><w:t>Quarterly </w:t></w:r><w:r><w:t>budget</w:t></w:r></w:p>\
             <w:p><w:r><w:t>Tom &amp; Jerry</w:t><w:tab/><w:t>&#8364;5</w:t></w:r></w:p>\
             <w:p><w:r><w:instrText>PAGE</w:instrText></w:r></w:p>\
             <w:tbl><w:tr>\
             <w:tc><w:p><w:r><w:t>Item</w:t></w:r></w:p><w:p><w:r><w:t>name</w:t></w:r></w:p></w:tc>\
             <w:tc><w:p><w:r><w:t>Cost</w:t></w:r></w:p></w:tc>\
             </w:tr><w:tr>\
             <w:tc><w:p><w:r><w:t>Paper</w:t></w:r></w:p></w:tc>\
             <w:tc><w:p><w:r><w:t>12</w:t></w:r></w:p></w:tc>\
             </w:tr></w:tbl>",
        );
        let path = zip(dir.path(), "notes.docx", &[("word/document.xml", &body)]);
        assert_eq!(
            read(&path, "docx", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Quarterly budget\nTom & Jerry €5\nItem name \tCost\nPaper \t12")
        );
    }

    #[test]
    fn reads_slides_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let first = slide(
            "<a:p><a:r><a:t>Roadmap</a:t></a:r></a:p>\
             <a:p><a:r><a:t>Launch in May</a:t></a:r></a:p>",
        );
        let second = slide(
            "<a:p><a:r><a:t>Hiring</a:t></a:r><a:br/><a:r><a:t>Two engineers</a:t></a:r></a:p>",
        );
        let tenth = slide("<a:p><a:r><a:t>Questions</a:t></a:r></a:p>");
        let path = zip(
            dir.path(),
            "deck.pptx",
            &[
                ("ppt/slides/slide10.xml", &tenth),
                ("ppt/slides/slide2.xml", &second),
                ("ppt/slides/slide1.xml", &first),
                ("ppt/slides/_rels/slide1.xml.rels", "<Relationships/>"),
            ],
        );
        assert_eq!(
            read(&path, "pptx", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Roadmap\nLaunch in May\nHiring\nTwo engineers\nQuestions")
        );
    }

    #[test]
    fn reads_open_documents() {
        let dir = tempfile::tempdir().unwrap();
        let odt = open_document(
            dir.path(),
            "letter.odt",
            MANIFEST,
            "<office:text><text:h>Dear landlord</text:h>\
             <text:p>The heating<text:s/>is <text:span>broken</text:span>.</text:p>\
             <table:table><table:table-row>\
             <table:table-cell><text:p>Room</text:p></table:table-cell>\
             <table:table-cell><text:p>Kitchen</text:p></table:table-cell>\
             </table:table-row></table:table></office:text>",
        );
        assert_eq!(
            read(&odt, "odt", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Dear landlord\nThe heating is broken.\nRoom \tKitchen")
        );

        let ods = open_document(
            dir.path(),
            "costs.ods",
            MANIFEST,
            "<office:spreadsheet><table:table table:name=\"Sheet1\"><table:table-row>\
             <table:table-cell><text:p>Item</text:p></table:table-cell>\
             <table:table-cell table:number-columns-repeated=\"3\"/>\
             <table:table-cell><text:p>12</text:p></table:table-cell>\
             </table:table-row><table:table-row>\
             <table:table-cell><text:p>Paper</text:p></table:table-cell>\
             </table:table-row></table:table></office:spreadsheet>",
        );
        assert_eq!(
            read(&ods, "ods", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Item \t12\nPaper")
        );

        let odp = open_document(
            dir.path(),
            "talk.odp",
            MANIFEST,
            "<office:presentation><draw:page><draw:frame><draw:text-box>\
             <text:p>Welcome</text:p><text:p>Agenda<text:line-break/>Lunch</text:p>\
             </draw:text-box></draw:frame></draw:page></office:presentation>",
        );
        assert_eq!(
            read(&odp, "odp", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Welcome\nAgenda\nLunch")
        );
    }

    #[test]
    fn reads_spreadsheet_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = zip(
            dir.path(),
            "costs.xlsx",
            &[
                (
                    "_rels/.rels",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
                ),
                (
                    "xl/workbook.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Costs" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
                ),
                (
                    "xl/sharedStrings.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Item</t></si><si><t>Paper</t></si></sst>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row>
<row r="2"><c r="A2" t="s"><v>1</v></c><c r="C2"><v>12.5</v></c></row>
<row r="1048576"><c r="XFD1048576" t="inlineStr"><is><t>Far away</t></is></c></row>
</sheetData></worksheet>"#,
                ),
            ],
        );
        assert_eq!(
            read(&path, "xlsx", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Item\tCost\nPaper\t12.5\nFar away")
        );
    }

    #[test]
    fn reads_old_excel_files() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prices.xls");
        assert_eq!(
            read(&path, "xls", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("Item\tCost\nPaper\t12.5\nToner\tink cartridge")
        );
    }

    #[test]
    fn keeps_the_start_of_long_documents() {
        let dir = tempfile::tempdir().unwrap();
        let body = word(&"<w:p><w:r><w:t>lorem ipsum ünïcode</w:t></w:r></w:p>".repeat(10_000));
        let path = zip(dir.path(), "long.docx", &[("word/document.xml", &body)]);
        let text = read(&path, "docx", DEFAULT_TEXT_LIMIT).unwrap();
        assert!(text.starts_with("lorem ipsum ünïcode\nlorem"));
        assert!(text.len() <= DEFAULT_TEXT_LIMIT);
        assert!(text.len() > DEFAULT_TEXT_LIMIT - 100);
    }

    #[test]
    fn stops_at_the_deadline() {
        let mut text = Text::new(DEFAULT_TEXT_LIMIT, Instant::now());
        thread::sleep(Duration::from_millis(1));
        assert!(!text.push("late"));
        assert_eq!(text.finish(), None);
    }

    #[test]
    fn skips_broken_and_encrypted_files() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk");
        fs::write(&junk, b"PK\x03\x04 not a zip %PDF-1.4 \xff\xfe").unwrap();
        for ext in EXTS {
            assert_eq!(read(&junk, ext, DEFAULT_TEXT_LIMIT), None, "{ext}");
        }

        let mut truncated = pdf(&["Quarterly budget review"]);
        truncated.truncate(truncated.len() / 2);
        let path = dir.path().join("truncated.pdf");
        fs::write(&path, truncated).unwrap();
        assert_eq!(read(&path, "pdf", DEFAULT_TEXT_LIMIT), None);

        let path = zip(
            dir.path(),
            "broken.docx",
            &[(
                "word/document.xml",
                "<w:document><w:body><w:p><w:t>kept</w:t></w:p></w:x>",
            )],
        );
        assert_eq!(
            read(&path, "docx", DEFAULT_TEXT_LIMIT).as_deref(),
            Some("kept")
        );

        let path = zip(dir.path(), "broken.xlsx", &[("xl/workbook.xml", "<")]);
        assert_eq!(read(&path, "xlsx", DEFAULT_TEXT_LIMIT), None);

        let manifest = MANIFEST.replace("/>", "><manifest:encryption-data/></manifest:file-entry>");
        let path = open_document(
            dir.path(),
            "secret.odt",
            &manifest,
            "<office:text><text:p>hidden</text:p></office:text>",
        );
        assert_eq!(read(&path, "odt", DEFAULT_TEXT_LIMIT), None);
    }

    #[test]
    fn skips_huge_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.pdf");
        File::create(&path).unwrap().set_len(MAX_SIZE + 1).unwrap();
        assert_eq!(read(&path, "pdf", DEFAULT_TEXT_LIMIT), None);
    }
}
