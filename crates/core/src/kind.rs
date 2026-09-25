use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Project,
    Folder,
    App,
    Code,
    Script,
    Key,
    Pdf,
    Doc,
    Sheet,
    Slides,
    Image,
    Video,
    Audio,
    Archive,
    Config,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 16] = [
        Kind::Project,
        Kind::Folder,
        Kind::App,
        Kind::Code,
        Kind::Script,
        Kind::Key,
        Kind::Pdf,
        Kind::Doc,
        Kind::Sheet,
        Kind::Slides,
        Kind::Image,
        Kind::Video,
        Kind::Audio,
        Kind::Archive,
        Kind::Config,
        Kind::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Project => "project",
            Kind::Folder => "folder",
            Kind::App => "app",
            Kind::Code => "code",
            Kind::Script => "script",
            Kind::Key => "key",
            Kind::Pdf => "pdf",
            Kind::Doc => "doc",
            Kind::Sheet => "sheet",
            Kind::Slides => "slides",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Archive => "archive",
            Kind::Config => "config",
            Kind::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<Kind> {
        let name = name.to_lowercase();
        let singular = name.strip_suffix('s').unwrap_or(&name);
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str() == name || k.as_str() == singular)
    }

    pub(crate) fn needs_head(ext: &str, executable: bool) -> bool {
        (ext.is_empty() && executable) || ext == "key"
    }

    pub(crate) fn of_file(name: &str, ext: &str, in_project: bool, head: &[u8]) -> Kind {
        let name = name.to_lowercase();
        if ENV_TEMPLATE_EXTS.contains(&ext) && name.starts_with(".env.") {
            return Kind::Config;
        }
        if ext == "key" && head.starts_with(b"PK") {
            return Kind::Slides;
        }
        if KEY_NAMES.contains(&name.as_str())
            || KEY_PREFIXES.iter().any(|p| name.starts_with(p))
            || KEY_EXTS.contains(&ext)
        {
            return Kind::Key;
        }
        let script = if in_project { Kind::Code } else { Kind::Script };
        match ext {
            "pdf" => Kind::Pdf,
            _ if DOC_EXTS.contains(&ext) => Kind::Doc,
            _ if SHEET_EXTS.contains(&ext) => Kind::Sheet,
            _ if SLIDES_EXTS.contains(&ext) => Kind::Slides,
            _ if IMAGE_EXTS.contains(&ext) => Kind::Image,
            _ if VIDEO_EXTS.contains(&ext) => Kind::Video,
            _ if AUDIO_EXTS.contains(&ext) => Kind::Audio,
            _ if ARCHIVE_EXTS.contains(&ext) => Kind::Archive,
            _ if APP_EXTS.contains(&ext) => Kind::App,
            _ if SCRIPT_EXTS.contains(&ext) => script,
            _ if CODE_EXTS.contains(&ext) => Kind::Code,
            _ if CONFIG_EXTS.contains(&ext) => Kind::Config,
            "" if head.starts_with(b"#!") => script,
            _ => Kind::Other,
        }
    }

    pub(crate) fn of_bundle(ext: &str) -> Option<Kind> {
        Some(match ext {
            "app" => Kind::App,
            "pages" | "rtfd" => Kind::Doc,
            "numbers" => Kind::Sheet,
            "key" => Kind::Slides,
            "photoslibrary" => Kind::Image,
            "musiclibrary" => Kind::Audio,
            "tvlibrary" => Kind::Video,
            "bundle" | "framework" | "plugin" | "xcarchive" => Kind::Other,
            _ => return None,
        })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const KEY_NAMES: &[&str] = &[
    ".env",
    ".netrc",
    ".pgpass",
    "authorized_keys",
    "credentials",
    "kubeconfig",
];
const KEY_PREFIXES: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".env."];
const ENV_TEMPLATE_EXTS: &[&str] = &["example", "sample", "template", "dist"];
const KEY_EXTS: &[&str] = &[
    "pem", "key", "p12", "pfx", "jks", "keystore", "ppk", "kdbx", "gpg", "asc", "crt", "cer",
    "csr", "pub",
];
const DOC_EXTS: &[&str] = &[
    "doc", "docx", "odt", "rtf", "txt", "md", "markdown", "rst", "org", "tex", "epub", "pages",
];
const SHEET_EXTS: &[&str] = &["xls", "xlsx", "ods", "csv", "tsv", "numbers"];
const SLIDES_EXTS: &[&str] = &["ppt", "pptx", "odp"];
const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "avif", "bmp", "tif", "tiff", "svg",
    "ico", "psd", "xcf", "raw", "cr2", "cr3", "nef", "arw", "dng",
];
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "mov", "webm", "avi", "m4v", "wmv", "flv"];
const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma",
];
const ARCHIVE_EXTS: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "iso", "deb", "rpm", "dmg", "apk",
];
const APP_EXTS: &[&str] = &["exe", "msi", "appimage"];
const SCRIPT_EXTS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "ps1", "bat", "cmd", "py", "rb", "pl", "lua", "php", "js", "mjs",
    "cjs", "ts",
];
const CODE_EXTS: &[&str] = &[
    "rs", "go", "c", "h", "cc", "cpp", "hpp", "java", "kt", "kts", "swift", "dart", "scala", "cs",
    "r", "jl", "ex", "exs", "erl", "hs", "ml", "clj", "sql", "tsx", "jsx", "vue", "svelte", "html",
    "css", "scss",
];
const CONFIG_EXTS: &[&str] = &[
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "cfg",
    "xml",
    "plist",
    "properties",
];

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn classifies_files() {
        let kind = |name, ext, in_project, head| Kind::of_file(name, ext, in_project, head);
        assert_eq!(kind("id_ed25519", "", false, b""), Kind::Key);
        assert_eq!(kind(".env.local", "local", true, b""), Kind::Key);
        assert_eq!(kind(".env.example", "example", true, b""), Kind::Config);
        assert_eq!(kind("backup.sh", "sh", false, b""), Kind::Script);
        assert_eq!(kind("backup.sh", "sh", true, b""), Kind::Code);
        assert_eq!(kind("deploy", "", false, b"#!/b"), Kind::Script);
        assert_eq!(kind("trellis", "", false, b"\x7fELF"), Kind::Other);
        assert_eq!(kind("main.rs", "rs", false, b""), Kind::Code);
        assert_eq!(kind("Taxes.PDF", "pdf", false, b""), Kind::Pdf);
        assert_eq!(kind("server.key", "key", false, b"----"), Kind::Key);
        assert_eq!(kind("Pitch.key", "key", false, b"PK\x03\x04"), Kind::Slides);
        assert_eq!(kind("warp.AppImage", "appimage", false, b""), Kind::App);
        assert_eq!(kind("notes", "", false, b""), Kind::Other);
    }

    #[test]
    fn classifies_bundles() {
        assert_eq!(Kind::of_bundle("app"), Some(Kind::App));
        assert_eq!(Kind::of_bundle("photoslibrary"), Some(Kind::Image));
        assert_eq!(Kind::of_bundle("d"), None);
    }

    #[test]
    fn parses_names() {
        assert_eq!(Kind::from_name("Images"), Some(Kind::Image));
        assert_eq!(Kind::from_name("slides"), Some(Kind::Slides));
        assert_eq!(Kind::from_name("keys"), Some(Kind::Key));
        assert_eq!(Kind::from_name("banana"), None);
    }
}
