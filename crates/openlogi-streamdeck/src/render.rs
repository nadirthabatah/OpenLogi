//! Turning a picture into the bytes a key screen accepts.
//!
//! The counterpart of [`crate::image`], which frames an already-encoded image
//! into packets: this produces the encoding. Kept behind the `render` feature
//! so the rest of the crate stays dependency-light and portable — the wasm
//! job checks it with default features, and an image codec has no business in
//! that build.
//!
//! Two model facts decide the output, and both come from the catalogue rather
//! than from assumption: the key screen's pixel size, and the rotation its
//! panel is mounted at. Get the rotation wrong and every key is upside down,
//! which is the most likely visible symptom of a wrong catalogue entry.

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, ImageEncoder as _, Rgb, RgbImage};

use crate::ProtocolError;
use crate::model::{ImageFormat as KeyFormat, ImageRotation, Model};

/// JPEG quality for key images.
///
/// High enough that flat colour and text edges stay clean, low enough that a
/// key image is a few kilobytes — which matters, because every image is
/// chopped into 1016-byte packets and written one at a time.
const QUALITY: u8 = 90;

/// Encode `picture` for `model`'s key screens.
///
/// Scales to the model's key size, applies its panel rotation, and encodes in
/// the format it accepts. The result is what [`crate::image::key_image_packets`]
/// expects.
///
/// # Errors
///
/// - [`ProtocolError::ScreenlessModel`] if the model has no key screens.
/// - [`ProtocolError::ImageFramingUnsupported`] for a screen format this
///   function does not encode yet (gen 1's bitmaps).
/// - [`ProtocolError::ImageEncoding`] if the encoder itself fails.
pub fn key_image(model: &Model, picture: &DynamicImage) -> Result<Vec<u8>, ProtocolError> {
    let screens = model
        .screens
        .ok_or(ProtocolError::ScreenlessModel { model: model.name })?;
    if screens.format != KeyFormat::Jpeg {
        return Err(ProtocolError::ImageFramingUnsupported { model: model.name });
    }

    let size = u32::from(screens.size_px);
    // Triangle filtering: key screens are small and images are usually
    // downscaled a long way onto them, where nearest-neighbour aliases badly
    // and a slower filter buys nothing anyone can see at 72 pixels.
    let scaled = picture.resize_exact(size, size, image::imageops::FilterType::Triangle);
    let oriented = match screens.rotation {
        ImageRotation::None => scaled,
        ImageRotation::Quarter => scaled.rotate90(),
        ImageRotation::Half => scaled.rotate180(),
    };

    let rgb = oriented.to_rgb8();
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, QUALITY)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|error| ProtocolError::ImageEncoding {
            detail: error.to_string(),
        })?;
    Ok(encoded)
}

