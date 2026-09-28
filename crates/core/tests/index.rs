use std::{fs, io::Write, path::Path};

use sonar_core::{Index, Query, Rules};
use zip::{ZipWriter, write::SimpleFileOptions};

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, relative).unwrap();
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn docx(root: &Path, relative: &str, paragraphs: &[&str]) {
    let body: String = paragraphs
        .iter()
        .map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>"))
        .collect();
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    write!(
        zip,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    )
    .unwrap();
    zip.finish().unwrap();
}

fn lines(index: &Index, home: &Path, input: &str) -> Vec<(String, Option<String>)> {
    let query = Query::parse(input, home).unwrap();
    index
        .search(&query)
        .unwrap()
        .into_iter()
        .map(|hit| (hit.name, hit.line))
        .collect()
}

fn find(index: &Index, home: &Path, input: &str) -> Vec<String> {
    let query = Query::parse(input, home).unwrap();
    index
        .search(&query)
        .unwrap()
        .into_iter()
        .map(|hit| hit.name)
        .collect()
}

#[test]
fn scan_and_search() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    for file in [
        "Documents/invoice-march.pdf",
        "Documents/taxes/receipt2024.png",
        "scripts/backupPhotos.sh",
        ".ssh/id_ed25519",
        ".cache/junk.txt",
        "app/Cargo.toml",
        "app/src/main.rs",
        "app/target/debug/build.log",
    ] {
        touch(&home, file);
    }
    fs::create_dir(home.join("app/.git")).unwrap();
    fs::write(home.join("app/.gitignore"), "target/\n").unwrap();

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let stats = index.scan(&home, &rules).unwrap();
    assert_eq!(stats.projects, 1);

    assert_eq!(find(&index, &home, "invoice"), ["invoice-march.pdf"]);
    assert_eq!(
        find(&index, &home, "photos kind:script"),
        ["backupPhotos.sh"]
    );
    assert_eq!(find(&index, &home, "kind:key"), ["id_ed25519"]);
    assert!(find(&index, &home, "junk").is_empty());
    assert_eq!(find(&index, &home, "app"), ["app"]);
    assert!(find(&index, &home, "main").is_empty());
    assert_eq!(find(&index, &home, "main kind:code"), ["main.rs"]);
    assert!(find(&index, &home, "build in:app").is_empty());
    assert_eq!(
        find(&index, &home, "in:~/Documents kind:image"),
        ["receipt2024.png"]
    );
    assert!(find(&index, &home, "invoice -march").is_empty());

    fs::remove_file(home.join("Documents/invoice-march.pdf")).unwrap();
    let stats = index.scan(&home, &rules).unwrap();
    assert_eq!(stats.removed, 1);
    assert!(find(&index, &home, "invoice").is_empty());
}

#[test]
fn folders_are_the_ones_scanned() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    for file in [
        "Documents/taxes/receipt.png",
        ".cache/junk.txt",
        "app/Cargo.toml",
        "app/src/main.rs",
        "app/target/debug/build.log",
        "Tool.app/Contents/Info.plist",
    ] {
        touch(&home, file);
    }
    fs::create_dir(home.join("app/.git")).unwrap();
    fs::write(home.join("app/.gitignore"), "target/\n").unwrap();

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules).unwrap();

    let mut folders = index.folders().unwrap();
    folders.sort();
    let expected = ["Documents", "Documents/taxes", "app", "app/src"].map(|f| home.join(f));
    assert_eq!(folders, expected);
}

#[test]
fn search_inside_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(
        &home,
        "scripts/backupPhotos.sh",
        "#!/bin/sh\n# nightly\nrsync -av ~/Pictures nas:/photos\n",
    );
    write(
        &home,
        "Documents/notes.md",
        "# Monday\n\nMeeting about the quarterly budget.\n",
    );
    write(&home, "Documents/budget.txt", "numbers");
    write(&home, "Documents/photo.png", "rsync");
    write(&home, ".ssh/id_ed25519", "secretword");
    write(&home, "app/Cargo.toml", "");
    write(&home, "app/README.md", "zebra");
    fs::create_dir(home.join("app/.git")).unwrap();

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules).unwrap();

    let line = |text: &str| Some(text.to_owned());
    assert_eq!(
        lines(&index, &home, "rsync"),
        [(
            "backupPhotos.sh".to_owned(),
            line("rsync -av ~/Pictures nas:/photos")
        )]
    );
    assert_eq!(
        lines(&index, &home, "budget"),
        [
            ("budget.txt".to_owned(), None),
            (
                "notes.md".to_owned(),
                line("Meeting about the quarterly budget.")
            ),
        ]
    );
    assert_eq!(
        lines(&index, &home, "photos"),
        [("backupPhotos.sh".to_owned(), None)]
    );
    assert!(find(&index, &home, "secretword").is_empty());
    assert!(find(&index, &home, "zebra kind:doc").is_empty());
    assert!(find(&index, &home, "mo").is_empty());
    assert!(find(&index, &home, "budget -quarterly").contains(&"notes.md".to_owned()));

    write(&home, "Documents/notes.md", "Tuesday: dentist");
    index.scan(&home, &rules).unwrap();
    assert_eq!(find(&index, &home, "dentist"), ["notes.md"]);
    assert_eq!(find(&index, &home, "budget"), ["budget.txt"]);
}

#[test]
fn search_inside_documents() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("Documents")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minutes.pdf"),
        home.join("Documents/minutes.pdf"),
    )
    .unwrap();
    docx(
        &home,
        "Documents/lease.docx",
        &[
            "Tenancy agreement",
            "The landlord repairs the boiler within a week.",
        ],
    );
    docx(&home, "app/docs/design.docx", &["boiler"]);
    write(&home, "app/Cargo.toml", "");
    write(&home, "Documents/broken.docx", "not a zip");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules).unwrap();

    let line = |text: &str| Some(text.to_owned());
    assert_eq!(
        lines(&index, &home, "treasurer"),
        [(
            "minutes.pdf".to_owned(),
            line("The treasurer presented the audit")
        )]
    );
    assert_eq!(
        lines(&index, &home, "boiler"),
        [(
            "lease.docx".to_owned(),
            line("The landlord repairs the boiler within a week.")
        )]
    );
    assert_eq!(find(&index, &home, "tenancy kind:doc"), ["lease.docx"]);
    assert!(find(&index, &home, "zip").is_empty());
}
