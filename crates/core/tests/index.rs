use std::{fs, io::Write, path::Path};

use sonar_core::{
    Embedder, Fingerprint, Index, Job, Kind, Level, Output, ProcessOptions, Processor, Query,
    Rules, ScanOptions,
};
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

/// The same topics, understood somewhere else.
struct CloudTopics(Topics);

impl Embedder for CloudTopics {
    fn id(&self) -> &str {
        "cloud-topics"
    }

    fn passages(&mut self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.0.passages(texts)
    }

    fn query(&mut self, text: &str) -> anyhow::Result<Vec<f32>> {
        self.0.query(text)
    }

    fn min_score(&self) -> f32 {
        0.5
    }

    fn is_local(&self) -> bool {
        false
    }
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
    let stats = index.embed(&mut model, &[], &mut |_| true).unwrap();
    assert_eq!(stats.files, 2, "{stats:?}");
    assert_eq!(index.pending_meaning("topics").unwrap(), (0, 0));
    assert!(stats.names >= 4, "{stats:?}");
    let calls = model.calls;
    index.embed(&mut model, &[], &mut |_| true).unwrap();
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
    let stats = index.embed(&mut model, &[], &mut |_| false).unwrap();
    assert!(stats.stopped);
    assert_eq!(model.calls, 0);
}

#[test]
fn forgetting_reads_and_embeds_again() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "Documents/a.md", "photos");
    write(&home, "Documents/b.md", "backup");
    write(&home, "Other/c.md", "rsync");
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let options = ScanOptions::default();
    index.scan(&home, &rules, &options).unwrap();
    let mut model = Topics { calls: 0 };
    index.embed(&mut model, &[], &mut |_| true).unwrap();
    assert_eq!(index.pending_meaning("topics").unwrap(), (0, 0));

    assert_eq!(
        index.forget(&home.join("Documents")).unwrap(),
        3,
        "the folder and its files"
    );
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(index.pending_meaning("topics").unwrap(), (2, 2));
    assert_eq!(find(&index, &home, "photos"), ["a.md"]);
    assert_eq!(index.forget(&home.join("Other/c.md")).unwrap(), 1);
}

#[test]
fn private_folders_stay_local() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "Private/diary.md", "photos of the beach");
    write(&home, "Shared/notes.md", "backup plan");
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    index.scan(&home, &rules, &ScanOptions::default()).unwrap();
    let private = [home.join("Private")];

    let mut cloud = CloudTopics(Topics { calls: 0 });
    let stats = index.embed(&mut cloud, &private, &mut |_| true).unwrap();
    assert_eq!(
        (stats.names, stats.files),
        (1, 1),
        "only what's under Shared"
    );
    let mut local = Topics { calls: 0 };
    let stats = index.embed(&mut local, &private, &mut |_| true).unwrap();
    assert_eq!(
        (stats.names, stats.files),
        (2, 2),
        "a local model reads everything"
    );
}

/// Says what's in a picture by its file's first line, and fails on empty files.
struct Looker {
    version: &'static str,
    seen: Vec<String>,
    frames: usize,
}

impl Processor for Looker {
    fn id(&self) -> &str {
        "looker"
    }
    fn version(&self) -> &str {
        self.version
    }
    fn kinds(&self) -> &[Kind] {
        &[Kind::Image, Kind::Video]
    }
    fn frames(&self) -> usize {
        self.frames
    }
    fn is_local(&self) -> bool {
        true
    }
    fn process(&mut self, job: &Job) -> anyhow::Result<Output> {
        self.seen
            .push(job.path.file_name().unwrap().to_string_lossy().into_owned());
        if job.kind == Kind::Video {
            assert_eq!(job.frames.len(), self.frames);
            assert!(job.duration.unwrap() > 1.0);
            return Ok(Output {
                fingerprint: Some(Fingerprint {
                    algo: "test".into(),
                    bits: 42,
                }),
                ..Output::default()
            });
        }
        let text = fs::read_to_string(job.path)?;
        let first = text.lines().next().filter(|l| !l.is_empty());
        let first = first.ok_or_else(|| anyhow::anyhow!("nothing to see"))?;
        Ok(Output {
            text: Some(format!("A picture of {first}")),
            tags: vec!["photo".into(), first.to_owned()],
            labels: Vec::new(),
            fingerprint: None,
        })
    }
}

#[test]
fn processors_make_pictures_searchable() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, "Pictures/IMG_0001.jpg", "lighthouse\n");
    write(&home, "Pictures/IMG_0002.jpg", "");
    write(&home, "Backup/copy.jpg", "lighthouse\n");
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let mut options = ScanOptions::default();
    options.levels.set(Kind::Image, Level::Meaning);
    index.scan(&home, &rules, &options).unwrap();
    assert!(find(&index, &home, "lighthouse").is_empty());

    let frames = tmp.path().join("frames");
    let process = ProcessOptions {
        root: &home,
        frames: &frames,
        private: &[],
    };
    let mut looker = Looker {
        version: "1",
        seen: Vec::new(),
        frames: 4,
    };
    let stats = index.process(&mut looker, &process, &mut |_| true).unwrap();
    assert_eq!((stats.done, stats.failed), (1, 1), "{stats:?}");
    assert_eq!(
        looker.seen.len(),
        2,
        "the copy isn't looked at again: {:?}",
        looker.seen
    );
    let mut found = find(&index, &home, "lighthouse");
    found.sort();
    assert_eq!(found, ["IMG_0001.jpg", "copy.jpg"]);

    // What it said is searched by meaning too.
    let mut model = Topics { calls: 0 };
    index.embed(&mut model, &[], &mut |_| true).unwrap();
    let query = Query::parse("picture photo", &home).unwrap();
    let hits = index.search_with(&query, &mut model).unwrap();
    assert!(hits.iter().any(|h| h.name == "IMG_0001.jpg"), "{hits:?}");

    // Files it has seen aren't looked at again, until it changes.
    index.process(&mut looker, &process, &mut |_| true).unwrap();
    assert_eq!(looker.seen.len(), 2);
    looker.version = "2";
    index.process(&mut looker, &process, &mut |_| true).unwrap();
    assert_eq!(looker.seen.len(), 4);

    // Rescanning keeps what it said.
    index.scan(&home, &rules, &options).unwrap();
    assert_eq!(find(&index, &home, "lighthouse").len(), 2);
}

