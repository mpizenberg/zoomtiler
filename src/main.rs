// SPDX-License-Identifier: MPL-2.0

use anyhow::Context;
use image::io::Reader as ImageReader;
use image::{GenericImage, GenericImageView, RgbImage};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let app_name = env!("CARGO_PKG_NAME");
    let app = seahorse::App::new(app_name)
        .description(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .version(env!("CARGO_PKG_VERSION"))
        .usage(format!("{} IMG...", app_name))
        .action(|c| run(c).unwrap());
    app.run(args);
}

fn run(c: &seahorse::Context) -> anyhow::Result<()> {
    let img_paths: Vec<&Path> = c.args.iter().map(Path::new).collect();
    assert!(!img_paths.is_empty(), "At least one input image is needed");

    // Read the image sizes.
    let mut img_sizes = Vec::with_capacity(img_paths.len());
    for path in &img_paths {
        let size = imagesize::size(path)?;
        img_sizes.push((size.width, size.height));
        eprintln!("height: {}", size.height);
    }

    // Assert that all image heights are the same.
    let height = img_sizes[0].1;
    for (_, h) in &img_sizes {
        assert_eq!(*h, height);
    }

    // Compute the total width of the panorama.
    let width_sum: usize = img_sizes.iter().map(|(w, _)| w).sum();

    // Compute the number of levels required with the tiles sizes.
    let tile_size = 512;
    let tile_count_width = (width_sum + tile_size - 1) / tile_size;
    let tile_count_height = (height + tile_size - 1) / tile_size;
    let width_levels = levels_for(tile_count_width);
    let height_levels = levels_for(tile_count_height);

    // let's choose a number of levels half way from the min and max
    // which correspond to the number of levels for the smallest and longest dimensions.
    let levels = (width_levels + height_levels + 1) / 2;
    eprintln!("height_levels: {}", height_levels);
    eprintln!("width_levels: {}", width_levels);
    eprintln!("levels: {}", levels);

    // Start generating the images at the highest resolution level.
    // TODO: when vertical panoramas inputs will be allowed,
    // be careful with the image access order.
    let mut extractor = ImgExtractor::new(&img_paths, &img_sizes);
    for tx in 0..tile_count_width {
        for ty in 0..tile_count_height {
            let img = extractor.extract(tile_size, tx, ty)?;
            let img_path = format!("tiles/{}_{}_{}.jpg", levels - 1, tx, ty);
            img.save(&img_path)?;
        }
    }

    Ok(())

    // // Build the concatenated image.
    // let mut concat_img = RgbImage::new(width_sum, max_height);
    // let mut width_acc = 0;
    // for path in &img_paths {
    //     eprintln!("Loading {}", path.display());
    //     let img = ImageReader::open(path)?.decode()?.into_rgb8();
    //     concat_img.copy_from(&img, width_acc, 0)?;
    //     width_acc += img.width();
    // }
    //
    // // Save the concatenated image to disk.
    // concat_img
    //     .save("out.jpg")
    //     .context("Failed to save concatenated image")
}

struct ImgExtractor {
    paths: Vec<PathBuf>,
    sizes: Vec<(usize, usize)>,
    full_width: usize,
    full_height: usize,
    img_cache: HashMap<usize, RgbImage>,
}

impl ImgExtractor {
    fn new(img_paths: &[&Path], img_sizes: &[(usize, usize)]) -> Self {
        let full_height = img_sizes[0].1;
        let full_width = img_sizes.iter().map(|(w, _)| *w).sum();
        Self {
            paths: img_paths.iter().map(PathBuf::from).collect(),
            sizes: img_sizes.iter().cloned().collect(),
            full_width,
            full_height,
            img_cache: HashMap::default(),
        }
    }
    fn extract(&mut self, tile_size: usize, tx: usize, ty: usize) -> anyhow::Result<RgbImage> {
        let left = tx * tile_size;
        let top = ty * tile_size;
        let right = (left + tile_size).min(self.full_width);
        let bottom = (top + tile_size).min(self.full_height);

        // Initialize an RgbImage of the correct size.
        let tile_width = tile_size.min(right - left) as u32;
        let tile_height = tile_size.min(bottom - top) as u32;
        let mut img_tile = RgbImage::new(tile_width, tile_height);

        // Identify the images that need to be loaded.
        // Clear all images from the cache that are not in that list.
        // Load images that are not loaded yet in the cache.
        let mut accum_left = 0;
        for (id, (w, _)) in self.sizes.iter().enumerate() {
            if left <= accum_left + w {
                eprintln!("Using image {} for tile ({}, {})", id, tx, ty);
                // load the image if not
                let img: &RgbImage = match self.img_cache.entry(id) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => {
                        v.insert(ImageReader::open(&self.paths[id])?.decode()?.into_rgb8())
                    }
                };
                // copy the correct view
                let inner_left = (left as i64 - accum_left as i64).max(0) as u32;
                let inner_right = (right - accum_left).min(*w) as u32;
                let inner_width = inner_right - inner_left;
                let img_view = img.view(inner_left, top as u32, inner_width, (bottom - top) as u32);

                let tile_inner_left = (accum_left as i64 - left as i64).max(0) as u32;
                img_tile.copy_from(&img_view, tile_inner_left, 0)?;
            } else if self.img_cache.contains_key(&id) {
                // unload the image if it was loaded
                self.img_cache.remove(&id);
            }
            accum_left += w;
            if accum_left >= right {
                break;
            }
        }

        Ok(img_tile)
    }
}

struct Level {
    level: usize,
    tile_count_width: usize,
    tile_count_height: usize,
}

/// Compute the number of levels required for a given amount of tiles.
fn levels_for(n: usize) -> usize {
    assert!(n > 0);
    if n == 1 {
        1
    } else {
        let l2 = log_2(n);
        if n % (2_usize.pow(l2 as u32)) != 0 {
            l2 + 2
        } else {
            l2 + 1
        }
    }
}

fn log_2(x: usize) -> usize {
    num_bits::<usize>() - x.leading_zeros() as usize - 1
}

const fn num_bits<T>() -> usize {
    std::mem::size_of::<T>() * 8
}
