use std::{fs, path::Path};

use sonar_core::{Index, Query, Rules};

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, relative).unwrap();
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