#[test]
fn processors_get_frames_of_videos() {
    let ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .output();
    if ffmpeg.is_err() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("Videos")).unwrap();
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=160x120:rate=10",
        ])
        .arg(home.join("Videos/clip.mp4"))
        .status()
        .unwrap();
    assert!(made.success());
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let mut options = ScanOptions::default();
    options.levels.set(Kind::Video, Level::Text);
    index.scan(&home, &rules, &options).unwrap();
    let frames = tmp.path().join("frames");
    let mut looker = Looker {
        version: "1",
        seen: Vec::new(),
        frames: 4,
    };
    let process = ProcessOptions {
        root: &home,
        frames: &frames,
        private: &[],
    };
    let stats = index.process(&mut looker, &process, &mut |_| true).unwrap();
    assert_eq!(stats.done, 1, "{stats:?}");
    assert_eq!(
        fs::read_dir(&frames).unwrap().count(),
        1,
        "frames kept by hash"
    );

    fs::remove_file(home.join("Videos/clip.mp4")).unwrap();
    index.scan(&home, &rules, &options).unwrap();
    index.process(&mut looker, &process, &mut |_| true).unwrap();
    assert_eq!(
        fs::read_dir(&frames).unwrap().count(),
        0,
        "frames of gone videos go"
    );
}

/// Fingerprints pictures by their text: "a" and "b" look alike, "z" doesn't.
struct Printer;

impl Processor for Printer {
    fn id(&self) -> &str {
        "printer"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn kinds(&self) -> &[Kind] {
        &[Kind::Image]
    }
    fn frames(&self) -> usize {
        0
    }
    fn is_local(&self) -> bool {
        true
    }
    fn process(&mut self, job: &Job) -> anyhow::Result<Output> {
        let bits = match fs::read_to_string(job.path)?.chars().next() {
            Some('a') => 0b0000,
            Some('b') => 0b0011,
            _ => u64::MAX,
        };
        Ok(Output {
            fingerprint: Some(Fingerprint {
                algo: "test".into(),
                bits,
            }),
            ..Output::default()
        })
    }
}

#[test]
fn duplicates_and_look_alikes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let big = "x".repeat(20_000);
    write(&home, "Documents/report.pdf", &big);
    write(&home, "Downloads/report (1).pdf", &big);
    write(&home, "Downloads/other.pdf", &"y".repeat(20_000));
    write(&home, "Documents/notes.txt", "draft one");
    write(&home, "Documents/notes copy.txt", "draft two");
    write(&home, "Pictures/a.jpg", &format!("a{}", "1".repeat(20_000)));
    write(&home, "Pictures/b.jpg", &format!("b{}", "2".repeat(20_001)));
    write(&home, "Pictures/z.jpg", &format!("z{}", "3".repeat(20_002)));
    let rules = Rules::load(&tmp.path().join("ignore"), &home).unwrap();
    let mut index = Index::open(&tmp.path().join("index.db")).unwrap();
    let mut options = ScanOptions::default();
    options.levels.set(Kind::Image, Level::Text);
    index.scan(&home, &rules, &options).unwrap();
    let frames = tmp.path().join("frames");
    let process = ProcessOptions {
        root: &home,
        frames: &frames,
        private: &[],
    };
    index
        .process(&mut Printer, &process, &mut |_| true)
        .unwrap();

    let groups = |input: &str| -> Vec<(String, Vec<String>)> {
        let query = Query::parse(input, &home).unwrap();
        index
            .search(&query)
            .unwrap()
            .into_iter()
            .map(|h| (h.name, h.line.into_iter().collect()))
            .collect()
    };
    let same = groups("dupes:same");
    assert_eq!(same.len(), 2, "{same:?}");
    assert!(
        same[0].1[0].starts_with("Same content · 2 files · 19.5 KB spare"),
        "{same:?}"
    );
    assert_eq!(same[1].1[0], format!("Copy of {}", same[0].0));

    let looks = groups("dupes:looks");
    let mut names: Vec<&str> = looks.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    assert_eq!(names, ["a.jpg", "b.jpg"], "{looks:?}");
    assert!(
        groups("dupes:looks<1").is_empty(),
        "a and b differ in 2 bits"
    );

    let named = groups("dupes:names");
    let mut names: Vec<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        ["notes copy.txt", "notes.txt"],
        "report (1) is an exact copy"
    );

    assert_eq!(groups("dupes: kind:image").len(), 2);
    let similar = groups("similar:~/Pictures/a.jpg");
    assert_eq!(
        similar,
        [(
            "b.jpg".to_owned(),
            vec!["Looks alike: 2 of 64 bits apart".to_owned()]
        )]
    );
    let copies = groups("similar:~/Documents/report.pdf");
    assert_eq!(copies[0].0, "report (1).pdf");
    assert!(Query::parse("dupes:everything", &home).is_err());
}