/// A key filled with one colour, ready to encode.
///
/// Separate from [`key_image`] so a caller that just wants a coloured key does
/// not have to construct an image to get one — and so the size is taken from
/// the model rather than guessed.
///
/// # Errors
///
/// [`ProtocolError::ScreenlessModel`] if the model has no key screens.
pub fn solid(model: &Model, red: u8, green: u8, blue: u8) -> Result<DynamicImage, ProtocolError> {
    let screens = model
        .screens
        .ok_or(ProtocolError::ScreenlessModel { model: model.name })?;
    let size = u32::from(screens.size_px);
    Ok(DynamicImage::ImageRgb8(RgbImage::from_pixel(
        size,
        size,
        Rgb([red, green, blue]),
    )))
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, GenericImageView as _, ImageEncoder as _, Rgb, RgbImage};

    use super::{key_image, solid};
    use crate::ProtocolError;
    use crate::model::{ELGATO_VENDOR_ID, identify};

    fn model(product_id: u16) -> &'static crate::model::Model {
        identify(ELGATO_VENDOR_ID, product_id).expect("catalogued")
    }

    #[test]
    fn a_solid_key_is_the_models_own_size() {
        // The MK.2 is 72px, the XL 96px, the Plus 120px: the size must come
        // from the catalogue, not from a constant.
        for (product_id, expected) in [(0x0080, 72), (0x006c, 96), (0x0084, 120)] {
            let filled = solid(model(product_id), 1, 2, 3).expect("has screens");
            assert_eq!(filled.dimensions(), (expected, expected));
        }
    }

    #[test]
    fn a_screenless_model_cannot_be_filled_or_encoded() {
        let pedal = model(0x0086);
        assert!(matches!(
            solid(pedal, 0, 0, 0).expect_err("the Pedal has no screens"),
            ProtocolError::ScreenlessModel { .. }
        ));
        let any = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([0, 0, 0])));
        assert!(matches!(
            key_image(pedal, &any).expect_err("the Pedal has no screens"),
            ProtocolError::ScreenlessModel { .. }
        ));
    }

    #[test]
    fn gen1_bitmaps_are_refused_rather_than_encoded_as_jpeg() {
        let original = model(0x0060);
        let any = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([0, 0, 0])));
        assert!(matches!(
            key_image(original, &any).expect_err("gen 1 wants a bitmap"),
            ProtocolError::ImageFramingUnsupported { .. }
        ));
    }

    #[test]
    fn an_encoded_key_image_is_a_jpeg_of_the_models_size() {
        let mk2 = model(0x0080);
        let filled = solid(mk2, 200, 40, 40).expect("has screens");
        let encoded = key_image(mk2, &filled).expect("encodes");

        // JPEG's own magic, so this asserts the format rather than trusting
        // the call that asked for it.
        assert_eq!(
            &encoded[..2],
            &[0xff, 0xd8],
            "starts with a JPEG SOI marker"
        );
        assert_eq!(
            &encoded[encoded.len() - 2..],
            &[0xff, 0xd9],
            "ends with EOI"
        );

        let decoded = image::load_from_memory(&encoded).expect("re-reads as an image");
        assert_eq!(decoded.dimensions(), (72, 72));
    }

    #[test]
    fn an_oversized_picture_is_scaled_down_to_the_key() {
        let mk2 = model(0x0080);
        let big = DynamicImage::ImageRgb8(RgbImage::from_pixel(1000, 400, Rgb([10, 20, 30])));
        let encoded = key_image(mk2, &big).expect("encodes");
        let decoded = image::load_from_memory(&encoded).expect("re-reads");
        assert_eq!(decoded.dimensions(), (72, 72));
    }

    /// A half turn is the difference between a legible key and an upside-down
    /// one, so it is asserted rather than assumed: a picture with a distinct
    /// top-left corner must come back with that corner at bottom-right.
    #[test]
    fn the_models_rotation_is_actually_applied() {
        let mk2 = model(0x0080); // rotation: Half
        assert_eq!(
            mk2.screens.expect("has screens").rotation,
            crate::model::ImageRotation::Half
        );

        let mut marked = RgbImage::from_pixel(72, 72, Rgb([0, 0, 0]));
        // A bright block in the top-left quarter.
        for y in 0..20 {
            for x in 0..20 {
                marked.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let encoded = key_image(mk2, &DynamicImage::ImageRgb8(marked)).expect("encodes");
        let decoded = image::load_from_memory(&encoded)
            .expect("re-reads")
            .to_rgb8();

        let corner = |x: u32, y: u32| u32::from(decoded.get_pixel(x, y).0[0]);
        assert!(
            corner(61, 61) > 200,
            "the marked corner must land at bottom-right after a half turn"
        );
        assert!(
            corner(10, 10) < 60,
            "and the original top-left must now be dark"
        );
    }

    /// The quality setting must actually reach the encoder. It did not once:
    /// the constant was defined and then ignored, which is invisible except
    /// as a file size.
    #[test]
    fn the_quality_setting_reaches_the_encoder() {
        // A photographic gradient, so quality has something to act on — a
        // flat fill compresses to nearly the same size at any setting.
        let mut noisy = RgbImage::new(72, 72);
        for (x, y, pixel) in noisy.enumerate_pixels_mut() {
            let v = u8::try_from((x * 7 + y * 13) % 256).unwrap_or(0);
            *pixel = Rgb([v, v.wrapping_mul(3), v.wrapping_add(90)]);
        }
        let picture = DynamicImage::ImageRgb8(noisy);
        let at_configured = key_image(model(0x0080), &picture).expect("encodes");

        let mut low = Vec::new();
        let rgb = picture.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut low, 10)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .expect("encodes");

        assert!(
            at_configured.len() > low.len(),
            "quality {} produced {} bytes, no larger than quality 10's {} — \
             the setting is not reaching the encoder",
            super::QUALITY,
            at_configured.len(),
            low.len()
        );
    }

    #[test]
    fn an_encoded_key_image_frames_into_packets() {
        // The two halves of the image path meet here: what render produces
        // must be what the framer accepts.
        let mk2 = model(0x0080);
        let filled = solid(mk2, 0, 128, 255).expect("has screens");
        let encoded = key_image(mk2, &filled).expect("encodes");
        let packets = crate::image::key_image_packets(mk2, 0, &encoded).expect("frames");
        assert!(!packets.is_empty());
        assert_eq!(
            packets.last().expect("at least one packet")[3],
            1,
            "the final packet is flagged last"
        );
    }
}
