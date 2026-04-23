use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

const LINUX_ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];
const ICO_ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const ICNS_ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

struct IconSpec {
    name: &'static str,
    source_svg: &'static str,
    linux_png_prefix: &'static str,
    windows_ico: &'static str,
    macos_icns: &'static str,
}

const ICON_SPECS: [IconSpec; 2] = [
    IconSpec {
        name: "papyru2_pin_file",
        source_svg: "assets/icons/source/pin-ok-red.svg",
        linux_png_prefix: "assets/icons/linux/papyru2_pin_file",
        windows_ico: "assets/icons/windows/papyru2_pin_file_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_pin_file_app_icon.icns",
    },
    IconSpec {
        name: "papyru2_textfile_import",
        source_svg: "assets/icons/source/import-2-yg.svg",
        linux_png_prefix: "assets/icons/linux/papyru2_textfile_import",
        windows_ico: "assets/icons/windows/papyru2_textfile_import_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_textfile_import_app_icon.icns",
    },
];

fn main() {
    if let Err(error) = run() {
        eprintln!("generate_app_icons failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    for spec in ICON_SPECS {
        generate_icon_set(&spec).with_context(|| format!("failed to generate {}", spec.name))?;
    }

    println!("generated_app_icon_sets={}", ICON_SPECS.len());
    Ok(())
}

fn generate_icon_set(spec: &IconSpec) -> Result<()> {
    ensure_path_exists(Path::new(spec.source_svg), "source SVG")?;

    for size in LINUX_ICON_SIZES {
        let png_path = linux_png_path(spec, size);
        write_svg_png(Path::new(spec.source_svg), &png_path, size)?;
    }

    let ico_images = render_icon_images(Path::new(spec.source_svg), &ICO_ICON_SIZES)?;
    write_ico(Path::new(spec.windows_ico), &ico_images)?;

    let icns_images = render_icon_images(Path::new(spec.source_svg), &ICNS_ICON_SIZES)?;
    write_icns(Path::new(spec.macos_icns), &icns_images)?;

    Ok(())
}

fn render_icon_images(source_svg: &Path, sizes: &[u32]) -> Result<Vec<(u32, Vec<u8>)>> {
    let mut images = Vec::with_capacity(sizes.len());
    for size in sizes {
        images.push((*size, render_svg_png_data(source_svg, *size)?));
    }
    Ok(images)
}

fn write_svg_png(source_svg: &Path, output_path: &Path, size: u32) -> Result<()> {
    ensure_parent_dir(output_path)?;
    let data = render_svg_png_data(source_svg, size)?;
    fs::write(output_path, data)
        .with_context(|| format!("failed to save {}", output_path.display()))?;
    Ok(())
}

fn render_svg_png_data(source_svg: &Path, size: u32) -> Result<Vec<u8>> {
    let svg_data =
        fs::read(source_svg).with_context(|| format!("failed to read {}", source_svg.display()))?;
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &options)
        .with_context(|| format!("failed to parse {}", source_svg.display()))?;
    let svg_size = tree.size();
    let scale_x = size as f32 / svg_size.width();
    let scale_y = size as f32 / svg_size.height();

    let mut pixmap = Pixmap::new(size, size).context("failed to allocate icon pixmap")?;
    resvg::render(
        &tree,
        Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().context("failed to encode icon PNG")
}

fn write_ico(output_path: &Path, png_images: &[(u32, Vec<u8>)]) -> Result<()> {
    ensure_parent_dir(output_path)?;
    let mut file = fs::File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;

    write_u16_le(&mut file, 0)?;
    write_u16_le(&mut file, 1)?;
    write_u16_le(&mut file, png_images.len().try_into()?)?;

    let mut data_offset = 6 + png_images.len() * 16;
    for (size, data) in png_images {
        file.write_all(&[ico_dimension_byte(*size), ico_dimension_byte(*size), 0, 0])?;
        write_u16_le(&mut file, 1)?;
        write_u16_le(&mut file, 32)?;
        write_u32_le(&mut file, data.len().try_into()?)?;
        write_u32_le(&mut file, data_offset.try_into()?)?;
        data_offset += data.len();
    }

    for (_, data) in png_images {
        file.write_all(data)?;
    }

    Ok(())
}

fn write_icns(output_path: &Path, png_images: &[(u32, Vec<u8>)]) -> Result<()> {
    ensure_parent_dir(output_path)?;
    let mut file = fs::File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;

    let total_len: usize = 8 + png_images
        .iter()
        .map(|(_, data)| 8 + data.len())
        .sum::<usize>();
    file.write_all(b"icns")?;
    write_u32_be(&mut file, total_len.try_into()?)?;

    for (size, data) in png_images {
        file.write_all(icns_type(*size)?)?;
        write_u32_be(&mut file, (8 + data.len()).try_into()?)?;
        file.write_all(data)?;
    }

    Ok(())
}

fn linux_png_path(spec: &IconSpec, size: u32) -> PathBuf {
    PathBuf::from(format!("{}_{}x{}.png", spec.linux_png_prefix, size, size))
}

fn ico_dimension_byte(size: u32) -> u8 {
    if size >= 256 { 0 } else { size as u8 }
}

fn icns_type(size: u32) -> Result<&'static [u8; 4]> {
    match size {
        16 => Ok(b"icp4"),
        32 => Ok(b"icp5"),
        64 => Ok(b"icp6"),
        128 => Ok(b"ic07"),
        256 => Ok(b"ic08"),
        512 => Ok(b"ic09"),
        1024 => Ok(b"ic10"),
        other => bail!("unsupported ICNS icon size {other}"),
    }
}

fn ensure_path_exists(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        bail!("required {label} is missing: {}", path.display());
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    Ok(())
}

fn write_u16_le(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32_le(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32_be(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_be_bytes())
}
