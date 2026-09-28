use std::{fs, io::Write, path::Path};

use sonar_core::{Embedder, Index, Kind, Level, Query, Rules, ScanOptions};
use zip::{ZipWriter, write::SimpleFileOptions};

fn limit(bytes: usize) -> ScanOptions {
    ScanOptions {
        text_limit: bytes,
        ..ScanOptions::default()
    }
}

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
    let stats = index.scan(&home, &rules, &ScanOptions::default()).unwrap();
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
    let stats = index.scan(&home, &rules, &ScanOptions::default()).unwrap();
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
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();

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
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();

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
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();
    assert_eq!(find(&index, &home, "dentist"), ["notes.md"]);
    assert_eq!(find(&index, &home, "budget"), ["budget.txt"]);
}

#[test]
fn a_new_text_limit_rereads_unchanged_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "Documents/log.txt", "start middle finish");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules, &limit(12)).unwrap();
    assert_eq!(find(&index, &home, "middle"), ["log.txt"]);
    assert!(find(&index, &home, "finish").is_empty());

    index.scan(&home, &rules, &ScanOptions::default()).unwrap();
    assert_eq!(find(&index, &home, "finish"), ["log.txt"]);

    index.scan(&home, &rules, &limit(5)).unwrap();
    assert!(find(&index, &home, "middle").is_empty());
}

#[test]
fn same_size_writes_in_the_same_second_are_seen() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "notes.txt", "apple");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let options = ScanOptions::default();
    index.scan(&home, &rules, &options).unwrap();
    write(&home, "notes.txt", "mango");
    index.scan(&home, &rules, &options).unwrap();
    assert!(find(&index, &home, "apple").is_empty());
    assert_eq!(find(&index, &home, "mango"), ["notes.txt"]);
}

#[test]
fn moved_files_keep_their_text() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "Inbox/lease.txt", "boiler repairs");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let options = ScanOptions::default();
    index.scan(&home, &rules, &options).unwrap();
    fs::create_dir_all(home.join("Archive")).unwrap();
    fs::rename(home.join("Inbox/lease.txt"), home.join("Archive/lease.txt")).unwrap();
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(find(&index, &home, "boiler"), ["lease.txt"]);

    // Text the cache lost is read again.
    drop(index);
    fs::remove_file(tmp.path().join("cache.db")).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(
        lines(&index, &home, "boiler"),
        [("lease.txt".to_owned(), Some("boiler repairs".to_owned()))]
    );

    // The index can be rebuilt from scratch; the cache outlives it.
    drop(index);
    fs::remove_file(tmp.path().join("index.db")).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(find(&index, &home, "boiler"), ["lease.txt"]);
}

#[test]
fn levels_choose_what_is_indexed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "notes.md", "gazebo plans");
    write(&home, "setup.toml", "gazebo = true");
    write(&home, "movie.mp4", "");
    write(&home, "app/Cargo.toml", "");
    write(&home, "app/src/main.rs", "fn gazebo() {}");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let mut options = ScanOptions::default();
    index.scan(&home, &rules, &options).unwrap();
    let mut found = find(&index, &home, "gazebo");
    found.sort();
    assert_eq!(found, ["notes.md", "setup.toml"]);

    options.levels.set(Kind::Config, Level::Name);
    options.levels.set(Kind::Video, Level::Skip);
    options.levels.set(Kind::Code, Level::Text);
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(find(&index, &home, "gazebo"), ["notes.md"]);
    assert_eq!(find(&index, &home, "gazebo kind:code"), ["main.rs"]);
    assert!(find(&index, &home, "movie").is_empty());
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
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();

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

/// Understands a few topics: each is a dimension, and a text's vector says which
/// topics its words belong to.
struct Topics {
    calls: usize,
}

const TOPICS: [&[&str]; 3] = [
    &["photo", "photos", "picture", "pictures", "image"],
    &["backup", "backs", "copy", "rsync", "sync"],
    &["money", "invoice", "receipt", "paid", "payment", "發票"],
];

impl Embedder for Topics {
    fn id(&self) -> &str {
        "topics"
    }

    fn passages(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.calls += texts.len();
        Ok(texts.iter().map(|t| topics(t)).collect())
    }

    fn query(&mut self, text: &str) -> anyhow::Result<Vec<f32>> {
        Ok(topics(text))
    }

    fn min_score(&self) -> f32 {
        0.5
    }
}

fn topics(text: &str) -> Vec<f32> {
    let text = text.to_lowercase();
    let mut v: Vec<f32> = TOPICS
        .iter()
        .map(|words| words.iter().filter(|w| text.contains(*w)).count() as f32)
        .collect();
    v.push(0.1);
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / norm).collect()
}

#[test]
fn search_by_meaning() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(
        &home,
        "bin/nightly.sh",
        "#!/bin/sh\nrsync -av ~/Pictures nas:/photos\n",
    );
    write(&home, "Documents/notes.md", "Lunch with Sam on Friday.\n");
    write(&home, "Documents/2024 發票.txt", "");
    write(&home, "Pictures/beach photo.jpg", "");

    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let mut options = ScanOptions::default();
    options.levels.set(Kind::Image, Level::Meaning);
    index.scan(&home, &rules, &options).unwrap();

    let mut model = Topics { calls: 0 };
    assert_eq!(index.pending_meaning("topics").unwrap().1, 2);
    let stats = index.embed(&mut model, &mut |_| true).unwrap();
    assert_eq!(stats.files, 2, "{stats:?}");
    assert_eq!(index.pending_meaning("topics").unwrap(), (0, 0));
    assert!(stats.names >= 4, "{stats:?}");
    let calls = model.calls;
    index.embed(&mut model, &mut |_| true).unwrap();
    assert_eq!(model.calls, calls, "nothing is embedded twice");

    let mut search = |input: &str| -> Vec<(String, Option<String>)> {
        let query = Query::parse(input, &home).unwrap();
        index
            .search_with(&query, &mut model)
            .unwrap()
            .into_iter()
            .map(|h| (h.name, h.line))
            .collect()
    };
    // No file has these words, but the script is about them.
    assert_eq!(
        search("script that backs up my pictures")[0],
        (
            "nightly.sh".to_owned(),
            Some("rsync -av ~/Pictures nas:/photos".to_owned())
        )
    );
    // Names are searched by meaning too, in any language the model knows.
    assert_eq!(search("payment records")[0].0, "2024 發票.txt");
    // Filters apply to what's found by meaning.
    assert!(search("backup of photos kind:doc").is_empty());
    assert_eq!(search("photo image")[0].0, "beach photo.jpg");
    // Short and exact queries are matched by words only.
    assert!(search("\"backup\"").is_empty());
}

#[test]
fn embedding_stops_when_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "a.md", "photos");
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();
    let mut model = Topics { calls: 0 };
    let stats = index.embed(&mut model, &mut |_| false).unwrap();
    assert!(stats.stopped);
    assert_eq!(model.calls, 0);
}
