use std::fs::File;
use std::io::Write;
use std::num::NonZero;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use mp3lame_encoder::{Builder, DualPcm, FlushNoGap, MonoPcm};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::CODEC_ID_VORBIS;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// Audio processing service for decoding, resampling, and encoding audio files
pub struct AudioProcessor {
    /// Target sample rate for resampling (default 44100 Hz)
    target_sample_rate: u32,
    /// OGG encoding quality (-0.1 to 1.0, default 0.5)
    ogg_quality: f32,
    /// Requested output channel count (`-c:N`); `None` keeps the source's
    channels: Option<usize>,
    /// Output format the manifest declares (`-fmt:<format>`); checked against the target extension
    declared_format: Option<AudioFormat>,
    /// MP3 bit rate in kbit/s (`-b:N`, default 192)
    mp3_bitrate: Mp3Bitrate,
    /// Whether `-b` was given
    bitrate_given: bool,
}

/// A manifest audio parameter this build refuses, with the token and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioParamError {
    /// The offending token as written in the manifest, e.g. `-b:100`.
    pub token: String,
    /// Why it is refused.
    pub reason: String,
}

impl std::fmt::Display for AudioParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "audio parameter `{}`: {}", self.token, self.reason)
    }
}

impl std::error::Error for AudioParamError {}

/// The MP3 bit rates LAME encodes at, in kbit/s. Any other value is refused, never rounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp3Bitrate(u32);

impl Mp3Bitrate {
    const SUPPORTED: [u32; 16] = [
        8, 16, 24, 32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];

    /// The bit rate in kbit/s.
    pub fn kbps(self) -> u32 {
        self.0
    }

    fn lame(self) -> mp3lame_encoder::Bitrate {
        use mp3lame_encoder::Bitrate as B;
        match self.0 {
            8 => B::Kbps8,
            16 => B::Kbps16,
            24 => B::Kbps24,
            32 => B::Kbps32,
            40 => B::Kbps40,
            48 => B::Kbps48,
            64 => B::Kbps64,
            80 => B::Kbps80,
            96 => B::Kbps96,
            112 => B::Kbps112,
            128 => B::Kbps128,
            160 => B::Kbps160,
            224 => B::Kbps224,
            256 => B::Kbps256,
            320 => B::Kbps320,
            _ => B::Kbps192,
        }
    }
}

impl AudioProcessor {
    pub fn new() -> Self {
        Self {
            target_sample_rate: 44100,
            ogg_quality: 0.5,
            channels: None,
            declared_format: None,
            mp3_bitrate: Mp3Bitrate(192),
            bitrate_given: false,
        }
    }

    pub fn with_target_sample_rate(mut self, rate: u32) -> Self {
        self.target_sample_rate = rate;
        self
    }

    pub fn with_ogg_quality(mut self, quality: f32) -> Self {
        self.ogg_quality = quality.clamp(-0.1, 1.0);
        self
    }

    /// Configure from MPI manifest params string (e.g. "-f:24000 -q:5" or "-c:2 -fmt:mp3 -b:192").
    ///
    /// - `-f:<hz>` target sample rate
    /// - `-q:<0-10>` OGG quality (mapped to -0.1..1.0)
    /// - `-c:<1|2>` output channel count (mono and stereo convert into each other)
    /// - `-fmt:<ogg|wav|mp3>` the output format; must match the target extension
    /// - `-b:<kbps>` MP3 bit rate, one of 8, 16, 24, 32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320
    ///
    /// Any other token, or a value outside what is listed, is an error: nothing is ignored or rounded.
    pub fn with_params(mut self, params: &str) -> Result<Self, AudioParamError> {
        let bad = |token: &str, reason: &str| AudioParamError {
            token: token.to_owned(),
            reason: reason.to_owned(),
        };
        for token in params.split_whitespace() {
            let (key, value) = token.split_once(':').unwrap_or((token, ""));
            match key {
                "-f" => match value.parse::<u32>() {
                    Ok(freq) if freq > 0 => self.target_sample_rate = freq,
                    _ => {
                        return Err(bad(
                            token,
                            "the sample rate must be a positive number of Hz",
                        ));
                    }
                },
                "-q" => match value.parse::<f32>() {
                    Ok(q) if q.is_finite() && (-1.0..=10.0).contains(&q) => {
                        // OggEnc2 quality scale: 0-10 maps to vorbis -0.1 to 1.0
                        self.ogg_quality = (q / 10.0).clamp(-0.1, 1.0);
                    }
                    _ => return Err(bad(token, "the quality must be a number from -1 to 10")),
                },
                "-c" => match value.parse::<usize>() {
                    Ok(c @ (1 | 2)) => self.channels = Some(c),
                    _ => {
                        return Err(bad(
                            token,
                            "only 1 (mono) or 2 (stereo) channels are supported",
                        ));
                    }
                },
                "-fmt" => match AudioFormat::from_extension(value) {
                    Some(format) => self.declared_format = Some(format),
                    None => return Err(bad(token, "the format must be ogg, wav or mp3")),
                },
                "-b" => match value.parse::<u32>() {
                    Ok(kbps) if Mp3Bitrate::SUPPORTED.contains(&kbps) => {
                        self.mp3_bitrate = Mp3Bitrate(kbps);
                        self.bitrate_given = true;
                    }
                    _ => {
                        return Err(bad(
                            token,
                            "the bit rate must be one of 8, 16, 24, 32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256 or 320 kbit/s",
                        ));
                    }
                },
                _ => return Err(bad(token, "this parameter is not implemented")),
            }
        }
        Ok(self)
    }

