//! Windows Media Foundation hardware H.264 / H.265 decoder.
//!
//! Instantiates the OS-bundled `IMFTransform` for the requested codec (H.264 / HEVC),
//! configures input and output media types, and runs a `ProcessInput` / `ProcessOutput`
//! pump. Output is requested as NV12 because every MF hardware decoder supports it.
//! Frames are handed to the caller as native NV12 (Y in `plane_primary`, interleaved
//! UV in `plane_secondary`) so downstream code can do colour conversion on the GPU.

use super::{VideoCodec, VideoDecoder};
use crate::error::Error;
use crate::types::{Frame, FrameQuality, PixelFormat};
use bytes::Bytes;
use std::time::Duration;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::GUID;

/// Media Foundation decoder instance.
pub(crate) struct MediaFoundationDecoder {
    transform: IMFTransform,
    output_type: IMFMediaType,
    width: u32,
    height: u32,
    stride: u32,
    #[allow(dead_code)]
    codec: VideoCodec,
    parameter_sets_annex_b: Vec<u8>,
    mf_started: bool,
}

unsafe impl Send for MediaFoundationDecoder {}

impl VideoDecoder for MediaFoundationDecoder {
    fn new(codec: VideoCodec, extradata: &[u8]) -> Result<Self, Error> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(map_error)?;
        }

        let subtype = match codec {
            VideoCodec::H264 => MFVideoFormat_H264,
            VideoCodec::H265 => MFVideoFormat_HEVC,
        };

        let transform = activate_decoder(subtype)?;
        unsafe {
            if let Ok(attrs) = transform.GetAttributes() {
                let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            }
        }
        let parameter_sets_annex_b = extradata_to_annex_b(codec, extradata);
        let (input_type, width, height) = build_input_type(codec, subtype, extradata)?;
        unsafe {
            transform
                .SetInputType(0, &input_type, 0)
                .map_err(map_error)?;
        }

        let output_type = select_output_type(&transform, width, height)?;
        unsafe {
            transform
                .SetOutputType(0, &output_type, 0)
                .map_err(map_error)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(map_error)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(map_error)?;
        }

        let stride = nv12_stride(width);

        Ok(Self {
            transform,
            output_type,
            width,
            height,
            stride,
            codec,
            parameter_sets_annex_b,
            mf_started: true,
        })
    }

    fn decode(&mut self, nal: &[u8], timestamp: Duration) -> Result<Vec<Frame>, Error> {
        let converted = avcc_to_annex_b(nal);
        if converted.is_empty() {
            return Ok(Vec::new());
        }
        let mut annex_b = Vec::with_capacity(self.parameter_sets_annex_b.len() + converted.len());
        annex_b.extend_from_slice(&self.parameter_sets_annex_b);
        annex_b.extend_from_slice(&converted);
        let input_sample = build_input_sample(&annex_b, timestamp)?;
        unsafe {
            self.transform
                .ProcessInput(0, &input_sample, 0)
                .map_err(map_error)?;
        }

        let mut frames = Vec::new();
        while let Some(frame) = self.pull_output()? {
            frames.push(frame);
        }
        Ok(frames)
    }
}

fn avcc_to_annex_b(avcc: &[u8]) -> Vec<u8> {
    const START_CODE: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
    let mut annex_b = Vec::with_capacity(avcc.len());
    let mut offset = 0;
    while offset + 4 <= avcc.len() {
        let len = u32::from_be_bytes([
            avcc[offset],
            avcc[offset + 1],
            avcc[offset + 2],
            avcc[offset + 3],
        ]) as usize;
        offset += 4;
        if len == 0 || offset + len > avcc.len() {
            break;
        }
        annex_b.extend_from_slice(&START_CODE);
        annex_b.extend_from_slice(&avcc[offset..offset + len]);
        offset += len;
    }
    annex_b
}

impl MediaFoundationDecoder {
    fn pull_output(&mut self) -> Result<Option<Frame>, Error> {
        for _ in 0..4 {
            let info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(map_error)?;
            let provides_samples =
                info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;

            let pre_allocated = if provides_samples {
                None
            } else {
                let required = info.cbSize.max(1);
                Some(create_output_sample(required)?)
            };

            let mut buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(pre_allocated),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };

            let mut status = 0u32;
            let hr = unsafe {
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status)
            };

