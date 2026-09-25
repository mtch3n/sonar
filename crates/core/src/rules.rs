use std::{fs, path::Path};

use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

#[derive(Clone)]
pub struct Rules(Gitignore);

impl Rules {
    pub fn load(path: &Path, root: &Path) -> Result<Rules> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, default_rules())
                .with_context(|| format!("writing {}", path.display()))?;
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Rules::parse(root, &text)
    }

    pub fn parse(root: &Path, text: &str) -> Result<Rules> {
        let mut builder = GitignoreBuilder::new(root);
        for line in text.lines() {
            builder.add_line(None, line)?;
        }
        Ok(Rules(builder.build()?))
    }

    pub(crate) fn excludes(&self, path: &Path, is_dir: bool) -> bool {
        self.0.matched(path, is_dir).is_ignore()
    }
}

const COMMON_RULES: &str = "\
.*/
!/.ssh/
!/.gnupg/
!/.kube/
/.kube/cache/

.DS_Store
Thumbs.db
desktop.ini

node_modules/
__pycache__/
/go/pkg/
/fvm/
";

fn default_rules() -> String {
    let platform = if cfg!(target_os = "macos") {
        "/Library/\n"
    } else if cfg!(windows) {
        "/AppData/\n"
    } else {
        "/Android/Sdk/\n"
    };
    format!("{COMMON_RULES}{platform}")
}