    /// Refuse parameters that cannot apply to an output of `format`: a declared `-fmt` that is not it,
    /// or a `-b` bit rate on anything but MP3.
    pub fn check_output(&self, format: AudioFormat) -> Result<()> {
        if let Some(declared) = self.declared_format
            && declared != format
        {
            bail!(
                "the manifest declares `-fmt:{}` but the target file is {}",
                declared.name(),
                format.name()
            );
        }
        if self.bitrate_given && format != AudioFormat::Mp3 {
            bail!(
                "`-b` sets an MP3 bit rate, but the target file is {}",
                format.name()
            );
        }
        Ok(())
    }

    /// Refuse parameters the Ogg resample operation (op 4) cannot honour.
    fn check_ogg_resample(&self) -> Result<()> {
        if self.channels.is_some() || self.declared_format.is_some() || self.bitrate_given {
            bail!("the Ogg resample operation only takes `-f` and `-q`");
        }
        Ok(())
    }

    /// Convert to the requested channel count (mono to stereo duplicates, stereo to mono averages).
    fn remix(&self, mut audio: DecodedAudio) -> Result<DecodedAudio> {
        let Some(target) = self.channels else {
            return Ok(audio);
        };
        match (audio.channels, target) {
            (from, to) if from == to => {}
            (1, 2) => {
                audio.samples = audio.samples.iter().flat_map(|&s| [s, s]).collect();
                audio.channels = 2;
            }
            (2, 1) => {
                audio.samples = audio
                    .samples
                    .chunks(2)
                    .map(|f| f.iter().sum::<f32>() / f.len() as f32)
                    .collect();
                audio.channels = 1;
            }
            (from, to) => bail!("cannot convert {from}-channel audio to {to} channels"),
        }
        Ok(audio)
    }

