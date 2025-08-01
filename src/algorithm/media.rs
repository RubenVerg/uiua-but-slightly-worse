//! En/decode Uiua arrays to/from media formats

use std::{collections::BTreeMap, f64::consts::E, mem::take};

use ecow::{eco_vec, EcoString, EcoVec};
use enum_iterator::{all, Sequence};
#[cfg(feature = "audio_encode")]
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
#[cfg(feature = "image")]
use image::{DynamicImage, ImageFormat};
use serde::*;

#[allow(unused_imports)]
use crate::{Array, Uiua, UiuaResult, Value};
use crate::{Complex, FieldInfo, Shape, Signature, SysBackend};

use super::monadic::hsv_to_rgb;

/// Conversion of a value to some media format based on the value's shape
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum SmartOutput {
    Normal(String),
    Png(Vec<u8>, Option<String>),
    Gif(Vec<u8>, Option<String>),
    Apng(Vec<u8>, Option<String>),
    Wav(Vec<u8>, Option<String>),
    Svg { svg: String, original: Value },
}

const MIN_AUTO_IMAGE_DIM: usize = 30;

impl SmartOutput {
    /// Convert a value to a SmartOutput
    ///
    /// Animations default to GIF
    pub fn from_value(value: Value, frame_rate: f64, backend: &dyn SysBackend) -> Self {
        Self::from_value_impl(value, frame_rate, false, backend)
    }
    /// Convert a value to a SmartOutput
    ///
    /// Animations default to APNG
    pub fn from_value_prefer_apng(value: Value, frame_rate: f64, backend: &dyn SysBackend) -> Self {
        Self::from_value_impl(value, frame_rate, true, backend)
    }
    fn from_value_impl(
        value: Value,
        frame_rate: f64,
        prefer_apng: bool,
        backend: &dyn SysBackend,
    ) -> Self {
        // Try to convert the value to audio
        #[cfg(feature = "audio_encode")]
        if value.row_count() >= 44100 / 4
            && matches!(&value, Value::Num(arr) if arr.elements().all(|x| x.abs() <= 5.0))
        {
            if let Ok(bytes) = value_to_wav_bytes(&value, backend.audio_sample_rate()) {
                let label = value.meta.label.as_ref().map(Into::into);
                return Self::Wav(bytes, label);
            }
        }
        // Try to convert the value to an image
        #[cfg(feature = "image")]
        if let Ok(image) = value_to_image(&value) {
            if image.width() >= MIN_AUTO_IMAGE_DIM as u32
                && image.height() >= MIN_AUTO_IMAGE_DIM as u32
            {
                if let Ok(bytes) = image_to_bytes(&image, ImageFormat::Png) {
                    let label = value.meta.label.as_ref().map(Into::into);
                    return Self::Png(bytes, label);
                }
            }
        }
        // Try to convert the value to a gif or apng
        let animation = if prefer_apng {
            Self::try_apng(&value, frame_rate).or_else(|| Self::try_gif(&value, frame_rate))
        } else {
            Self::try_gif(&value, frame_rate).or_else(|| Self::try_apng(&value, frame_rate))
        };
        if let Some(anim) = animation {
            return anim;
        }
        // Try to convert the value to an svg
        if let Some(str) = value.as_string_opt() {
            let mut str = str.trim().to_string();
            if str.starts_with("<svg") && str.ends_with("</svg>") {
                if !str.contains("xmlns") {
                    str = str.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1);
                }
                return Self::Svg {
                    svg: str,
                    original: value,
                };
            }
        }
        // Otherwise, just show the value
        Self::Normal(value.show())
    }
    #[cfg(not(feature = "gif"))]
    fn try_gif(value: &Value, frame_rate: f64) -> Option<Self> {
        None
    }
    #[cfg(feature = "gif")]
    fn try_gif(value: &Value, frame_rate: f64) -> Option<Self> {
        let bytes = value_to_gif_bytes(value, frame_rate).ok()?;
        match &*value.shape {
            &[f, h, w] | &[f, h, w, _]
                if h >= MIN_AUTO_IMAGE_DIM && w >= MIN_AUTO_IMAGE_DIM && f >= 5 =>
            {
                let label = value.meta.label.as_ref().map(Into::into);
                Some(Self::Gif(bytes, label))
            }
            _ => None,
        }
    }
    #[cfg(not(feature = "apng"))]
    fn try_apng(value: &Value, frame_rate: f64) -> Option<Self> {
        None
    }
    #[cfg(feature = "apng")]
    fn try_apng(value: &Value, frame_rate: f64) -> Option<Self> {
        let bytes = value_to_apng_bytes(value, frame_rate).ok()?;
        match &*value.shape {
            &[f, h, w] | &[f, h, w, _]
                if h >= MIN_AUTO_IMAGE_DIM && w >= MIN_AUTO_IMAGE_DIM && f >= 5 =>
            {
                let label = value.meta.label.as_ref().map(Into::into);
                Some(Self::Apng(bytes.into_iter().collect(), label))
            }
            _ => None,
        }
    }
}

pub(crate) fn image_encode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "image")]
    {
        let format = env
            .pop(1)?
            .as_string(env, "Image format must be a string")?;
        let value = env.pop(2)?;
        let output_format = match format.as_str() {
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "png" => ImageFormat::Png,
            "bmp" => ImageFormat::Bmp,
            "gif" => ImageFormat::Gif,
            "ico" => ImageFormat::Ico,
            "qoi" => ImageFormat::Qoi,
            "webp" => ImageFormat::WebP,
            format => return Err(env.error(format!("Invalid image format: {format}"))),
        };
        let bytes =
            crate::media::value_to_image_bytes(&value, output_format).map_err(|e| env.error(e))?;
        env.push(Array::<u8>::from(bytes.as_slice()));
        Ok(())
    }
    #[cfg(not(feature = "image"))]
    Err(env.error("Image encoding is not supported in this environment"))
}

pub(crate) fn image_decode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "image")]
    {
        let bytes: crate::cowslice::CowSlice<u8> = match env.pop(1)? {
            Value::Byte(arr) => {
                if arr.rank() != 1 {
                    return Err(env.error(format!(
                        "Image bytes array must be rank 1, but is rank {}",
                        arr.rank()
                    )));
                }
                arr.data
            }
            Value::Num(arr) => {
                if arr.rank() != 1 {
                    return Err(env.error(format!(
                        "Image bytes array must be rank 1, but is rank {}",
                        arr.rank()
                    )));
                }
                arr.data.iter().map(|&x| x as u8).collect()
            }
            _ => return Err(env.error("Image bytes must be a numeric array")),
        };
        let format = image::guess_format(&bytes)
            .map_err(|e| env.error(format!("Failed to read image: {e}")))?;
        let array =
            crate::media::image_bytes_to_array(&bytes, false, true).map_err(|e| env.error(e))?;
        env.push(array);
        env.push(match format {
            image::ImageFormat::Jpeg => "jpeg".into(),
            fmt => format!("{fmt:?}").to_lowercase(),
        });
        Ok(())
    }
    #[cfg(not(feature = "image"))]
    Err(env.error("Image decoding is not supported in this environment"))
}

