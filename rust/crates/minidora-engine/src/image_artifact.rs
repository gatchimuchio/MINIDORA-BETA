//! Bounded, deterministic drawing and region edits of actual raster pixels.
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{
    ColorType, DynamicImage, GenericImageView, ImageEncoder, ImageFormat, ImageReader, Limits,
    Rgba, RgbaImage,
};
use minidora_domain::{
    Error, ImageOperation, ImagePlacement, ImageRectangle, ImageShape, RequestAttachment, Result,
    failure,
};
use std::{io, time::Instant};

const MAX_SIDE: u32 = 2048;
const MAX_SHAPES: usize = 16;
const MAX_DRAW_PIXELS: u64 = 16_777_216;
const MAX_DECODE_ALLOC: u64 = 40 * 1024 * 1024;
pub(crate) const MAX_PNG_BYTES: usize = 6 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn invalid_image() -> Error {
    failure(
        "IMAGE_INVALID",
        "PNGまたはJPEGの画像データを確認できませんでした。",
    )
}

fn invalid_geometry() -> Error {
    failure(
        "IMAGE_GEOMETRY_INVALID",
        "幅と高さが正の範囲を画像内に指定してください。円には同じ幅と高さが必要です。",
    )
}

fn resource_limit() -> Error {
    failure(
        "IMAGE_ARTIFACT_LIMIT",
        "画像の寸法、描画範囲、またはPNGのサイズが上限を超えました。",
    )
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "DEADLINE",
            "画像処理の途中で元の依頼の締切に達しました。",
        ));
    }
    Ok(())
}

fn check_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(invalid_geometry());
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(resource_limit());
    }
    Ok(())
}

fn checked_rectangle(rect: &ImageRectangle, width: u32, height: u32) -> Result<(u32, u32)> {
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or_else(invalid_geometry)?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or_else(invalid_geometry)?;
    if rect.width == 0 || rect.height == 0 || right > width || bottom > height {
        return Err(invalid_geometry());
    }
    Ok((right, bottom))
}

fn png_container(bytes: &[u8]) -> Result<()> {
    let mut chunks = bytes
        .strip_prefix(PNG_SIGNATURE)
        .ok_or_else(invalid_image)?;
    if !chunks.starts_with(b"\0\0\0\x0dIHDR") {
        return Err(invalid_image());
    }
    while let Some(header) = chunks.get(..8) {
        let size = u32::from_be_bytes(header[..4].try_into().map_err(|_| invalid_image())?);
        let end = 12usize
            .checked_add(size as usize)
            .ok_or_else(invalid_image)?;
        let rest = chunks.get(end..).ok_or_else(invalid_image)?;
        match &header[4..8] {
            b"IEND" if chunks == b"\0\0\0\0IEND\xae\x42\x60\x82" => return Ok(()),
            b"IEND" => return Err(invalid_image()),
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(failure(
                    "IMAGE_ANIMATION_UNSUPPORTED",
                    "アニメーションPNGの編集には対応していません。",
                ));
            }
            _ => chunks = rest,
        }
    }
    Err(invalid_image())
}