    /// Decode an audio file to raw PCM samples
    /// Returns (samples, channels, sample_rate)
    pub fn decode_file(&self, path: &Path) -> Result<DecodedAudio> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        Self::decode_stream(mss, &hint)
    }

    /// Decode audio from raw bytes (in-memory)
    pub fn decode_bytes(&self, data: &[u8], format_hint: Option<&str>) -> Result<DecodedAudio> {
        let cursor = std::io::Cursor::new(data.to_vec());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = format_hint {
            hint.with_extension(ext);
        }

        Self::decode_stream(mss, &hint)
    }

    fn probe_format(mss: MediaSourceStream<'static>, hint: &Hint) -> Result<Box<dyn FormatReader>> {
        symphonia::default::get_probe()
            .probe(
                hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .context("Failed to probe audio format")
    }

    fn decode_stream(mss: MediaSourceStream<'static>, hint: &Hint) -> Result<DecodedAudio> {
        let mut format = Self::probe_format(mss, hint)?;
        let track = format
            .default_track(TrackType::Audio)
            .context("No audio tracks found")?;

        let track_id = track.id;
        let codec_params = match track.codec_params.as_ref() {
            Some(CodecParameters::Audio(params)) => params.clone(),
            _ => bail!("No audio tracks found"),
        };

        let sample_rate = codec_params.sample_rate.context("Unknown sample rate")?;
        let channels = codec_params
            .channels
            .as_ref()
            .context("Unknown channel count")?
            .count();

        // Symphonia 0.5 never trimmed encoder delay or padding; 0.6 does by default.
        let mut decoder_opts = AudioDecoderOptions::default();
        decoder_opts.gapless = false;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&codec_params, &decoder_opts)
            .context("Failed to create decoder")?;

        let mut all_samples: Vec<f32> = Vec::new();
        let mut scratch: Vec<f32> = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            };

            if packet.track_id != track_id {
                continue;
            }

            let decoded = decoder.decode(&packet)?;
            Self::append_samples(&decoded, &mut scratch, &mut all_samples);
        }

        Ok(DecodedAudio {
            samples: all_samples,
            channels,
            sample_rate,
        })
    }

    fn probe_audio_info(data: &[u8], format_hint: Option<&str>) -> Result<AudioInfo> {
        let cursor = std::io::Cursor::new(data.to_vec());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = format_hint {
            hint.with_extension(ext);
        }

        let format = Self::probe_format(mss, &hint)?;
        let track = format
            .default_track(TrackType::Audio)
            .context("No audio tracks found")?;
        let Some(CodecParameters::Audio(codec_params)) = track.codec_params.as_ref() else {
            bail!("No audio tracks found");
        };

        Ok(AudioInfo {
            codec_is_vorbis: codec_params.codec == CODEC_ID_VORBIS,
            sample_rate: codec_params.sample_rate,
            channels: codec_params
                .channels
                .as_ref()
                .map(|channels| channels.count()),
        })
    }

    /// Append samples from a Symphonia audio buffer in interleaved format
    /// ([L1, R1, L2, R2, ...]), converted to f32. `scratch` is reused between calls.
    fn append_samples(
        buffer: &GenericAudioBufferRef<'_>,
        scratch: &mut Vec<f32>,
        output: &mut Vec<f32>,
    ) {
        buffer.copy_to_vec_interleaved(scratch);
        output.extend_from_slice(scratch);
    }

    /// Resample audio to target sample rate
    /// Uses settings matching oggenc2's SRC_SINC_FASTEST (libsamplerate)
    pub fn resample(&self, audio: DecodedAudio) -> Result<DecodedAudio> {
        self.resample_to_rate(&audio, self.target_sample_rate)
    }

    fn resample_to_rate(
        &self,
        audio: &DecodedAudio,
        target_sample_rate: u32,
    ) -> Result<DecodedAudio> {
        if audio.sample_rate == target_sample_rate {
            return Ok(audio.clone());
        }

        let channels = audio.channels;
        if channels == 0 {
            bail!("Cannot resample audio with zero channels");
        }
        let ratio = target_sample_rate as f64 / audio.sample_rate as f64;

        // Deinterleave samples into per-channel vectors
        let samples_per_channel = audio.samples.len() / channels;
        let mut channel_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); channels];

        for (i, sample) in audio.samples.iter().enumerate() {
            channel_data[i % channels].push(*sample);
        }

        let resampled =
            Self::resample_channels(channel_data, samples_per_channel, channels, ratio)?;

        // Interleave samples back together
        let output_len = resampled[0].len();
        let mut output = Vec::with_capacity(output_len * channels);
        for i in 0..output_len {
            for channel_samples in resampled.iter().take(channels) {
                output.push(channel_samples[i]);
            }
        }

        Ok(DecodedAudio {
            samples: output,
            channels,
            sample_rate: target_sample_rate,
        })
    }

    /// Resample per-channel data without interleaving the output.
    /// Returns Vec<Vec<f32>> (one vec per channel) - avoids redundant
    /// interleave+deinterleave when the next step (OGG encode) also wants per-channel data.
    ///
    /// Resampler params match oggenc2's default: libsamplerate SRC_SINC_FASTEST
    fn resample_channels(
        channel_data: Vec<Vec<f32>>,
        samples_per_channel: usize,
        channels: usize,
        ratio: f64,
    ) -> Result<Vec<Vec<f32>>> {
        // Match oggenc2's SRC_SINC_FASTEST quality
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 64,
            window: WindowFunction::BlackmanHarris2,
        };

        let mut resampler =
            SincFixedIn::<f32>::new(ratio, 2.0, params, samples_per_channel, channels)
                .context("Failed to create resampler")?;

        resampler
            .process(&channel_data, None)
            .context("Failed to resample audio")
    }

    /// Encode audio to OGG Vorbis format
    pub fn encode_ogg(&self, audio: &DecodedAudio) -> Result<Vec<u8>> {
        // Deinterleave into per-channel data for the encoder
        let samples_per_channel = audio.samples.len() / audio.channels;
        let mut channel_samples: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); audio.channels];

        for (i, &sample) in audio.samples.iter().enumerate() {
            channel_samples[i % audio.channels].push(sample.clamp(-1.0, 1.0));
        }

        Self::encode_ogg_channels(&channel_samples, audio.sample_rate, self.ogg_quality)
    }

    /// Encode per-channel data directly to OGG without interleaving first.
    /// Used by the optimized pipeline to skip the interleave -> deinterleave roundtrip.
    fn encode_ogg_channels(
        channel_data: &[Vec<f32>],
        sample_rate: u32,
        quality: f32,
    ) -> Result<Vec<u8>> {
        let channels = channel_data.len();
        let mut output = Vec::new();

        let sr = NonZero::new(sample_rate)
            .ok_or_else(|| anyhow::anyhow!("Sample rate cannot be zero"))?;
        let ch = NonZero::new(channels as u8)
            .ok_or_else(|| anyhow::anyhow!("Channel count cannot be zero"))?;

        let mut encoder = VorbisEncoderBuilder::new(sr, ch, &mut output)
            .map_err(|e| anyhow::anyhow!("Failed to create Vorbis encoder: {:?}", e))?;

        encoder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: quality,
        });

        let mut encoder = encoder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build Vorbis encoder: {:?}", e))?;

        let clamped: Vec<Vec<f32>> = channel_data
            .iter()
            .map(|ch| ch.iter().map(|s| s.clamp(-1.0, 1.0)).collect())
            .collect();
        let channel_slices: Vec<&[f32]> = clamped.iter().map(|v| v.as_slice()).collect();

        encoder
            .encode_audio_block(&channel_slices)
            .map_err(|e| anyhow::anyhow!("Failed to encode audio: {:?}", e))?;

        encoder
            .finish()
            .map_err(|e| anyhow::anyhow!("Failed to finish encoding: {:?}", e))?;

        Ok(output)
    }

    /// Encode audio to WAV format
    pub fn encode_wav(&self, audio: &DecodedAudio) -> Result<Vec<u8>> {
        let spec = WavSpec {
            channels: audio.channels as u16,
            sample_rate: audio.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut buffer = Vec::new();
        let cursor = std::io::Cursor::new(&mut buffer);
        let mut writer = WavWriter::new(cursor, spec).context("Failed to create WAV writer")?;

        for &sample in &audio.samples {
            let sample_i16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer
                .write_sample(sample_i16)
                .context("Failed to write sample")?;
        }

        writer.finalize().context("Failed to finalize WAV")?;

        Ok(buffer)
    }

    /// Process OggEnc2 operation: decode OGG, resample, re-encode to OGG.
    ///
    /// If the source is already Ogg Vorbis at the manifest target sample rate,
    /// copy it through unchanged. Vorbis files do not expose the original
    /// encoder quality setting reliably, so quality cannot be safely compared.
    pub fn process_ogg_resample(&self, input_data: &[u8]) -> Result<Vec<u8>> {
        self.check_ogg_resample()?;
        if self.can_copy_ogg_resample_input(input_data)? {
            return Ok(input_data.to_vec());
        }

        if Self::use_ffmpeg_audio_backend() {
            return self.process_ogg_resample_ffmpeg(input_data);
        }

        let decoded = self
            .decode_bytes(input_data, Some("ogg"))
            .context("Failed to decode OGG")?;

        let channels = decoded.channels;
        let samples_per_channel = decoded.samples.len() / channels;

        if decoded.sample_rate == self.target_sample_rate {
            return self.encode_ogg(&decoded);
        }

        // Deinterleave once (decode produces interleaved samples).
        let mut channel_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); channels];
        for (i, &sample) in decoded.samples.iter().enumerate() {
            channel_data[i % channels].push(sample);
        }

        let ratio = self.target_sample_rate as f64 / decoded.sample_rate as f64;
        let resampled = Self::resample_channels(channel_data, samples_per_channel, channels, ratio)
            .context("Failed to resample")?;

        Self::encode_ogg_channels(&resampled, self.target_sample_rate, self.ogg_quality)
            .context("Failed to encode OGG")
    }

    fn can_copy_ogg_resample_input(&self, input_data: &[u8]) -> Result<bool> {
        let info = Self::probe_audio_info(input_data, Some("ogg"))?;

        Ok(info.codec_is_vorbis
            && info.sample_rate == Some(self.target_sample_rate)
            && info.channels.is_some_and(|channels| channels > 0))
    }

    fn use_ffmpeg_audio_backend() -> bool {
        std::env::var("TTW_AUDIO_BACKEND")
            .is_ok_and(|backend| backend.eq_ignore_ascii_case("ffmpeg"))
    }

    fn process_ogg_resample_ffmpeg(&self, input_data: &[u8]) -> Result<Vec<u8>> {
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "ogg",
                "-i",
                "pipe:0",
                "-map",
                "0:a:0",
                "-vn",
                "-ar",
                &self.target_sample_rate.to_string(),
                "-c:a",
                "libvorbis",
                "-q:a",
                &self.ffmpeg_vorbis_quality().to_string(),
                "-f",
                "ogg",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start ffmpeg for OggEnc2")?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .context("Failed to open ffmpeg stdin")?;
            stdin
                .write_all(input_data)
                .context("Failed to write input audio to ffmpeg")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for ffmpeg")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ffmpeg OggEnc2 failed: {}", stderr.trim());
        }

        Ok(output.stdout)
    }

    fn ffmpeg_vorbis_quality(&self) -> String {
        let quality = (self.ogg_quality * 10.0).clamp(-1.0, 10.0);
        format!("{quality:.2}")
    }

    /// Process AudioEnc operation: convert to specified format
    pub fn process_audio_conversion(
        &self,
        input_data: &[u8],
        input_format: Option<&str>,
        output_format: AudioFormat,
    ) -> Result<Vec<u8>> {
        self.check_output(output_format)?;
        let decoded = self
            .decode_bytes(input_data, input_format)
            .context("Failed to decode audio")?;
        let decoded = self.remix(decoded)?;

        match output_format {
            AudioFormat::Ogg => {
                let resampled = self.resample(decoded).context("Failed to resample")?;
                self.encode_ogg(&resampled)
            }
            AudioFormat::Wav => {
                let resampled = self.resample(decoded).context("Failed to resample")?;
                self.encode_wav(&resampled)
            }
            AudioFormat::Mp3 => {
                let resampled = self.resample(decoded).context("Failed to resample")?;
                self.encode_mp3(&resampled)
            }
        }
    }

    /// Encode audio to MP3 using LAME
    fn encode_mp3(&self, audio: &DecodedAudio) -> Result<Vec<u8>> {
        use std::mem::MaybeUninit;

        let mut builder =
            Builder::new().ok_or_else(|| anyhow::anyhow!("Failed to create LAME encoder"))?;

        builder
            .set_num_channels(audio.channels as u8)
            .map_err(|e| anyhow::anyhow!("Failed to set channels: {:?}", e))?;
        builder
            .set_sample_rate(audio.sample_rate)
            .map_err(|e| anyhow::anyhow!("Failed to set sample rate: {:?}", e))?;
        builder
            .set_quality(mp3lame_encoder::Quality::Best)
            .map_err(|e| anyhow::anyhow!("Failed to set quality: {:?}", e))?;
        builder
            .set_brate(self.mp3_bitrate.lame())
            .map_err(|e| anyhow::anyhow!("Failed to set bitrate: {:?}", e))?;

        let mut encoder = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build LAME encoder: {:?}", e))?;

        // Convert f32 samples to i16
        let samples_i16: Vec<i16> = audio
            .samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();

        let mut mp3_data = Vec::new();

        if audio.channels == 1 {
            // Mono - the samples are one channel, not left/right pairs
            let input = MonoPcm(&samples_i16);
            let mut output: Vec<MaybeUninit<u8>> =
                vec![
                    MaybeUninit::uninit();
                    mp3lame_encoder::max_required_buffer_size(samples_i16.len())
                ];
            let encoded = encoder
                .encode(input, &mut output)
                .map_err(|e| anyhow::anyhow!("Failed to encode MP3: {:?}", e))?;
            // SAFETY: encode() initializes the first `encoded` bytes
            mp3_data.extend(output[..encoded].iter().map(|b| unsafe { b.assume_init() }));
        } else if audio.channels == 2 {
            // Stereo - deinterleave into dual PCM
            let frame_count = samples_i16.len() / 2;
            let mut left = Vec::with_capacity(frame_count);
            let mut right = Vec::with_capacity(frame_count);

            for chunk in samples_i16.chunks(2) {
                left.push(chunk[0]);
                right.push(chunk.get(1).copied().unwrap_or(chunk[0]));
            }

            let input = DualPcm {
                left: &left,
                right: &right,
            };
            let mut output: Vec<MaybeUninit<u8>> =
                vec![MaybeUninit::uninit(); mp3lame_encoder::max_required_buffer_size(frame_count)];
            let encoded = encoder
                .encode(input, &mut output)
                .map_err(|e| anyhow::anyhow!("Failed to encode stereo MP3: {:?}", e))?;
            // SAFETY: encode() initializes the first `encoded` bytes
            mp3_data.extend(output[..encoded].iter().map(|b| unsafe { b.assume_init() }));
        } else {
            bail!("MP3 encoding only supports mono or stereo audio");
        }

        // Flush encoder
        let mut output: Vec<MaybeUninit<u8>> =
            vec![MaybeUninit::uninit(); mp3lame_encoder::max_required_buffer_size(1024)];
        let flushed = encoder
            .flush::<FlushNoGap>(&mut output)
            .map_err(|e| anyhow::anyhow!("Failed to flush MP3 encoder: {:?}", e))?;
        // SAFETY: flush() initializes the first `flushed` bytes
        mp3_data.extend(output[..flushed].iter().map(|b| unsafe { b.assume_init() }));

        Ok(mp3_data)
    }
}

