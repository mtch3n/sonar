//! Fingerprint: a perceptual hash of each image and video, so ones that look the
//! same are found even at other sizes or qualities.
//!
//! Videos are fingerprinted the way Stash does it: 25 frames from across the video,
//! skipping the first and last 5%, laid out 5 × 5 and hashed as one picture. The
//! hash is a DCT perceptual hash, as goimagehash's `PerceptionHash` makes it.

use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use image::{DynamicImage, GenericImage, RgbImage, imageops::FilterType};
use serde::Deserialize;
use serde_json::json;

pub const ID: &str = "fingerprint";
/// Changes when fingerprints are made differently.
pub const VERSION: &str = "1";
pub const IMAGE: &str = "phash";
pub const VIDEO: &str = "phash-video";
/// Frames of each video, laid out as a grid of this many columns and rows.
pub const FRAMES: usize = 25;
const GRID: u32 = 5;
/// How wide each frame is in the grid, as Stash makes it.
const FRAME_WIDTH: u32 = 160;

#[derive(Deserialize)]
struct Request {
    path: PathBuf,
    kind: String,
    #[serde(default)]
    frames: Vec<PathBuf>,
}

/// Answers each file on stdin with its fingerprint.
pub fn serve() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let answer = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match fingerprint(&request) {
                Ok((algo, bits)) => {
                    json!({"fingerprint": {"algo": algo, "bits": format!("{bits:016x}")}})
                }
                Err(err) => json!({"error": err}),
            },
            Err(err) => json!({"error": format!("couldn't read the request: {err}")}),
        };
        if writeln!(stdout, "{answer}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return;
        }
    }
}

fn fingerprint(request: &Request) -> Result<(&'static str, u64), String> {
    if request.kind == "video" {
        Ok((VIDEO, phash(&sprite(&request.frames)?)))
    } else {
        Ok((IMAGE, phash(&open(&request.path)?)))
    }
}

fn open(path: &Path) -> Result<DynamicImage, String> {
    image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|err| err.to_string())?
        .decode()
        .map_err(|err| format!("couldn't read the picture: {err}"))
}

/// The frames of a video in a 5 × 5 grid, each scaled to 160 pixels wide.
fn sprite(frames: &[PathBuf]) -> Result<DynamicImage, String> {
    if frames.len() != FRAMES {
        return Err(format!("needs {FRAMES} frames, got {}", frames.len()));
    }
    let frames = frames
        .iter()
        .map(|f| open(f).map(|f| f.resize(FRAME_WIDTH, u32::MAX, FilterType::Triangle)))
        .collect::<Result<Vec<_>, _>>()?;
    let height = frames[0].height();
    let mut grid = RgbImage::new(FRAME_WIDTH * GRID, height * GRID);
    for (i, frame) in frames.iter().enumerate() {
        let (x, y) = (i as u32 % GRID * FRAME_WIDTH, i as u32 / GRID * height);
        let frame = frame.resize_exact(FRAME_WIDTH, height, FilterType::Triangle);
        grid.copy_from(&frame.to_rgb8(), x, y)
            .map_err(|err| err.to_string())?;
    }
    Ok(DynamicImage::ImageRgb8(grid))
}

/// A 64-bit DCT perceptual hash: the picture in gray at 64 × 64, its lowest 8 × 8
/// frequencies, and a bit for each saying whether it's above their median.
pub fn phash(picture: &DynamicImage) -> u64 {
    const SIZE: usize = 64;
    let gray = picture
        .resize_exact(SIZE as u32, SIZE as u32, FilterType::Triangle)
        .to_rgb8();
    let mut pixels: Vec<f64> = gray
        .pixels()
        .map(|p| 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]))
        .collect();
    // A 2-D DCT is a 1-D DCT of each row, then of each column.
    for row in pixels.chunks_exact_mut(SIZE) {
        let transformed = dct(row);
        row.copy_from_slice(&transformed);
    }
    for x in 0..SIZE {
        let column: Vec<f64> = (0..SIZE).map(|y| pixels[y * SIZE + x]).collect();
        for (y, value) in dct(&column).into_iter().enumerate() {
            pixels[y * SIZE + x] = value;
        }
    }
    let low: Vec<f64> = (0..8)
        .flat_map(|y| (0..8).map(move |x| (y, x)))
        .map(|(y, x)| pixels[y * SIZE + x])
        .collect();
    let mut sorted = low.clone();
    sorted.sort_by(f64::total_cmp);
    let median = (sorted[31] + sorted[32]) / 2.0;
    low.iter()
        .enumerate()
        .filter(|(_, v)| **v > median)
        .fold(0u64, |bits, (i, _)| bits | 1 << (63 - i))
}

fn dct(input: &[f64]) -> Vec<f64> {
    let n = input.len() as f64;
    (0..input.len())
        .map(|k| {
            input
                .iter()
                .enumerate()
                .map(|(i, x)| x * (std::f64::consts::PI / n * (i as f64 + 0.5) * k as f64).cos())
                .sum()
        })
        .collect()
}

/// How many bits two fingerprints differ in: 0 is the same picture, and up to
/// about 10 is the same one resized, recompressed or slightly edited.
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn picture(width: u32, height: u32, seed: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            let v = ((x * 7 + y * 3 + seed * 13) % 255) as u8;
            let w = (((x / 20 + y / 20 + seed) % 2) * 200) as u8;
            Rgb([v, w, v / 2])
        }))
    }

    #[test]
    fn same_picture_at_another_size_hashes_alike() {
        let big = picture(800, 600, 1);
        let small = big.resize(200, 150, FilterType::Triangle);
        let other = picture(800, 600, 4);
        assert!(distance(phash(&big), phash(&small)) <= 4);
        assert!(distance(phash(&big), phash(&other)) > 10);
    }

    #[test]
    fn videos_are_hashed_as_a_grid_of_frames() {
        let dir = tempfile::tempdir().unwrap();
        let frames: Vec<PathBuf> = (0..FRAMES)
            .map(|i| {
                let path = dir.path().join(format!("{i:02}.jpg"));
                picture(512, 288, i as u32).save(&path).unwrap();
                path
            })
            .collect();
        let request = Request {
            path: dir.path().join("clip.mp4"),
            kind: "video".into(),
            frames: frames.clone(),
        };
        let (algo, first) = fingerprint(&request).unwrap();
        assert_eq!(algo, VIDEO);
        let again = fingerprint(&request).unwrap().1;
        assert_eq!(first, again);
        let short = Request {
            frames: frames[..3].to_vec(),
            ..request
        };
        assert!(fingerprint(&short).is_err());
    }
}
