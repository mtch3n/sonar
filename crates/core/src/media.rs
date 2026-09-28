//! Frames and lengths of videos, through ffmpeg and ffprobe when they're installed.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

/// Frames taken from each video: enough for a perceptual fingerprint of a 5 × 5
/// grid, from which models that want fewer get an even spread.
pub(crate) const FRAMES: usize = 25;
/// How wide frames are kept, in pixels: enough for a model to read a scene.
const FRAME_WIDTH: u32 = 512;

/// Whether ffmpeg and ffprobe can be run.
pub(crate) fn available() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|program| {
        Command::new(program)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// A video's length in seconds.
pub(crate) fn duration(path: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("couldn't run ffprobe")?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()
        .filter(|d: &f64| d.is_finite() && *d > 0.0)
        .context("ffprobe found no length")
}

/// [`FRAMES`] frames from across the video, skipping the first and last 5% where
/// intros and credits are, kept in `dir` so they're taken once.
pub(crate) fn frames(path: &Path, duration: f64, dir: &Path) -> Result<Vec<PathBuf>> {
    let wanted: Vec<PathBuf> = (0..FRAMES)
        .map(|i| dir.join(format!("{i:02}.jpg")))
        .collect();
    if wanted.iter().all(|f| f.exists()) {
        return Ok(wanted);
    }
    fs::create_dir_all(dir)?;
    let step = duration * 0.9 / FRAMES as f64;
    for (i, frame) in wanted.iter().enumerate() {
        let at = duration * 0.05 + step * (i as f64 + 0.5);
        // Seeking before the input is fast: it jumps to the nearest keyframe.
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-ss", &format!("{at:.3}"), "-i"])
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={FRAME_WIDTH}:-2"),
                "-q:v",
                "4",
            ])
            .arg(frame)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("couldn't run ffmpeg")?;
        if !status.success() || !frame.exists() {
            bail!("ffmpeg couldn't take a frame at {at:.1}s");
        }
    }
    Ok(wanted)
}

/// `count` frames spread evenly over `frames`.
pub(crate) fn spread(frames: &[PathBuf], count: usize) -> Vec<PathBuf> {
    if count >= frames.len() {
        return frames.to_vec();
    }
    (0..count)
        .map(|i| frames[(i * frames.len() + frames.len() / 2) / count].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_frames_evenly() {
        let frames: Vec<PathBuf> = (0..25).map(|i| PathBuf::from(format!("{i}"))).collect();
        let picked: Vec<String> = spread(&frames, 4)
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(picked, ["3", "9", "15", "21"]);
        assert_eq!(spread(&frames, 30).len(), 25);
    }

    #[test]
    fn reads_a_videos_length_and_frames() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("clip.mp4");
        let made = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=4:size=320x240:rate=10",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(made.success());
        let length = duration(&video).unwrap();
        assert!((length - 4.0).abs() < 0.2, "{length}");
        let frames = frames(&video, length, &dir.path().join("frames")).unwrap();
        assert_eq!(frames.len(), FRAMES);
        assert!(frames.iter().all(|f| f.metadata().unwrap().len() > 0));
    }
}