fn decode(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage> {
    if format == ImageFormat::Png {
        png_container(bytes)?;
    }
    let mut reader = ImageReader::with_format(io::Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let raster = reader.decode().map_err(|error| match error {
        image::ImageError::Limits(_) => resource_limit(),
        _ => invalid_image(),
    })?;
    check_dimensions(raster.width(), raster.height())?;
    Ok(raster)
}

pub(crate) fn inspect_png(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() > MAX_PNG_BYTES {
        return Err(resource_limit());
    }
    Ok(decode(bytes, ImageFormat::Png)?.dimensions())
}

fn draw_shape(raster: &mut RgbaImage, placement: &ImagePlacement, deadline: Instant) -> Result<()> {
    let r = &placement.rectangle;
    let [red, green, blue] = placement.color;
    let fill = Rgba([red, green, blue, 255]);
    let width = i64::from(r.width);
    let height = i64::from(r.height);
    for y in r.y..r.y + r.height {
        check_deadline(deadline)?;
        let pixel_y = i64::from(2 * (y - r.y) + 1);
        for x in r.x..r.x + r.width {
            let dx = i64::from(2 * (x - r.x) + 1) - width;
            // Test the pixel's center in doubled integer coordinates.
            let inside = match placement.shape {
                ImageShape::Rectangle => true,
                ImageShape::Circle => {
                    let dy = pixel_y - height;
                    dx * dx + dy * dy <= width * width
                }
                ImageShape::Triangle => 2 * height * dx.abs() <= width * pixel_y,
            };
            if inside {
                raster.put_pixel(x, y, fill);
            }
        }
    }
    Ok(())
}

fn draw(
    width: u32,
    height: u32,
    background: [u8; 3],
    shapes: &[ImagePlacement],
    deadline: Instant,
) -> Result<DynamicImage> {
    check_dimensions(width, height)?;
    if shapes.len() > MAX_SHAPES {
        return Err(resource_limit());
    }
    let mut candidate_pixels = 0;
    for placement in shapes {
        let r = &placement.rectangle;
        checked_rectangle(r, width, height)?;
        if matches!(placement.shape, ImageShape::Circle) && r.width != r.height {
            return Err(invalid_geometry());
        }
        candidate_pixels += u64::from(r.width) * u64::from(r.height);
    }
    if candidate_pixels > MAX_DRAW_PIXELS {
        return Err(resource_limit());
    }
    let [red, green, blue] = background;
    let mut raster = RgbaImage::from_pixel(width, height, Rgba([red, green, blue, 255]));
    for placement in shapes {
        draw_shape(&mut raster, placement, deadline)?;
    }
    Ok(DynamicImage::ImageRgba8(raster))
}

fn edit(
    reference: &RequestAttachment,
    rect: &ImageRectangle,
    color: [u8; 3],
    deadline: Instant,
) -> Result<DynamicImage> {
    let encoded = reference.image_bytes()?.ok_or_else(invalid_image)?;
    let format = if encoded.starts_with(PNG_SIGNATURE) {
        ImageFormat::Png
    } else {
        ImageFormat::Jpeg
    };
    let raster = decode(&encoded, format)?;
    let (right, bottom) = checked_rectangle(rect, raster.width(), raster.height())?;
    let [red, green, blue] = color;
    // Keep 16-bit source samples at their original precision. Other supported
    // input formats expand to RGBA8 without changing existing RGBA samples.
    if matches!(
        raster.color(),
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16
    ) {
        let mut pixels = raster.into_rgba16();
        let replacement = Rgba([red, green, blue, 255].map(|sample| u16::from(sample) * 257));
        for y in rect.y..bottom {
            check_deadline(deadline)?;
            for x in rect.x..right {
                pixels.put_pixel(x, y, replacement);
            }
        }
        Ok(DynamicImage::ImageRgba16(pixels))
    } else {
        let mut pixels = raster.into_rgba8();
        let replacement = Rgba([red, green, blue, 255]);
        for y in rect.y..bottom {
            check_deadline(deadline)?;
            for x in rect.x..right {
                pixels.put_pixel(x, y, replacement);
            }
        }
        Ok(DynamicImage::ImageRgba8(pixels))
    }
}

struct PngOutput {
    bytes: Vec<u8>,
    deadline: Instant,
    exceeded: bool,
}

impl io::Write for PngOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if bytes.len() > MAX_PNG_BYTES.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("PNG size limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode(raster: &DynamicImage, deadline: Instant) -> Result<Vec<u8>> {
    check_deadline(deadline)?;
    let mut output = PngOutput {
        bytes: Vec::new(),
        deadline,
        exceeded: false,
    };
    let result =
        PngEncoder::new_with_quality(&mut output, CompressionType::Fast, FilterType::Adaptive)
            .write_image(
                raster.as_bytes(),
                raster.width(),
                raster.height(),
                raster.color().into(),
            );
    check_deadline(deadline)?;
    if output.exceeded {
        return Err(resource_limit());
    }
    result.map_err(|_| invalid_image())?;
    let readback = decode(&output.bytes, ImageFormat::Png)?;
    check_deadline(deadline)?;
    if readback.dimensions() != raster.dimensions()
        || readback.color() != raster.color()
        || readback.as_bytes() != raster.as_bytes()
    {
        return Err(invalid_image());
    }
    Ok(output.bytes)
}

pub(crate) fn render(
    operation: &ImageOperation,
    reference: Option<&RequestAttachment>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    check_deadline(deadline)?;
    let raster = match (operation, reference) {
        (
            ImageOperation::Draw {
                width,
                height,
                background,
                shapes,
            },
            None,
        ) => draw(*width, *height, *background, shapes, deadline)?,
        (ImageOperation::Fill { rectangle, color }, Some(reference)) => {
            edit(reference, rectangle, *color, deadline)?
        }
        _ => {
            return Err(failure(
                "IMAGE_SOURCE_MISMATCH",
                "新規画像の作成と、元画像を一つ指定した編集を区別してください。",
            ));
        }
    };
    encode(&raster, deadline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, codecs::jpeg::JpegEncoder};
    use std::time::Duration;

    fn rectangle(x: u32, y: u32, width: u32, height: u32) -> ImageRectangle {
        ImageRectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn draw_renders_placements_in_order_and_rejects_overflow() {
        let deadline = Instant::now() + Duration::from_secs(3);
        let image = ImageOperation::Draw {
            width: 15,
            height: 12,
            background: [255, 255, 255],
            shapes: vec![
                ImagePlacement {
                    shape: ImageShape::Rectangle,
                    rectangle: rectangle(1, 1, 9, 9),
                    color: [0, 0, 255],
                },
                ImagePlacement {
                    shape: ImageShape::Circle,
                    rectangle: rectangle(5, 2, 6, 6),
                    color: [255, 0, 0],
                },
                ImagePlacement {
                    shape: ImageShape::Triangle,
                    rectangle: rectangle(1, 7, 5, 4),
                    color: [0, 255, 0],
                },
            ],
        };
        let png = render(&image, None, deadline).unwrap();
        assert_eq!(inspect_png(&png).unwrap(), (15, 12));
        assert_eq!(render(&image, None, deadline).unwrap(), png);
        let pixels = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(pixels[(0, 0)].0, [255, 255, 255, 255]);
        assert_eq!(pixels[(5, 2)].0, [0, 0, 255, 255]);
        assert_eq!(pixels[(8, 4)].0, [255, 0, 0, 255]);
        assert_eq!(pixels[(3, 7)].0, [0, 255, 0, 255]);
        assert_eq!(pixels[(1, 7)].0, [0, 0, 255, 255]);
        assert!(inspect_png(&png[..png.len() - 12]).is_err());
        let ImageOperation::Draw { mut shapes, .. } = image else {
            unreachable!();
        };
        shapes[0].rectangle.x = u32::MAX;
        assert!(draw(15, 12, [255; 3], &shapes, deadline).is_err());
    }

    #[test]
    fn png_edit_preserves_every_untouched_16_bit_sample() {
        let deadline = Instant::now() + Duration::from_secs(3);
        let original = ImageBuffer::from_fn(4, 3, |x, y| {
            Rgba([x as u16 * 111, y as u16 * 321, 4093, 777])
        });
        let source = encode(&DynamicImage::ImageRgba16(original.clone()), deadline).unwrap();
        let attachment = RequestAttachment::binary_snapshot("reference.png", &source);
        let fill = ImageOperation::Fill {
            rectangle: rectangle(1, 1, 2, 1),
            color: [17, 23, 103],
        };
        let png = render(&fill, Some(&attachment), deadline).unwrap();
        let edited = image::load_from_memory(&png).unwrap();
        assert_eq!(edited.color(), ColorType::Rgba16);
        let edited = edited.into_rgba16();
        for y in 0..3 {
            for x in 0..4 {
                let expected = if y == 1 && (1..3).contains(&x) {
                    [4369, 5911, 26471, 65535]
                } else {
                    original[(x, y)].0
                };
                assert_eq!(edited[(x, y)].0, expected);
            }
        }
    }

    #[test]
    fn jpeg_edit_uses_the_decoded_pixels_and_the_sealed_format() {
        let deadline = Instant::now() + Duration::from_secs(3);
        let original = image::RgbImage::from_fn(3, 2, |x, y| Rgb([90, x as u8, y as u8]));
        let mut source = Vec::new();
        JpegEncoder::new(&mut source)
            .encode_image(&original)
            .unwrap();
        let attachment = RequestAttachment::binary_snapshot("reference.JPG", &source);
        let fill = ImageOperation::Fill {
            rectangle: rectangle(2, 1, 1, 1),
            color: [10, 20, 30],
        };
        let png = render(&fill, Some(&attachment), deadline).unwrap();
        let edited = image::load_from_memory(&png).unwrap().into_rgba8();
        let original = image::load_from_memory(&source).unwrap().into_rgba8();
        assert_eq!(edited[(2, 1)].0, [10, 20, 30, 255]);
        assert_eq!(edited[(1, 1)], original[(1, 1)]);
        let wrong = RequestAttachment::binary_snapshot("reference.png", &source);
        assert!(render(&fill, Some(&wrong), deadline).is_err());
    }
}
