// SPDX-License-Identifier: MPL-2.0

use anyhow::Context;
use image::imageops::crop_imm;
use image::io::Reader as ImageReader;
use image::{GenericImage, GenericImageView, Rgb, RgbImage};
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

/// Image output path with deepzoom convention.
fn img_out_path(dir: &Path, tx: usize, ty: usize) -> PathBuf {
    dir.join(format!("{}_{}.jpg", tx, ty))
}

fn run(c: &seahorse::Context) -> anyhow::Result<()> {
    let img_paths: Vec<&Path> = c.args.iter().map(Path::new).collect();
    assert!(!img_paths.is_empty(), "At least one input image is needed");

    // Create output directory.
    let output_dir = Path::new("tiles");
    let img_output_dir = output_dir.join("tiles_files");
    std::fs::create_dir_all(&img_output_dir)?;

    // Read the image sizes.
    let mut img_sizes = Vec::with_capacity(img_paths.len());
    for path in &img_paths {
        let size = imagesize::size(path)?;
        img_sizes.push((size.width, size.height));
        eprintln!("height: {}", size.height);
    }

    // Assert that all image heights are the same.
    let height = img_sizes.iter().min_by_key(|(_, h)| h).unwrap().1;
    for ((_, h), path) in img_sizes.iter().zip(&img_paths) {
        if *h > height {
            eprintln!(
                "BEWARE that image {} has height {} > {}",
                path.display(),
                h,
                height
            );
        }
    }

    // Crop the image sizes as will do the algorithm.
    img_sizes.iter_mut().for_each(|(_, h)| *h = height);

    // Compute the total width of the panorama.
    let width_sum: usize = img_sizes.iter().map(|(w, _)| w).sum();

    // Compute the number of levels required with the tiles sizes.
    let tile_size = 512;
    let tile_count_width = (width_sum + tile_size - 1) / tile_size;
    let tile_count_height = (height + tile_size - 1) / tile_size;
    // let width_levels = levels_for(tile_count_width);
    // let height_levels = levels_for(tile_count_height);

    // let's choose a number of levels half way from the min and max
    // which correspond to the number of levels for the smallest and longest dimensions.
    // let levels = (width_levels + height_levels + 1) / 2;
    let levels = levels_for(width_sum.max(height));
    eprintln!("levels: {}", levels);

    // Start generating the images at the highest resolution level.
    // TODO: when vertical panoramas inputs will be allowed,
    // be careful with the image access order.
    let level_out_dir = img_output_dir.join((levels - 1).to_string());
    std::fs::create_dir_all(&level_out_dir)?;
    let mut extractor = ImgExtractor::new(&img_paths, &img_sizes);
    for tx in 0..tile_count_width {
        for ty in 0..tile_count_height {
            let img = extractor.extract(height, tile_size, tx, ty)?;
            img.save(img_out_path(&level_out_dir, tx, ty))?;
        }
    }

    // Now, we need to take 2x2 blocs of images
    // and complete the pyramid of levels by halfing the resolution each time.
    let mut parent_x_tiles = tile_count_width;
    let mut parent_y_tiles = tile_count_height;
    for parent_level in (1..levels).rev() {
        let (child_x_tiles, child_y_tiles) = compute_half_resolutions(
            &img_output_dir,
            parent_level,
            parent_x_tiles,
            parent_y_tiles,
        )?;
        parent_x_tiles = child_x_tiles;
        parent_y_tiles = child_y_tiles;
    }

    // Write the ImageProperties.xml file.
    let xml_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Image xmlns="http://schemas.microsoft.com/deepzoom/2008" TileSize="{}" Overlap="0" Format="jpg"><Size Width="{}" Height="{}"/></Image>"#,
        tile_size, width_sum, height
    );
    std::fs::write("tiles/tiles.dzi", xml_content).context("Failed to write xml file")
}