impl Default for AudioProcessor {
    fn default() -> Self {
        Self::new()
    }
}

struct AudioInfo {
    codec_is_vorbis: bool,
    sample_rate: Option<u32>,
    channels: Option<usize>,
}

/// Decoded audio data
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Interleaved PCM samples as f32 (-1.0 to 1.0)
    pub samples: Vec<f32>,
    /// Number of channels
    pub channels: usize,
    /// Sample rate in Hz
    pub sample_rate: u32,
}

/// Output audio formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Ogg,
    Wav,
    Mp3,
}

impl AudioFormat {
    /// The format's name as a manifest writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ogg => "ogg",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "ogg" => Some(Self::Ogg),
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use ttwright_testkit::{covers, ids};

    use super::*;

    /// A synthetic sine tone, `seconds` long, as a WAV file with `channels` channels.
    fn sine_wav(channels: u16, rate: u32, seconds: f32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let spec = WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::new(&mut out, spec).unwrap();
        for i in 0..(rate as f32 * seconds) as usize {
            let v =
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
        out.into_inner()
    }

    /// The (channels, kbit/s, sample rate) every MPEG-1 Layer III frame header in `mp3` claims; all frames must agree
    /// on channels and rate. Skips an ID3v2 tag if present.
    fn mp3_header(mp3: &[u8]) -> (usize, Vec<u32>, u32) {
        const KBPS: [u32; 15] = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ];
        const RATES: [u32; 3] = [44100, 48000, 32000];
        let mut pos = 0;
        let mut channels = None;
        let mut rates = Vec::new();
        let mut sample_rate = 0;
        while pos + 4 <= mp3.len() {
            let h = u32::from_be_bytes(mp3[pos..pos + 4].try_into().unwrap());
            let layer3_v1 = h >> 21 == 0x7ff && (h >> 19) & 3 == 3 && (h >> 17) & 3 == 1;
            let br = ((h >> 12) & 15) as usize;
            let sr = ((h >> 10) & 3) as usize;
            if !layer3_v1 || br == 0 || br == 15 || sr == 3 {
                pos += 1;
                continue;
            }
            let ch = if (h >> 6) & 3 == 3 { 1 } else { 2 };
            assert_eq!(*channels.get_or_insert(ch), ch, "channel count changed");
            sample_rate = RATES[sr];
            rates.push(KBPS[br]);
            pos += (144 * KBPS[br] * 1000 / RATES[sr] + ((h >> 9) & 1)) as usize;
        }
        (channels.expect("no MP3 frame found"), rates, sample_rate)
    }

