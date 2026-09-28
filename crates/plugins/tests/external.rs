#![cfg(unix)]

use std::{fs, path::Path, sync::Arc, time::Duration};

use serde_json::Map;
use sonar_plugins::{Action, External, Manifest, resolve};

/// Echoes each query back as an item, except for a few that misbehave on purpose.
const SCRIPT: &str = r#"#!/bin/sh
while IFS= read -r line; do
  q=$(printf '%s' "$line" | sed 's/^{"query":"\([^"]*\)".*$/\1/')
  case "$q" in
    greet)
      g=$(printf '%s' "$line" | sed 's/.*"greeting":"\([^"]*\)".*/\1/')
      printf '{"items":[{"title":"%s","action":{"copy":"x"}}]}\n' "$g" ;;
    crash) echo "boom" >&2; exit 1 ;;
    fail) echo '{"error":"no luck"}' ;;
    noise) echo 'hello' ;;
    slow) sleep 1; echo '{"items":[]}' ;;
    *) printf '{"items":[{"title":"%s","action":{"copy":"%s"}}]}\n' "$q" "$q" ;;
  esac
done
"#;

fn plugin(dir: &Path, answer_within: Duration) -> External {
    // Run by sh rather than executed itself: a file just written can't be executed
    // while another test's process, started at that moment, still holds it open.
    fs::write(dir.join("echo.sh"), SCRIPT).unwrap();
    fs::write(
        dir.join("plugin.toml"),
        "name = \"Echo\"\ncommand = [\"sh\", \"echo.sh\"]\n\n[[settings]]\nkey = \"greeting\"\ntitle = \"Greeting\"\ntype = \"text\"\ndefault = \"hello\"\n",
    )
    .unwrap();
    let manifest = Manifest::read(dir).unwrap();
    let (settings, _) = resolve(&manifest.settings, &Map::new());
    External::new(manifest, settings, |_| {}, answer_within)
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
    assert_eq!(titles(&plugin, "greet").await, Ok(vec!["hello".into()]));

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
        Map::new(),
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