/// Compute half resolution images and output the number of tiles generated.
fn compute_half_resolutions(
    img_output_dir: &Path,
    previous_lvl: usize,
    tile_count_width: usize,
    tile_count_height: usize,
) -> anyhow::Result<(usize, usize)> {
    let level_out_dir = img_output_dir.join((previous_lvl - 1).to_string());
    std::fs::create_dir_all(&level_out_dir)?;
    let half_tile_count_width = (tile_count_width + 1) / 2;
    let half_tile_count_height = (tile_count_height + 1) / 2;
    for tx in 0..half_tile_count_width {
        for ty in 0..half_tile_count_height {
            let img_path =
                |tx, ty| img_out_path(&img_output_dir.join(previous_lvl.to_string()), tx, ty);
            let top_left: RgbImage = ImageReader::open(img_path(tx * 2, ty * 2))?
                .decode()?
                .into_rgb8();
            let top_right: RgbImage = match ImageReader::open(img_path(tx * 2 + 1, ty * 2)) {
                Ok(reader) => reader.decode()?.into_rgb8(),
                Err(_) => RgbImage::new(0, top_left.height()),
            };
            let bottom_left: RgbImage = match ImageReader::open(img_path(tx * 2, ty * 2 + 1)) {
                Ok(reader) => reader.decode()?.into_rgb8(),
                Err(_) => RgbImage::new(top_left.width(), 0),
            };
            let bottom_right: RgbImage = match ImageReader::open(img_path(tx * 2 + 1, ty * 2 + 1)) {
                Ok(reader) => reader.decode()?.into_rgb8(),
                Err(_) => RgbImage::new(top_right.width(), bottom_left.height()),
            };
            let half_img: RgbImage = half_res(top_left, top_right, bottom_left, bottom_right);
            half_img.save(img_out_path(&level_out_dir, tx, ty))?;
        }
    }
    Ok((half_tile_count_width, half_tile_count_height))
}

fn half_res(
    top_left: RgbImage,
    top_right: RgbImage,
    bottom_left: RgbImage,
    bottom_right: RgbImage,
) -> RgbImage {
    assert_eq!(top_left.width(), bottom_left.width());
    assert_eq!(top_left.height(), top_right.height());
    assert_eq!(bottom_right.width(), top_right.width());
    assert_eq!(bottom_right.height(), bottom_left.height());
    let half_width = (top_left.width() + top_right.width() + 1) / 2;
    let half_height = (top_left.height() + bottom_left.height() + 1) / 2;

    // Helper closure to use the correct image to retrieve a given pixel.
    let img_source_and_offset = |x, y| {
        if x < top_left.width() && y < top_left.height() {
            (&top_left, 0, 0)
        } else if x < top_left.width() {
            (&bottom_left, 0, top_left.height())
        } else if y < top_left.height() {
            (&top_right, top_left.width(), 0)
        } else {
            (&bottom_right, top_left.width(), top_left.height())
        }
    };
    // Helper closure to extract the correct pixel from the correct image.
    let extract_pixel = |target_x, target_y| {
        let (img, offset_x, offset_y) = img_source_and_offset(target_x, target_y);
        let local_x = target_x - offset_x;
        let local_y = target_y - offset_y;
        if local_x >= img.width() || local_y >= img.height() {
            None
        } else {
            Some(*img.get_pixel(local_x, local_y))
        }
    };
    RgbImage::from_fn(half_width, half_height, |x, y| {
        let pixels = [
            extract_pixel(2 * x, 2 * y),
            extract_pixel(2 * x + 1, 2 * y),
            extract_pixel(2 * x, 2 * y + 1),
            extract_pixel(2 * x + 1, 2 * y + 1),
        ];
        // Compute the mean of the 4 subpixels (maybe less if near a border)
        let (pix_count, pix_sum): (u16, [u16; 3]) = IntoIterator::into_iter(pixels).fold(
            (0, [0, 0, 0]),
            |(count, [r_sum, g_sum, b_sum]), maybe_pix| match maybe_pix {
                Some(pix) => (
                    count + 1,
                    [
                        r_sum + pix.0[0] as u16,
                        g_sum + pix.0[1] as u16,
                        b_sum + pix.0[2] as u16,
                    ],
                ),
                None => (count, [r_sum, g_sum, b_sum]),
            },
        );
        Rgb([
            (pix_sum[0] / pix_count) as u8,
            (pix_sum[1] / pix_count) as u8,
            (pix_sum[2] / pix_count) as u8,
        ])
    })
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
            sizes: img_sizes.to_vec(),
            full_width,
            full_height,
            img_cache: HashMap::default(),
        }
    }
    fn extract(
        &mut self,
        img_height: usize,
        tile_size: usize,
        tx: usize,
        ty: usize,
    ) -> anyhow::Result<RgbImage> {
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
        let crop = |img: RgbImage| crop_imm(&img, 0, 0, img.width(), img_height as u32).to_image();
        for (id, (w, _)) in self.sizes.iter().enumerate() {
            if left <= accum_left + w {
                eprintln!("Using image {} for tile ({}, {})", id, tx, ty);
                // load the image if not
                let img: &RgbImage = match self.img_cache.entry(id) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(crop(
                        ImageReader::open(&self.paths[id])?.decode()?.into_rgb8(),
                    )),
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