    fn decode_mp3(mp3: &[u8]) -> DecodedAudio {
        AudioProcessor::new()
            .decode_bytes(mp3, Some("mp3"))
            .unwrap()
    }

    fn to_mp3(params: &str, wav: &[u8]) -> Result<Vec<u8>> {
        AudioProcessor::new()
            .with_params(params)?
            .process_audio_conversion(wav, Some("wav"), AudioFormat::Mp3)
    }

    #[test]
    fn a_requested_bit_rate_is_the_bit_rate_produced() {
        covers(&[
            ids::install::features::AUDIO_PARAM__B,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let wav = sine_wav(2, 44100, 2.0);
        for kbps in [128u32, 192, 256] {
            let mp3 = to_mp3(&format!("-b:{kbps}"), &wav).unwrap();
            let (_, rates, _) = mp3_header(&mp3);
            assert!(!rates.is_empty());
            assert!(
                rates.iter().all(|&r| r == kbps),
                "asked {kbps}, frame headers say {:?}",
                rates.iter().collect::<BTreeSet<_>>()
            );
        }
    }

    #[test]
    fn the_bit_rate_is_192_when_none_is_asked() {
        covers(&[ids::install::features::AUDIO_FORMAT_MP3]);
        let (_, rates, _) = mp3_header(&to_mp3("", &sine_wav(2, 44100, 1.0)).unwrap());
        assert!(rates.iter().all(|&r| r == 192));
    }

    #[test]
    fn a_mono_source_asked_for_two_channels_comes_out_stereo() {
        covers(&[
            ids::install::features::AUDIO_PARAM__C,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let mp3 = to_mp3("-c:2 -fmt:mp3 -b:192", &sine_wav(1, 44100, 1.0)).unwrap();
        assert_eq!(mp3_header(&mp3).0, 2);
        assert_eq!(decode_mp3(&mp3).channels, 2);
    }

    #[test]
    fn a_stereo_source_asked_for_one_channel_comes_out_mono() {
        covers(&[
            ids::install::features::AUDIO_PARAM__C,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let mp3 = to_mp3("-c:1", &sine_wav(2, 44100, 1.0)).unwrap();
        assert_eq!(mp3_header(&mp3).0, 1);
        assert_eq!(decode_mp3(&mp3).channels, 1);
    }

    /// Decoded length in seconds; asserts one channel and that the length is within encoder padding of `seconds`.
    fn assert_mono_length(mp3: &[u8], seconds: f32) {
        let decoded = decode_mp3(mp3);
        assert_eq!(decoded.channels, 1);
        let got = decoded.samples.len() as f32 / decoded.sample_rate as f32;
        // Two MPEG-1 frames (2 x 1152 samples) cover encoder delay and padding.
        assert!(
            (got - seconds).abs() < 2.0 * 1152.0 / 44100.0 + 0.01,
            "decoded {got} s for a {seconds} s source"
        );
    }

    #[test]
    fn a_mono_source_keeps_its_length_with_no_channel_parameter() {
        covers(&[ids::install::features::AUDIO_FORMAT_MP3]);
        let mp3 = to_mp3("", &sine_wav(1, 44100, 1.0)).unwrap();
        assert_eq!(mp3_header(&mp3).0, 1);
        assert_mono_length(&mp3, 1.0);
    }

    #[test]
    fn a_mono_source_keeps_its_length_with_one_channel_asked() {
        covers(&[
            ids::install::features::AUDIO_PARAM__C,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let mp3 = to_mp3("-c:1", &sine_wav(1, 44100, 1.0)).unwrap();
        assert_eq!(mp3_header(&mp3).0, 1);
        assert_mono_length(&mp3, 1.0);
    }

    #[test]
    fn a_stereo_source_downmixed_to_one_channel_keeps_its_length() {
        covers(&[
            ids::install::features::AUDIO_PARAM__C,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let mp3 = to_mp3("-c:1", &sine_wav(2, 44100, 1.0)).unwrap();
        assert_eq!(mp3_header(&mp3).0, 1);
        assert_mono_length(&mp3, 1.0);
    }

    #[test]
    fn a_stereo_source_keeps_its_length() {
        covers(&[ids::install::features::AUDIO_FORMAT_MP3]);
        let decoded = decode_mp3(&to_mp3("", &sine_wav(2, 44100, 1.0)).unwrap());
        assert_eq!(decoded.channels, 2);
        let got = decoded.samples.len() as f32 / 2.0 / decoded.sample_rate as f32;
        assert!((got - 1.0).abs() < 2.0 * 1152.0 / 44100.0 + 0.01, "{got}");
    }

    #[test]
    fn the_channel_count_is_kept_when_none_is_asked() {
        covers(&[ids::install::features::AUDIO_FORMAT_MP3]);
        assert_eq!(
            mp3_header(&to_mp3("", &sine_wav(1, 44100, 1.0)).unwrap()).0,
            1
        );
    }

    #[test]
    fn the_whole_ttw_radio_parameter_set_produces_stereo_192() {
        covers(&[
            ids::install::features::AUDIO_PARAM__C,
            ids::install::features::AUDIO_PARAM__B,
            ids::install::features::AUDIO_FORMAT_MP3,
        ]);
        let mp3 = to_mp3("-c:2 -fmt:mp3 -b:192", &sine_wav(1, 44100, 2.0)).unwrap();
        let (channels, rates, rate) = mp3_header(&mp3);
        assert_eq!((channels, rate), (2, 44100));
        assert!(rates.iter().all(|&r| r == 192));
        let decoded = decode_mp3(&mp3);
        assert_eq!((decoded.channels, decoded.sample_rate), (2, 44100));
        assert!(
            decoded.samples.iter().any(|s| s.abs() > 0.1),
            "the tone is audible"
        );
    }

    #[test]
    fn a_format_that_is_not_the_target_extension_is_an_error() {
        let wav = sine_wav(2, 44100, 0.5);
        let e = AudioProcessor::new()
            .with_params("-fmt:ogg")
            .unwrap()
            .process_audio_conversion(&wav, Some("wav"), AudioFormat::Mp3)
            .unwrap_err();
        assert!(e.to_string().contains("-fmt:ogg"), "{e}");
    }

    #[test]
    fn a_bit_rate_on_a_non_mp3_target_is_an_error() {
        let e = AudioProcessor::new()
            .with_params("-b:192")
            .unwrap()
            .process_audio_conversion(&sine_wav(2, 44100, 0.5), Some("wav"), AudioFormat::Wav)
            .unwrap_err();
        assert!(e.to_string().contains("MP3 bit rate"), "{e}");
    }

    #[test]
    fn unsupported_values_and_unknown_tokens_are_named() {
        for (params, token) in [
            ("-b:100", "-b:100"),
            ("-b:", "-b:"),
            ("-c:6", "-c:6"),
            ("-c:0", "-c:0"),
            ("-fmt:flac", "-fmt:flac"),
            ("-f:0", "-f:0"),
            ("-q:abc", "-q:abc"),
            ("-f:24000 -z:1", "-z:1"),
            ("loose", "loose"),
        ] {
            let e = AudioProcessor::new().with_params(params).err().unwrap();
            assert_eq!(e.token, token, "{params}");
        }
    }

    #[test]
    fn the_known_parameters_are_accepted_together() {
        assert!(AudioProcessor::new().with_params("-f:24000 -q:5").is_ok());
        assert!(
            AudioProcessor::new()
                .with_params("-c:2 -fmt:mp3 -b:192")
                .is_ok()
        );
        assert!(AudioProcessor::new().with_params("").is_ok());
    }

    #[test]
    fn channel_and_format_parameters_are_refused_by_the_ogg_resample() {
        for params in ["-c:2", "-fmt:ogg", "-b:128"] {
            let e = AudioProcessor::new()
                .with_params(params)
                .unwrap()
                .process_ogg_resample(&[])
                .unwrap_err();
            assert!(e.to_string().contains("only takes"), "{params}: {e}");
        }
    }
}