pub(crate) fn gif_encode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "gif")]
    {
        let frame_rate = env.pop(1)?.as_num(env, "Framerate must be a number")?;
        let value = env.pop(2)?;
        let bytes =
            crate::media::value_to_gif_bytes(&value, frame_rate).map_err(|e| env.error(e))?;
        env.push(Array::<u8>::from(bytes.as_slice()));
        Ok(())
    }
    #[cfg(not(feature = "gif"))]
    Err(env.error("GIF encoding is not supported in this environment"))
}

pub(crate) fn gif_decode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "gif")]
    {
        let bytes = env
            .pop(1)?
            .as_bytes(env, "Gif bytes must be a byte array")?;
        let (frame_rate, value) =
            crate::media::gif_bytes_to_value(&bytes).map_err(|e| env.error(e))?;
        env.push(value);
        env.push(frame_rate);
        Ok(())
    }
    #[cfg(not(feature = "gif"))]
    Err(env.error("GIF encoding is not supported in this environment"))
}

pub(crate) fn apng_encode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "apng")]
    {
        let frame_rate = env.pop(1)?.as_num(env, "Framerate must be a number")?;
        let value = env.pop(2)?;
        let bytes =
            crate::media::value_to_apng_bytes(&value, frame_rate).map_err(|e| env.error(e))?;
        env.push(Array::<u8>::from(bytes));
        Ok(())
    }
    #[cfg(not(feature = "apng"))]
    Err(env.error("APNG encoding is not supported in this environment"))
}

pub(crate) fn audio_encode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "audio_encode")]
    {
        let format = env
            .pop(1)?
            .as_string(env, "Audio format must be a string")?;

        const SAMPLE_RATE_REQUIREMENT: &str = "Audio sample rate must be a positive integer";
        let sample_rate = u32::try_from(env.pop(2)?.as_nat(env, SAMPLE_RATE_REQUIREMENT)?)
            .map_err(|_| env.error("Audio sample rate is too high"))?;
        if sample_rate == 0 {
            return Err(env.error(format!("{SAMPLE_RATE_REQUIREMENT}, but it is zero")));
        }

        let value = env.pop(3)?;
        let bytes = match format.as_str() {
            "wav" => {
                crate::media::value_to_wav_bytes(&value, sample_rate).map_err(|e| env.error(e))?
            }
            format => {
                return Err(env.error(format!("Invalid or unsupported audio format: {format}")))
            }
        };
        env.push(Array::<u8>::from(bytes.as_slice()));
        Ok(())
    }
    #[cfg(not(feature = "audio_encode"))]
    Err(env.error("Audio encoding is not supported in this environment"))
}