            match hr {
                Ok(()) => {
                    let decoded = unsafe { std::mem::ManuallyDrop::take(&mut buffer.pSample) };
                    let Some(sample) = decoded else {
                        return Ok(None);
                    };
                    if self.width == 0 || self.height == 0 {
                        return Ok(None);
                    }
                    let (y_plane, uv_plane) = sample_to_nv12(&sample, self.height, self.stride)?;
                    return Ok(Some(Frame {
                        width: self.width,
                        height: self.height,
                        stride: self.stride,
                        timestamp: Duration::ZERO,
                        pixel_format: PixelFormat::Nv12,
                        quality: FrameQuality::Intact,
                        plane_primary: Bytes::from(y_plane),
                        plane_secondary: Bytes::from(uv_plane),
                    }));
                }
                Err(error) if error.code().0 == MF_E_TRANSFORM_NEED_MORE_INPUT.0 => {
                    return Ok(None);
                }
                Err(error) if error.code().0 == MF_E_TRANSFORM_STREAM_CHANGE.0 => {
                    self.reconfigure_output()?;
                    continue;
                }
                Err(error) => {
                    return Err(Error::Backend {
                        platform: "windows",
                        message: format!("ProcessOutput failed: {}", error.message()),
                    });
                }
            }
        }
        Ok(None)
    }

    fn reconfigure_output(&mut self) -> Result<(), Error> {
        let mut index = 0u32;
        loop {
            let next = unsafe { self.transform.GetOutputAvailableType(0, index) };
            match next {
                Ok(media_type) => {
                    let Ok(subtype) = (unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }) else {
                        index += 1;
                        continue;
                    };
                    if subtype == MFVideoFormat_NV12 {
                        unsafe {
                            self.transform
                                .SetOutputType(0, &media_type, 0)
                                .map_err(map_error)?;
                        }
                        let packed =
                            unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE).map_err(map_error)? };
                        self.width = (packed >> 32) as u32;
                        self.height = (packed & 0xFFFF_FFFF) as u32;
                        self.stride = unsafe { media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
                            .unwrap_or_else(|_| nv12_stride(self.width));
                        self.output_type = media_type;
                        return Ok(());
                    }
                    index += 1;
                }
                Err(_) => break,
            }
        }
        Err(Error::Backend {
            platform: "windows",
            message: "no NV12 output type available on stream change".into(),
        })
    }
}

impl Drop for MediaFoundationDecoder {
    fn drop(&mut self) {
        if self.mf_started {
            unsafe {
                let _ = self
                    .transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
                let _ = self
                    .transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
                let _ = MFShutdown();
            }
        }
    }
}

fn activate_decoder(subtype: GUID) -> Result<IMFTransform, Error> {
    let info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&info),
            None,
            &mut activates,
            &mut count,
        )
        .map_err(map_error)?;
    }
    if count == 0 {
        return Err(Error::Backend {
            platform: "windows",
            message: "no Media Foundation decoder registered".into(),
        });
    }
    let first = unsafe { (*activates).clone() }.ok_or(Error::Backend {
        platform: "windows",
        message: "null IMFActivate".into(),
    })?;
    let transform: IMFTransform = unsafe { first.ActivateObject() }.map_err(map_error)?;
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(activates as *const _)) };
    Ok(transform)
}

fn build_input_type(
    codec: VideoCodec,
    subtype: GUID,
    extradata: &[u8],
) -> Result<(IMFMediaType, u32, u32), Error> {
    let media_type = unsafe { MFCreateMediaType().map_err(map_error)? };
    unsafe {
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(map_error)?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, &subtype)
            .map_err(map_error)?;
        media_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(map_error)?;
        if !extradata.is_empty() {
            let sequence_header = extradata_to_annex_b(codec, extradata);
            if !sequence_header.is_empty() {
                media_type
                    .SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &sequence_header)
                    .map_err(map_error)?;
            }
        }
    }
    Ok((media_type, 0, 0))
}

