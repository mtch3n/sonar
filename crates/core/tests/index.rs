use std::{fs, path::Path};

use sonar_core::{Index, Query, Rules};

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