pub(crate) fn audio_decode(env: &mut Uiua) -> UiuaResult {
    #[cfg(feature = "audio_encode")]
    {
        let bytes: crate::cowslice::CowSlice<u8> = match env.pop(1)? {
            Value::Byte(arr) => {
                if arr.rank() != 1 {
                    return Err(env.error(format!(
                        "Audio bytes array must be rank 1, but is rank {}",
                        arr.rank()
                    )));
                }
                arr.data
            }
            Value::Num(arr) => {
                if arr.rank() != 1 {
                    return Err(env.error(format!(
                        "Audio bytes array must be rank 1, but is rank {}",
                        arr.rank()
                    )));
                }
                arr.data.iter().map(|&x| x as u8).collect()
            }
            _ => return Err(env.error("Audio bytes must be a numeric array")),
        };
        let (array, sample_rate) =
            crate::media::array_from_wav_bytes(&bytes).map_err(|e| env.error(e))?;
        env.push(array);
        env.push(sample_rate as usize);
        env.push("wav");
        Ok(())
    }
    #[cfg(not(feature = "audio_encode"))]
    Err(env.error("Audio decoding is not supported in this environment"))
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn value_to_image_bytes(value: &Value, format: ImageFormat) -> Result<Vec<u8>, String> {
    image_to_bytes(&value_to_image(value)?, format)
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn rgb_image_to_array(image: image::RgbImage) -> Array<f64> {
    let shape = crate::Shape::from([image.height() as usize, image.width() as usize, 3]);
    Array::new(
        shape,
        (image.into_raw().into_iter())
            .map(|b| b as f64 / 255.0)
            .collect::<crate::cowslice::CowSlice<_>>(),
    )
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn rgba_image_to_array(image: image::RgbaImage) -> Array<f64> {
    let shape = crate::Shape::from([image.height() as usize, image.width() as usize, 4]);
    Array::new(
        shape,
        (image.into_raw().into_iter())
            .map(|b| b as f64 / 255.0)
            .collect::<crate::cowslice::CowSlice<_>>(),
    )
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn image_bytes_to_array(bytes: &[u8], gray: bool, alpha: bool) -> Result<Array<f64>, String> {
    let image = image::load_from_memory(bytes).map_err(|e| format!("Failed to read image: {e}"))?;
    Ok(match (gray, alpha) {
        (false, false) => rgb_image_to_array(image.into_rgb8()),
        (false, true) => {
            let image = image.into_rgba8();
            let shape = crate::Shape::from([image.height() as usize, image.width() as usize, 4]);
            Array::new(
                shape,
                (image.into_raw().into_iter())
                    .map(|b| b as f64 / 255.0)
                    .collect::<crate::cowslice::CowSlice<_>>(),
            )
        }
        (true, false) => {
            let image = image.into_luma16();
            let shape = crate::Shape::from([image.height() as usize, image.width() as usize]);
            Array::new(
                shape,
                (image.into_raw().into_iter())
                    .map(|l| l as f64 / u16::MAX as f64)
                    .collect::<crate::cowslice::CowSlice<_>>(),
            )
        }
        (true, true) => {
            let image = image.into_luma_alpha16();
            let shape = crate::Shape::from([image.height() as usize, image.width() as usize, 2]);
            Array::new(
                shape,
                (image.into_raw().into_iter())
                    .map(|l| l as f64 / u16::MAX as f64)
                    .collect::<crate::cowslice::CowSlice<_>>(),
            )
        }
    })
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn image_to_bytes(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, format)
        .map_err(|e| format!("Failed to write image: {e}"))?;
    Ok(bytes.into_inner())
}

#[doc(hidden)]
#[cfg(feature = "image")]
pub fn value_to_image(value: &Value) -> Result<DynamicImage, String> {
    let is_complex = matches!(value, Value::Complex(_));
    if !is_complex && ![2, 3].contains(&value.rank()) || is_complex && value.rank() != 2 {
        return Err(format!(
            "Image must be a rank 2 or 3 numeric array, but it is a rank-{} {} array",
            value.rank(),
            value.type_name()
        ));
    }
    let bytes = match value {
        Value::Num(nums) => nums.data.iter().map(|f| (*f * 255.0) as u8).collect(),
        Value::Byte(bytes) => bytes.data.iter().map(|&b| (b > 0) as u8 * 255).collect(),
        Value::Complex(comp) => (comp.data.iter())
            .flat_map(|&c| complex_color(c).map(|c| (c * 255.0) as u8))
            .collect(),
        _ => return Err("Image must be a numeric or complex array".into()),
    };
    #[allow(clippy::match_ref_pats)]
    let [height, width, px_size] = match &*value.shape {
        &[a, b] if is_complex => [a, b, 3],
        &[a, b] if !is_complex => [a, b, 1],
        &[a, b, c] => [a, b, c],
        _ => unreachable!("Shape checked above"),
    };
    Ok(match px_size {
        1 => image::GrayImage::from_raw(width as u32, height as u32, bytes)
            .ok_or("Failed to create image")?
            .into(),
        2 => image::GrayAlphaImage::from_raw(width as u32, height as u32, bytes)
            .ok_or("Failed to create image")?
            .into(),
        3 => image::RgbImage::from_raw(width as u32, height as u32, bytes)
            .ok_or("Failed to create image")?
            .into(),
        4 => image::RgbaImage::from_raw(width as u32, height as u32, bytes)
            .ok_or("Failed to create image")?
            .into(),
        n => {
            return Err(format!(
                "For a color image, the last dimension of the image array must be between 1 and 4 but it is {n}"
            ))
        }
    })
}

fn complex_color(c: Complex) -> [f64; 3] {
    let h = c.arg();
    let mag = c.abs();
    let s = 0.3 + 0.7 * (-mag / 10.0).exp();
    let v = 1.0 - (-E * mag).exp();
    hsv_to_rgb(h, s, v)
}

#[doc(hidden)]
pub fn value_to_audio_channels(audio: &Value) -> Result<Vec<Vec<f64>>, String> {
    let orig = audio;
    let mut audio = audio;
    let mut transposed;
    if audio.rank() == 2 && audio.shape[1] > 5 {
        transposed = audio.clone();
        transposed.transpose();
        audio = &transposed;
    }
    let interleaved: Vec<f64> = match audio {
        Value::Num(nums) => nums.data.iter().copied().collect(),
        Value::Byte(byte) => byte.data.iter().map(|&b| b as f64).collect(),
        _ => return Err("Audio must be a numeric array".into()),
    };
    let (length, mut channels) = match &*audio.shape {
        [_] => (interleaved.len(), vec![interleaved]),
        &[len, ch] => (
            len,
            (0..ch)
                .map(|c| (0..len).map(|i| interleaved[i * ch + c]).collect())
                .collect(),
        ),
        _ => {
            return Err(format!(
                "Audio must be a rank 1 or 2 numeric array, but it is rank {}",
                orig.rank()
            ))
        }
    };
    if channels.len() > 5 {
        return Err(format!(
            "Audio can have at most 5 channels, but its shape is {}",
            orig.shape
        ));
    }

    if channels.is_empty() {
        channels.push(vec![0.0; length]);
    }
    Ok(channels)
}

#[doc(hidden)]
#[cfg(feature = "audio_encode")]
pub fn value_to_wav_bytes(audio: &Value, sample_rate: u32) -> Result<Vec<u8>, String> {
    if sample_rate == 0 {
        return Err("Sample rate must not be 0".to_string());
    }
    let channels = value_to_audio_channels(audio)?;
    #[cfg(not(feature = "audio"))]
    {
        channels_to_wav_bytes_impl(
            channels,
            |f| (f * i16::MAX as f64) as i16,
            16,
            SampleFormat::Int,
            sample_rate,
        )
    }
    #[cfg(feature = "audio")]
    {
        channels_to_wav_bytes_impl(channels, |f| f as f32, 32, SampleFormat::Float, sample_rate)
    }
}

#[cfg(feature = "audio_encode")]
fn channels_to_wav_bytes_impl<T: hound::Sample + Copy>(
    channels: Vec<Vec<f64>>,
    convert_samples: impl Fn(f64) -> T + Copy,
    bits_per_sample: u16,
    sample_format: SampleFormat,
    sample_rate: u32,
) -> Result<Vec<u8>, String> {
    // We use i16 samples for compatibility with Firefox (if I remember correctly)
    let channels: Vec<Vec<T>> = channels
        .into_iter()
        .map(|c| c.into_iter().map(convert_samples).collect())
        .collect();
    let spec = WavSpec {
        channels: channels.len() as u16,
        sample_rate,
        bits_per_sample,
        sample_format,
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut bytes, spec).map_err(|e| e.to_string())?;
    for i in 0..channels[0].len() {
        for channel in &channels {
            writer
                .write_sample(channel[i])
                .map_err(|e| format!("Failed to write audio: {e}"))?;
        }
    }
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize audio: {e}"))?;
    Ok(bytes.into_inner())
}

#[doc(hidden)]
#[cfg(feature = "audio_encode")]
pub fn stereo_to_wave_bytes<T: hound::Sample + Copy>(
    samples: &[[f64; 2]],
    convert_samples: impl Fn(f64) -> T + Copy,
    bits_per_sample: u16,
    sample_format: SampleFormat,
    sample_rate: u32,
) -> Result<Vec<u8>, String> {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample,
        sample_format,
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut writer = WavWriter::new(&mut bytes, spec).map_err(|e| e.to_string())?;
    for frame in samples {
        for sample in frame {
            writer
                .write_sample(convert_samples(*sample))
                .map_err(|e| format!("Failed to write audio: {e}"))?;
        }
    }
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize audio: {e}"))?;
    Ok(bytes.into_inner())
}

#[cfg(feature = "audio_encode")]
#[doc(hidden)]
pub fn array_from_wav_bytes(bytes: &[u8]) -> Result<(Array<f64>, u32), String> {
    let mut reader: WavReader<std::io::Cursor<&[u8]>> =
        WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => {
            array_from_wav_bytes_impl::<i16>(&mut reader, |i| i as f64 / i16::MAX as f64)
        }
        (SampleFormat::Int, 32) => {
            array_from_wav_bytes_impl::<i32>(&mut reader, |i| i as f64 / i32::MAX as f64)
        }
        (SampleFormat::Float, 32) => array_from_wav_bytes_impl::<f32>(&mut reader, |f| f as f64),
        (sample_format, bits_per_sample) => Err(format!(
            "Unsupported sample format: {sample_format:?} {bits_per_sample} bits per sample"
        )),
    }
}

#[cfg(feature = "audio_encode")]
fn array_from_wav_bytes_impl<T: hound::Sample>(
    reader: &mut WavReader<std::io::Cursor<&[u8]>>,
    sample_to_f64: impl Fn(T) -> f64,
) -> Result<(Array<f64>, u32), String> {
    use ecow::EcoVec;

    let channel_count = reader.spec().channels as usize;
    let mut samples = EcoVec::new();
    let mut curr_channel = 0;
    for sample in reader.samples::<T>() {
        let sample = sample.map_err(|e| e.to_string())?;
        samples.push(sample_to_f64(sample));
        curr_channel = (curr_channel + 1) % channel_count;
    }

    let sample_rate = reader.spec().sample_rate;
    let arr = if channel_count == 1 {
        samples.into()
    } else {
        Array::new([samples.len() / channel_count, channel_count], samples)
    };
    Ok((arr, sample_rate))
}

#[doc(hidden)]
#[cfg(feature = "gif")]
pub fn value_to_gif_bytes(value: &Value, frame_rate: f64) -> Result<Vec<u8>, String> {
    use std::collections::{HashMap, HashSet};

    use color_quant::NeuQuant;
    use gif::{DisposalMethod, Frame};
    use image::Rgba;

    if value.row_count() == 0 {
        return Err("Cannot convert empty array into GIF".into());
    }
    let mut frames = Vec::with_capacity(value.row_count());
    let mut width = 0;
    let mut height = 0;
    for row in value.rows() {
        let image = value_to_image(&row)?.into_rgba8();
        width = image.width();
        height = image.height();
        frames.push(image);
    }
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(format!(
            "GIF dimensions must be at most {}x{}, but the frames are {}x{}",
            u16::MAX,
            u16::MAX,
            width,
            height
        ));
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut opaque_colors = HashSet::new();
    for frame in &frames {
        for &Rgba([r, g, b, a]) in frame.pixels() {
            if a > 0 {
                opaque_colors.insert([r, g, b]);
            }
        }
    }
    let mut opaque_colors = opaque_colors.into_iter().collect::<Vec<_>>();
    opaque_colors.sort_unstable();
    let mut reduced_colors = HashSet::new();
    let mut color_reduction = HashMap::new();
    if opaque_colors.len() <= 255 {
        for color in opaque_colors {
            reduced_colors.insert(color);
            color_reduction.insert(color, color);
        }
    } else {
        let opaque_data: Vec<u8> = opaque_colors
            .iter()
            .flat_map(|c| c.iter().copied().chain([128]))
            .collect();
        let nq = NeuQuant::new(10, 255, &opaque_data);
        let map = nq.color_map_rgb();
        for color in opaque_colors {
            let index = nq.index_of(&[color[0], color[1], color[2], 128]);
            let start = index * 3;
            let reduced = [map[start], map[start + 1], map[start + 2]];
            reduced_colors.insert(reduced);
            color_reduction.insert(color, reduced);
        }
    }
    let mut reduced_colors = reduced_colors.into_iter().collect::<Vec<_>>();
    reduced_colors.sort_unstable();
    let mut palette = Vec::with_capacity(reduced_colors.len() * 3);
    let mut color_map: HashMap<[u8; 3], u8> = HashMap::new();
    for color in reduced_colors {
        color_map.insert(color, (palette.len() / 3) as u8);
        palette.extend(color);
    }
    let transparent_index = color_map.len() as u8;
    palette.extend([0; 3]);
    let mut encoder = gif::Encoder::new(&mut bytes, width as u16, height as u16, &palette)
        .map_err(|e| e.to_string())?;
    const MIN_FRAME_RATE: f64 = 1.0 / 60.0;
    let delay = ((1.0 / frame_rate.max(MIN_FRAME_RATE)).abs() * 100.0) as u16;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    for image in frames {
        let mut has_transparent = false;
        let indices: Vec<u8> = image
            .as_raw()
            .chunks_exact(4)
            .map(|chunk| {
                if chunk[3] == 0 {
                    has_transparent = true;
                    return transparent_index;
                }
                let color = [chunk[0], chunk[1], chunk[2]];
                let reduced = color_reduction[&color];
                color_map[&reduced]
            })
            .collect();
        let dispose = if has_transparent {
            DisposalMethod::Previous
        } else {
            DisposalMethod::Any
        };
        let mut frame = Frame {
            dispose,
            ..Frame::from_indexed_pixels(
                width as u16,
                height as u16,
                indices,
                Some(transparent_index),
            )
        };
        frame.delay = delay;
        encoder.write_frame(&frame).map_err(|e| e.to_string())?;
    }
    drop(encoder);
    Ok(bytes.into_inner())
}

#[doc(hidden)]
#[cfg(feature = "gif")]
pub fn gif_bytes_to_value(bytes: &[u8]) -> Result<(f64, Value), gif::DecodingError> {
    let mut decoder = gif::DecodeOptions::new();
    decoder.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = decoder.read_info(bytes)?;
    let first_frame = decoder.read_next_frame()?.unwrap();
    let gif_width = first_frame.width as usize;
    let gif_height = first_frame.height as usize;
    let mut data: crate::cowslice::CowSlice<f64> = Default::default();
    let mut frame_count = 1;
    let mut delay_sum = first_frame.delay as f64 / 100.0;
    // Init frame data with the first frame
    let mut frame_data = first_frame.buffer.to_vec();
    data.extend(frame_data.iter().map(|b| *b as f64 / 255.0));
    // Loop through the rest of the frames
    while let Some(frame) = decoder.read_next_frame()? {
        let frame_width = frame.width as usize;
        let frame_height = frame.height as usize;
        // Some frames may have different dimensions than the GIF
        if frame_width == gif_width && frame_height == gif_height {
            frame_data.copy_from_slice(&frame.buffer);
        } else {
            // Copy the frame into the correct position in the GIF
            let frame_left = frame.left as usize;
            let frame_top = frame.top as usize;
            for dy in 0..frame_height {
                let y = frame_top + dy;
                for dx in 0..frame_width {
                    let x = frame_left + dx;
                    let outer_i = (y * gif_width + x) * 4;
                    let inner_i = (dy * frame_width + dx) * 4;
                    frame_data[outer_i..][..4].copy_from_slice(&frame.buffer[inner_i..][..4]);
                }
            }
        }
        data.extend(frame_data.iter().map(|b| *b as f64 / 255.0));
        frame_count += 1;
        delay_sum += frame.delay as f64 / 100.0;
    }
    let avg_delay = delay_sum / frame_count as f64;
    let frame_rate = 1.0 / avg_delay;
    let shape = crate::Shape::from_iter([frame_count, gif_height, gif_width, 4]);
    let mut num = Value::Num(Array::new(shape, data));
    num.compress();
    Ok((frame_rate, num))
}

#[doc(hidden)]
#[cfg(feature = "apng")]
pub(crate) fn value_to_apng_bytes(value: &Value, frame_rate: f64) -> Result<EcoVec<u8>, String> {
    use png::{ColorType, Encoder};
    fn err(s: &'static str) -> impl Fn(png::EncodingError) -> String {
        move |e| format!("Error {s}: {e}")
    }

    if value.row_count() == 0 {
        return Err("Cannot convert empty array into APNG".into());
    }
    if value.rank() < 3 {
        return Err("APNG array must be at least rank 3".into());
    }
    let frame_count = value.shape[0] as u32;
    let height = value.shape[1] as u32;
    let width = value.shape[2] as u32;
    let mut buffer = EcoVec::new();
    let mut encoder = Encoder::new(&mut buffer, width, height);
    (encoder.set_animated(frame_count, 0)).map_err(err("marking as animated"))?;
    (encoder.set_frame_delay(1, (frame_rate.round() as u16).max(1)))
        .map_err(err("setting frame delay"))?;
    encoder.set_color(ColorType::Rgba);
    let mut writer = encoder.write_header().map_err(err("writing header"))?;
    for row in value.rows() {
        let image = value_to_image(&row)?.into_rgba8();
        (writer.write_image_data(&image.into_raw())).map_err(err("writing frame"))?;
    }
    writer.finish().map_err(err("finishing encoding"))?;
    Ok(buffer)
}

macro_rules! builtin_params {
    ($name:ident, $(($param:ident, $comment:literal)),* $(,)?) => {
        #[derive(Sequence)]
        pub(crate) enum $name {
            $($param,)*
        }
        impl $name {
            pub fn field_info() -> BTreeMap<EcoString, FieldInfo> {
                (all::<Self>().enumerate())
                    .map(|(index, param)| {
                        let name = match param {
                            $($name::$param => stringify!($param),)*
                        };
                        let comment = match param {
                            $($name::$param => $comment,)*
                        }
                        .into();
                        let init_sig = Some(Signature::new(0, 1));
                        let info = FieldInfo {
                            index,
                            init_sig,
                            comment,
                        };
                        (name.into(), info)
                    })
                    .collect()
            }
        }
    }
}

builtin_params!(
    VoxelsParam,
    (Fog, "Color for depth fog"),
    (Scale, "Number of pixels per voxel"),
    (Camera, "The position of the camera"),
);

pub(crate) fn voxels(val: &Value, env: &mut Uiua) -> UiuaResult<Value> {
    let args = take(&mut env.rt.set_args);
    let converted: Array<f64>;
    if ![3, 4].contains(&val.rank()) {
        return Err(env.error(format!(
            "Voxel array must be rank 3 or 4, but its shape is {}",
            val.shape
        )));
    }
    if val.rank() == 4 && ![2, 3, 4].contains(&val.shape[3]) {
        return Err(env.error(format!(
            "Rank 4 voxel array must have a last \
            dimension of 2, 3, or 4, but its shape is {}",
            val.shape
        )));
    }
    let arr = match val {
        Value::Num(arr) => arr,
        Value::Byte(arr) => {
            converted = arr.convert_ref();
            &converted
        }
        Value::Complex(arr) => {
            let mut shape = arr.shape.clone();
            let data: EcoVec<_> = if shape.last() == Some(&2) {
                shape.pop();
                shape.push(4);
                let mut data = eco_vec![0.0; shape.elements()];
                let slice = data.make_mut();
                for (i, &c) in arr.data.iter().enumerate() {
                    if i % 2 == 0 {
                        let rgb = complex_color(c);
                        for j in 0..3 {
                            slice[i / 2 * 4 + j] = rgb[j];
                        }
                    } else {
                        slice[i / 2 * 4 + 3] = c.abs();
                    }
                }
                data
            } else {
                shape.push(3);
                arr.data.iter().flat_map(|&c| complex_color(c)).collect()
            };
            converted = Array::new(shape, data);
            &converted
        }
        val => {
            return Err(env.error(format!(
                "Voxel array must be numeric, but it is {}",
                val.type_name_plural()
            )))
        }
    };
    let mut pos: Option<[f64; 3]> = None;
    let mut scale = None;
    let mut fog = None;
    for (arg, index) in args {
        match all::<VoxelsParam>().nth(index).unwrap() {
            VoxelsParam::Fog => {
                let nums = arg.as_nums(env, "Fog must be a scalar number or 3 numbers")?;
                match *nums {
                    [gray] if arg.shape.is_empty() => fog = Some([gray; 3]),
                    [r, g, b] => fog = Some([r, g, b]),
                    _ => {
                        return Err(env.error(format!(
                            "Fog must be a scalar or list of 3 numbers, but its shape is {}",
                            arg.shape
                        )))
                    }
                }
            }
            VoxelsParam::Scale => scale = Some(arg.as_num(env, "Scale must be a number")?),
            VoxelsParam::Camera => {
                let nums = arg.as_nums(env, "Camera position must be 3 numbers")?;
                if let [x, y, z] = *nums {
                    pos = Some([x, y, z]);
                } else {
                    return Err(env.error(format!(
                        "Camera position must be 3 numbers, but its shape is {}",
                        arg.shape
                    )));
                }
            }
        }
    }

    let mut pos_arg = pos.unwrap_or([1.0, 1.0, 1.0]);
    let scale = scale.unwrap_or(1.0);

    fn map<A: Copy, B: Copy, C, const N: usize>(
        a: [A; N],
        b: [B; N],
        f: impl Fn(A, B) -> C,
    ) -> [C; N] {
        std::array::from_fn(|i| f(a[i], b[i]))
    }
    fn mul(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        map(a, b, |a, b| a * b)
    }
    fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        map(a, b, |a, b| a + b)
    }
    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        map(a, b, |a, b| a - b)
    }
    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        mul(a, b).iter().sum()
    }
    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn plane_point(normal: [f64; 3], d: f64, point: [f64; 3]) -> [f64; 3] {
        let mag = (dot(normal, point) - d) / dot(normal, normal);
        let offset = normal.map(|n| n * mag);
        sub(point, offset)
    }
    fn mag(v: [f64; 3]) -> f64 {
        dot(v, v).sqrt()
    }
    fn norm(v: [f64; 3]) -> [f64; 3] {
        let mag = mag(v);
        v.map(|x| x / mag)
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Gray,
        GrayA,
        Rgb,
        Rgba,
    }
    let mode = if arr.rank() == 3 {
        Mode::Gray
    } else if arr.shape[3] == 2 {
        Mode::GrayA
    } else if arr.shape[3] == 3 {
        Mode::Rgb
    } else {
        Mode::Rgba
    };
    let vox_size = match mode {
        Mode::Gray => 1,
        Mode::GrayA => 2,
        Mode::Rgb => 3,
        Mode::Rgba => 4,
    };
    let fog_has_hue = fog.is_some_and(|fog| fog.windows(2).any(|w| w[0] != w[1]));
    let pix_size = match mode {
        Mode::Gray if fog_has_hue => 3,
        Mode::GrayA if fog_has_hue => 4,
        Mode::Gray => 1,
        Mode::GrayA => 2,
        Mode::Rgb => 3,
        Mode::Rgba => 4,
    };
    let color_size = (pix_size - 1) / 2 * 2 + 1;

    let max_dim = arr.shape.iter().take(3).copied().max().unwrap_or(0);
    let scene_radius = max_dim as f64 / 2.0;
    let shell_radius = (arr.shape.iter())
        .fold(0.0, |acc, &x| acc + (x as f64).powi(2))
        .sqrt()
        / 2.0;
    let res_dim = (shell_radius * 2.0 * scale).round() as usize;
    let mut res_shape = Shape::from([res_dim; 2]);
    let mut idxs = vec![0; res_shape.elements()];
    let mut depth_buf = vec![f64::INFINITY; res_shape.elements()];
    let mut translucents: Vec<(usize, usize, f64)> = Vec::new();

    let target = [scene_radius; 3];
    if pos_arg == [0.0; 3] {
        pos_arg = [1.1; 3];
    }
    pos_arg = norm(pos_arg);
    let cam_pos = [
        target[0] + shell_radius * pos_arg[0],
        target[1] + shell_radius * pos_arg[1],
        target[2] + shell_radius * pos_arg[2],
    ];
    let shell_dist = mag(sub(target, cam_pos)) - shell_radius;
    let normal = norm(sub(target, cam_pos));
    let d = dot(normal, cam_pos);
    let cam_center = plane_point(normal, d, target);
    let up_hint = if normal[0].abs() < 0.999 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = norm(cross(up_hint, normal));
    let v = cross(normal, u);

    // println!("im radius: {shell_radius:.3}");
    // println!("scene radius: {scene_radius:.3}");
    // println!("shell dist: {shell_dist:.3}");
    // println!("res_dim: {res_dim}");
    // println!("pos: {pos:.3?}");
    // println!("cam_center: {cam_center:.3?}");
    // println!("target: {target:.3?}");
    // println!("normal: {normal:.3?}");
    // println!("u: {u:.3?}, v: {v:.3?}");

    let x_stride = arr.shape[1] * arr.shape[2];
    let y_stride = arr.shape[2];
    let scale_start = 0.5 / scale;
    let scale_step = 1.0 / scale;
    let scale_steps = (scale.round() as usize).max(1);
    let offset = [
        (max_dim - arr.shape[0]) as f64 / 2.0,
        (max_dim - arr.shape[1]) as f64 / 2.0,
        (max_dim - arr.shape[2]) as f64 / 2.0,
    ];
    // Precompute scaling offsets
    let mut voxel_surface_offsets = Vec::with_capacity(scale_steps * scale_steps * 6);
    for i in 0..scale_steps {
        let di = scale_start + i as f64 * scale_step;
        for j in 0..scale_steps {
            let dj = scale_start + j as f64 * scale_step;
            for k in 0..scale_steps {
                if ![i, j, k].iter().any(|&x| x == 0 || x == scale_steps - 1) {
                    continue;
                }
                let dk = scale_start + k as f64 * scale_step;
                let offset = [di, dj, dk];
                voxel_surface_offsets.push(offset);
            }
        }
    }
    // Fill indices and depth buffer
    for i in 0..arr.shape[0] {
        for j in 0..arr.shape[1] {
            env.respect_execution_limit()?;
            for k in 0..arr.shape[2] {
                let arr_index = i * x_stride + j * y_stride + k;
                match mode {
                    Mode::Gray if arr.data[arr_index] == 0.0 => continue,
                    Mode::GrayA if arr.data[arr_index * 2 + 1] == 0.0 => continue,
                    Mode::Rgb if arr.data[arr_index * 3..][..3] == [0.0; 3] => continue,
                    Mode::Rgba if arr.data[arr_index * 4 + 3] == 0.0 => continue,
                    _ => {}
                }
                let corner = add([i, j, k].map(|d| d as f64), offset);
                for &offset in &voxel_surface_offsets {
                    let center = add(corner, offset);
                    let proj = plane_point(normal, d, center);
                    let delta = sub(center, proj);
                    let cam_delta = sub(proj, cam_center);
                    let x = (shell_radius - dot(cam_delta, u)) * scale;
                    let y = (shell_radius - dot(cam_delta, v)) * scale;
                    if x < 0.0 || y < 0.0 {
                        continue;
                    }
                    let x = x.floor() as usize;
                    let y = y.floor() as usize;
                    if x >= res_dim || y >= res_dim {
                        continue;
                    }
                    let dist = mag(delta);
                    let im_index = y * res_dim + x;
                    if dist < depth_buf[im_index] {
                        match mode {
                            Mode::GrayA if arr.data[arr_index * 2 + 1] != 1.0 => {
                                translucents.push((im_index, arr_index, dist))
                            }
                            Mode::Rgba if arr.data[arr_index * 4 + 3] != 1.0 => {
                                translucents.push((im_index, arr_index, dist))
                            }
                            _ => {
                                depth_buf[im_index] = dist;
                                idxs[im_index] = arr_index;
                            }
                        }
                    }
                }
            }
        }
    }
    // Render opaques
    let fog_mul =
        |depth: f64, alpha: f64| 1.0 - alpha * (depth - shell_dist) / (shell_radius * 2.0);
    if pix_size != 1 {
        res_shape.push(pix_size);
    }
    let mut res_data = if let Some(fog) = fog {
        if !fog_has_hue && matches!(mode, Mode::Gray | Mode::GrayA) {
            // Grayscale image with grayscale fog
            let fog = fog[0];
            let mut res_data = eco_vec![0f64; res_shape.elements()];
            for ((index, px), &depth) in idxs
                .into_iter()
                .zip(res_data.make_mut().chunks_exact_mut(pix_size))
                .zip(&depth_buf)
            {
                if depth == f64::INFINITY {
                    continue;
                }
                let factor = fog_mul(depth, 1.0);
                px[0] = arr.data[index * vox_size] * factor + fog * (1.0 - factor);
                if mode == Mode::GrayA {
                    px[1] = 1.0;
                }
            }
            res_data
        } else {
            // Image with colored fog
            let mut res_data = eco_vec![0f64; res_shape.elements()];
            for ((index, px), &depth) in idxs
                .into_iter()
                .zip(res_data.make_mut().chunks_exact_mut(pix_size))
                .zip(&depth_buf)
            {
                if depth == f64::INFINITY {
                    continue;
                }
                let factor = fog_mul(depth, 1.0);
                match mode {
                    Mode::Gray | Mode::GrayA => {
                        for i in 0..3 {
                            px[i] = arr.data[index * vox_size] * factor + fog[i] * (1.0 - factor);
                        }
                    }
                    Mode::Rgb | Mode::Rgba => {
                        for i in 0..3 {
                            px[i] =
                                arr.data[index * vox_size + i] * factor + fog[i] * (1.0 - factor);
                        }
                    }
                }
                if matches!(mode, Mode::GrayA | Mode::Rgba) {
                    px[pix_size - 1] = 1.0;
                }
            }
            res_data
        }
    } else {
        match mode {
            Mode::Gray | Mode::GrayA => {
                // Grayscale image without fog
                let mut res_data = eco_vec![0f64; res_shape.elements()];
                for ((index, px), &depth) in idxs
                    .into_iter()
                    .zip(res_data.make_mut().chunks_exact_mut(pix_size))
                    .zip(&depth_buf)
                {
                    if depth == f64::INFINITY {
                        continue;
                    }
                    px[0] = arr.data[index * vox_size];
                    if mode == Mode::GrayA {
                        px[1] = 1.0;
                    }
                }
                res_data
            }
            Mode::Rgb | Mode::Rgba => {
                // Colored image without fog
                let mut res_data = eco_vec![0f64; res_shape.elements()];
                for ((index, px), &depth) in idxs
                    .into_iter()
                    .zip(res_data.make_mut().chunks_exact_mut(pix_size))
                    .zip(&depth_buf)
                {
                    if depth == f64::INFINITY {
                        continue;
                    }
                    for i in 0..color_size {
                        px[i] = arr.data[index * vox_size + i];
                    }
                    if matches!(mode, Mode::Rgba) {
                        px[3] = 1.0;
                    }
                }
                res_data
            }
        }
    };
    // Render translucents
    translucents.sort_by(|(ai, aa, ad), (bi, ba, bd)| {
        ((ai, aa).cmp(&(bi, ba))).then_with(|| ad.partial_cmp(bd).unwrap())
    });
    translucents.dedup_by_key(|(i, a, _)| (*i, *a));
    translucents.sort_by(|(_, _, a), (_, _, b)| a.partial_cmp(b).unwrap());
    let image = res_data.make_mut();
    for (im_index, arr_index, dist) in translucents.into_iter().rev() {
        if depth_buf[im_index] < dist {
            continue;
        }
        let vox_alpha = arr.data[arr_index * vox_size + color_size];
        for i in 0..color_size {
            let bg = image[im_index * pix_size + i];
            let fg = arr.data[arr_index * vox_size + i];
            let new = (1.0 - vox_alpha) * bg + vox_alpha * fg;
            image[im_index * pix_size + i] = new;
        }
        image[im_index * pix_size + color_size] =
            (image[im_index * pix_size + color_size] + vox_alpha).min(1.0);
        if let Some(fog) = fog {
            let factor = fog_mul(dist, vox_alpha);
            for i in 0..color_size {
                image[im_index * pix_size + i] =
                    image[im_index * pix_size + i] * factor + fog[i] * (1.0 - factor);
            }
        }
    }
    Ok(Array::new(res_shape, res_data).into())
}