fn extradata_to_annex_b(codec: VideoCodec, extradata: &[u8]) -> Vec<u8> {
    const START_CODE: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
    let mut out = Vec::with_capacity(extradata.len() + 16);
    match codec {
        VideoCodec::H264 => {
            if extradata.len() < 7 {
                return out;
            }
            let num_sps = (extradata[5] & 0x1F) as usize;
            let mut offset = 6;
            for _ in 0..num_sps {
                if offset + 2 > extradata.len() {
                    return out;
                }
                let len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
                offset += 2;
                if offset + len > extradata.len() {
                    return out;
                }
                out.extend_from_slice(&START_CODE);
                out.extend_from_slice(&extradata[offset..offset + len]);
                offset += len;
            }
            if offset >= extradata.len() {
                return out;
            }
            let num_pps = extradata[offset] as usize;
            offset += 1;
            for _ in 0..num_pps {
                if offset + 2 > extradata.len() {
                    return out;
                }
                let len = u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
                offset += 2;
                if offset + len > extradata.len() {
                    return out;
                }
                out.extend_from_slice(&START_CODE);
                out.extend_from_slice(&extradata[offset..offset + len]);
                offset += len;
            }
        }
        VideoCodec::H265 => {
            if extradata.len() < 23 {
                return out;
            }
            let num_arrays = extradata[22] as usize;
            let mut offset = 23;
            for _ in 0..num_arrays {
                if offset + 3 > extradata.len() {
                    return out;
                }
                let num_nalus =
                    u16::from_be_bytes([extradata[offset + 1], extradata[offset + 2]]) as usize;
                offset += 3;
                for _ in 0..num_nalus {
                    if offset + 2 > extradata.len() {
                        return out;
                    }
                    let len =
                        u16::from_be_bytes([extradata[offset], extradata[offset + 1]]) as usize;
                    offset += 2;
                    if offset + len > extradata.len() {
                        return out;
                    }
                    out.extend_from_slice(&START_CODE);
                    out.extend_from_slice(&extradata[offset..offset + len]);
                    offset += len;
                }
            }
        }
    }
    out
}

fn select_output_type(
    transform: &IMFTransform,
    _width: u32,
    _height: u32,
) -> Result<IMFMediaType, Error> {
    let mut index = 0u32;
    loop {
        let next = unsafe { transform.GetOutputAvailableType(0, index) };
        match next {
            Ok(media_type) => {
                let Ok(subtype) = (unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }) else {
                    index += 1;
                    continue;
                };
                if subtype == MFVideoFormat_NV12 {
                    return Ok(media_type);
                }
                index += 1;
            }
            Err(_) => break,
        }
    }
    Err(Error::Backend {
        platform: "windows",
        message: "no NV12 output type advertised by decoder".into(),
    })
}

fn build_input_sample(nal: &[u8], timestamp: Duration) -> Result<IMFSample, Error> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(nal.len() as u32).map_err(map_error)?;
        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut max_len = 0u32;
        let mut cur_len = 0u32;
        buffer
            .Lock(&mut data_ptr, Some(&mut max_len), Some(&mut cur_len))
            .map_err(map_error)?;
        std::ptr::copy_nonoverlapping(nal.as_ptr(), data_ptr, nal.len());
        buffer
            .SetCurrentLength(nal.len() as u32)
            .map_err(map_error)?;
        let _ = buffer.Unlock();

        let sample = MFCreateSample().map_err(map_error)?;
        sample.AddBuffer(&buffer).map_err(map_error)?;
        sample
            .SetSampleTime(timestamp.as_nanos() as i64 / 100)
            .map_err(map_error)?;
        Ok(sample)
    }
}

fn create_output_sample(size: u32) -> Result<IMFSample, Error> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(size).map_err(map_error)?;
        let sample = MFCreateSample().map_err(map_error)?;
        sample.AddBuffer(&buffer).map_err(map_error)?;
        Ok(sample)
    }
}

fn sample_to_nv12(
    sample: &IMFSample,
    height: u32,
    stride: u32,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer().map_err(map_error)? };
    let mut base_ptr: *mut u8 = std::ptr::null_mut();
    let mut max_length = 0u32;
    let mut current_length = 0u32;
    unsafe {
        buffer
            .Lock(
                &mut base_ptr,
                Some(&mut max_length),
                Some(&mut current_length),
            )
            .map_err(map_error)?;
    }

    let height_usize = height as usize;
    let stride_usize = stride as usize;
    let y_size = stride_usize * height_usize;
    let uv_size = stride_usize * (height_usize / 2);
    let total = y_size + uv_size;

    let (y_plane, uv_plane) = if !base_ptr.is_null() && (current_length as usize) >= total {
        let slice = unsafe { std::slice::from_raw_parts(base_ptr, total) };
        (slice[..y_size].to_vec(), slice[y_size..].to_vec())
    } else {
        (vec![0u8; y_size], vec![0u8; uv_size])
    };

    unsafe {
        let _ = buffer.Unlock();
    }
    Ok((y_plane, uv_plane))
}

fn nv12_stride(width: u32) -> u32 {
    (width + 15) & !15
}

fn map_error(error: windows::core::Error) -> Error {
    Error::Backend {
        platform: "windows",
        message: error.message().to_string(),
    }
}
