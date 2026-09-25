#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

use sonar_plugins::{Action, External, Manifest};

/// Echoes each query back as an item, except for a few that misbehave on purpose.
const SCRIPT: &str = r#"#!/bin/sh
while IFS= read -r line; do
  q=$(printf '%s' "$line" | sed 's/^{"query":"\(.*\)"}$/\1/')
  case "$q" in
    crash) echo "boom" >&2; exit 1 ;;
    fail) echo '{"error":"no luck"}' ;;
    noise) echo 'hello' ;;
    slow) sleep 1; echo '{"items":[]}' ;;
    *) printf '{"items":[{"title":"%s","action":{"copy":"%s"}}]}\n' "$q" "$q" ;;
  esac
done
"#;

fn plugin(dir: &Path, answer_within: Duration) -> External {
    let script = dir.join("echo.sh");
    fs::write(&script, SCRIPT).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        dir.join("plugin.toml"),
        "name = \"Echo\"\ncommand = [\"./echo.sh\"]\n",
    )
    .unwrap();
    External::new(Manifest::read(dir).unwrap(), |_| {}, answer_within)
}

async fn titles(plugin: &External, query: &str) -> Result<Vec<String>, String> {
    let items = plugin.search(query).await.expect("not superseded")?;
    Ok(items.into_iter().map(|item| item.title).collect())
}

#[tokio::test]
async fn answers_and_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin = plugin(tmp.path(), Duration::from_secs(5));

    let items = plugin.search("abc").await.unwrap().unwrap();
    assert_eq!(items[0].title, "abc");
    assert_eq!(items[0].action, Action::Copy("abc".into()));

    assert_eq!(titles(&plugin, "fail").await, Err("no luck".into()));
    assert_eq!(
        titles(&plugin, "after fail").await,
        Ok(vec!["after fail".into()])
    );

    assert_eq!(titles(&plugin, "crash").await, Err("stopped: boom".into()));
    assert_eq!(
        titles(&plugin, "restarted").await,
        Ok(vec!["restarted".into()])
    );

    let noise = titles(&plugin, "noise").await.unwrap_err();
    assert!(noise.contains("JSON"), "{noise}");
    assert_eq!(
        titles(&plugin, "resynced").await,
        Ok(vec!["resynced".into()])
    );
}

#[tokio::test]
async fn restarts_a_plugin_that_takes_too_long() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin = plugin(tmp.path(), Duration::from_millis(300));
    assert_eq!(
        titles(&plugin, "slow").await,
        Err("didn't answer within 0.3 seconds".into())
    );
    assert_eq!(titles(&plugin, "next").await, Ok(vec!["next".into()]));
}

#[tokio::test]
async fn skips_queries_typed_past() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin = Arc::new(plugin(tmp.path(), Duration::from_secs(5)));

    let busy = tokio::spawn({
        let plugin = plugin.clone();
        async move { plugin.search("slow").await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let skipped = tokio::spawn({
        let plugin = plugin.clone();
        async move { plugin.search("typed past").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let latest = plugin.search("latest").await;

    assert_eq!(busy.await.unwrap(), Some(Ok(vec![])));
    assert_eq!(skipped.await.unwrap(), None);
    assert_eq!(latest.unwrap().unwrap()[0].title, "latest");
}

#[test]
fn a_missing_program_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("plugin.toml"),
        "name = \"Gone\"\ncommand = [\"sonar-no-such-program\"]\n",
    )
    .unwrap();
    let plugin = External::new(
        Manifest::read(tmp.path()).unwrap(),
        |_| {},
        Duration::from_secs(1),
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let answer = runtime.block_on(plugin.search("x")).unwrap().unwrap_err();
    assert!(
        answer.starts_with("couldn't start `sonar-no-such-program`"),
        "{answer}"
    );
}