builtin_params!(
    LayoutParam,
    (LineHeight, "The height of a line"),
    (Size, "Size of the rendering area"),
    (Color, "Text color"),
    (Bg, "Background color"),
);

pub(crate) fn layout_text(size: Value, text: Value, env: &mut Uiua) -> UiuaResult<Value> {
    #[cfg(feature = "font_shaping")]
    {
        layout_text_impl(size, text, env)
    }
    #[cfg(not(feature = "font_shaping"))]
    Err(env.error("Text layout is not supported in this environment"))
}

#[cfg(feature = "font_shaping")]
fn layout_text_impl(size: Value, text: Value, env: &mut Uiua) -> UiuaResult<Value> {
    use std::{cell::RefCell, iter::repeat_n};

    use cosmic_text::*;
    use ecow::eco_vec;

    use crate::{algorithm::validate_size, grid_fmt::GridFmt, Boxed, Shape};
    struct FontStuff {
        system: FontSystem,
        swash_cache: SwashCache,
    }
    thread_local! {
        static FONT_STUFF: RefCell<Option<FontStuff>> = const { RefCell::new(None) };
    }

    let args = take(&mut env.rt.set_args);

    let mut string = String::new();
    match text {
        Value::Char(arr) if arr.rank() <= 1 => string = arr.data.iter().copied().collect(),
        Value::Char(arr) if arr.rank() == 2 => {
            for (i, row) in arr.row_slices().enumerate() {
                if i > 0 {
                    string.push('\n');
                }
                string.extend(row.iter().copied());
            }
        }
        Value::Box(arr) if arr.rank() == 1 => {
            for (i, Boxed(val)) in arr.data.iter().enumerate() {
                if i > 0 {
                    string.push('\n');
                }
                match val {
                    Value::Char(arr) if arr.rank() <= 1 => string.extend(arr.data.iter().copied()),
                    Value::Char(arr) if arr.rank() == 2 => {
                        for (j, row) in arr.row_slices().enumerate() {
                            if j > 0 {
                                string.push(' ');
                            }
                            string.extend(row.iter().copied());
                        }
                    }
                    Value::Box(arr) if arr.rank() == 1 => {
                        for (j, Boxed(val)) in arr.data.iter().enumerate() {
                            if j > 0 {
                                string.push(' ');
                            }
                            string.push_str(&val.as_string(env, "Text word must be a string")?);
                        }
                    }
                    _ => string.push_str(&val.as_string(env, "Text line must be a string")?),
                }
            }
        }
        Value::Box(arr) if arr.rank() == 2 => {
            for (i, row) in arr.row_slices().enumerate() {
                if i > 0 {
                    string.push('\n');
                }
                for (j, Boxed(val)) in row.iter().enumerate() {
                    if j > 0 {
                        string.push(' ');
                    }
                    string.push_str(&val.as_string(env, "Text word must be a string")?);
                }
            }
        }
        val => {
            string = val.as_string(env, "Text must be a rank 0, 1, or 2 character or box array")?
        }
    }

    // Default options
    let size = size.as_num(env, "Size must be a number")? as f32;
    if size <= 0.0 {
        return Err(env.error("Text size must be positive"));
    }
    let mut line_height = 1.0;
    let mut width = None;
    let mut height = None;
    let mut color: Option<Color> = None;
    let mut bg = None;

    // Parse options
    for (arg, index) in args {
        match all::<LayoutParam>().nth(index).unwrap() {
            LayoutParam::LineHeight => {
                line_height = arg.as_num(env, "Line height must be a scalar number")? as f32
            }
            LayoutParam::Size => {
                let nums = arg.as_nums(env, "Size must be a scalar number or 2 numbers")?;
                let [h, w] = match *nums {
                    [s] if arg.shape.is_empty() => [s; 2],
                    [h, w] => [h, w],
                    _ => {
                        return Err(env.error(format!(
                            "Size must be a scalar or list of 2 numbers, but its shape is {}",
                            arg.shape
                        )))
                    }
                };
                if w < 0.0 || w.is_nan() {
                    return Err(env.error(format!(
                        "Canvas width must be a non-negative number, but it is {}",
                        w.grid_string(false)
                    )));
                }
                if h < 0.0 || h.is_nan() {
                    return Err(env.error(format!(
                        "Canvas height must be a non-negative number, but it is {}",
                        h.grid_string(false)
                    )));
                }
                if !w.is_infinite() {
                    width = Some(w as f32);
                }
                if !h.is_infinite() {
                    height = Some(h as f32);
                }
            }
            LayoutParam::Color => {
                let nums = arg.as_nums(
                    env,
                    "Color must be a scalar number or list of 3 or 4 numbers",
                )?;
                let ([r, g, b], a) = match *nums {
                    [gray] if arg.shape.is_empty() => ([gray; 3], None),
                    [r, g, b] => ([r, g, b], None),
                    [r, g, b, a] => ([r, g, b], Some(a)),
                    _ => {
                        return Err(env.error(format!(
                            "Color must be a scalar or list of 3 or 4 numbers, but its shape is {}",
                            arg.shape
                        )))
                    }
                };
                color = Some(if let Some(a) = a {
                    Color::rgba(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        (a * 255.0) as u8,
                    )
                } else {
                    Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
                });
            }
            LayoutParam::Bg => {
                bg = Some(arg.as_number_array::<f64>(env, "Background color must be numbers")?)
            }
        }
    }

    line_height *= size;
    let metrics = Metrics::new(size, line_height);

    FONT_STUFF.with(|stuff| -> UiuaResult<Value> {
        let mut stuff = stuff.borrow_mut();
        if stuff.is_none() {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db.set_monospace_family("Uiua386");
            db.set_sans_serif_family("Uiua386");
            db.load_font_data(include_bytes!("Uiua386.ttf").to_vec());
            let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
            let system = FontSystem::new_with_locale_and_db(locale, db);
            *stuff = Some(FontStuff {
                system,
                swash_cache: SwashCache::new(),
            });
        }
        let FontStuff {
            system,
            swash_cache,
        } = stuff.as_mut().unwrap();
        // Init buffer
        let mut buffer = Buffer::new(system, metrics);
        let mut buffer = buffer.borrow_with(system);
        buffer.set_size(width, height);
        let attrs = Attrs::new();
        buffer.set_text(&string, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(true);

        // Get canvas size
        let canvas_width = width.unwrap_or_else(|| {
            buffer
                .layout_runs()
                .map(|run| run.line_w)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0)
        });
        let canvas_height =
            height.unwrap_or_else(|| buffer.layout_runs().map(|run| run.line_height).sum::<f32>());

        // Init array shape/data
        let colored = color.is_some() || bg.is_some();
        let pixel_shape: &[usize] = if colored { &[4] } else { &[] };
        let mut canvas_shape = Shape::from_iter([canvas_height as usize, canvas_width as usize]);
        canvas_shape.extend(pixel_shape.iter().copied());
        let elem_count = validate_size::<f64>(canvas_shape.iter().copied(), env)?;
        let mut canvas_data = if let Some(bg) = bg {
            let color = match &*bg.shape {
                [] | [1] => [bg.data[0], bg.data[0], bg.data[0], 1.0],
                [3] | [4] => {
                    let alpha = bg.data.get(3).copied().unwrap_or(1.0);
                    [bg.data[0], bg.data[1], bg.data[2], alpha]
                }
                _ => return Err(env.error("Background color must be a list of 3 or 4 numbers")),
            };
            repeat_n(color, elem_count / 4).flatten().collect()
        } else {
            eco_vec![0.0; elem_count]
        };
        let slice = canvas_data.make_mut();
        // Render
        let color = color.unwrap_or(Color::rgb(0xFF, 0xFF, 0xFF));
        if color.a() == 0 {
            return Ok(Array::new(canvas_shape, canvas_data).into());
        }
        let a = color.a() as f64 / 255.0;
        buffer.draw(swash_cache, color, |x, y, _, _, color| {
            let alpha = color.a();
            if alpha == 0
                || (x < 0 || x >= canvas_width as i32)
                || (y < 0 || y >= canvas_height as i32)
            {
                return;
            }
            let i = (y * canvas_width as i32 + x) as usize;
            if colored {
                let a = a * alpha as f64 / 255.0;
                if a == 1.0 {
                    slice[i * 4] = color.r() as f64 / 255.0;
                    slice[i * 4 + 1] = color.g() as f64 / 255.0;
                    slice[i * 4 + 2] = color.b() as f64 / 255.0;
                    slice[i * 4 + 3] = 1.0;
                } else {
                    let [tr, tg, tb, ta, ..] = &mut slice[i * 4..] else {
                        unreachable!()
                    };
                    *tr = *tr * *ta * (1.0 - a) + color.r() as f64 / 255.0 * a;
                    *tg = *tg * *ta * (1.0 - a) + color.g() as f64 / 255.0 * a;
                    *tb = *tb * *ta * (1.0 - a) + color.b() as f64 / 255.0 * a;
                    *ta = 1.0 - ((1.0 - *ta) * (1.0 - a));
                }
            } else {
                slice[i] = color.a() as f64 / 255.0;
            }
        });
        Ok(Array::new(canvas_shape, canvas_data).into())
    })
}
