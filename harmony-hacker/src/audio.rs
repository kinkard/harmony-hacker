use std::path::Path;

use anyhow::{Context, Result};
use lazy_static::lazy_static;
use symphonia::core::{
    audio::AudioBufferRef,
    codecs::{self, CodecRegistry},
    formats::FormatReader,
    io::MediaSourceStream,
    probe::Hint,
};
use tracing::warn;

lazy_static! {
    static ref CODEC_REGISTRY: CodecRegistry = {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<symphonia_opus::OpusDecoder>();
        registry
    };
}

pub(crate) struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn codecs::Decoder>,
    track_id: u32,
}

impl Decoder {
    pub(crate) fn new(path: &Path) -> Result<Self> {
        let src = std::fs::File::open(path).context("failed to open media")?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(ext);
        }

        // Probe the media source.
        let probe_data = symphonia::default::get_probe()
            .format(&hint, mss, &Default::default(), &Default::default())
            .context("unsupported format")?;
        let format = probe_data.format;

        // Find a compatible track to decode. Try the default track first and then all other tracks
        let (decoder, track_id) = std::iter::once(format.default_track())
            .flatten()
            .chain(format.tracks().iter())
            .find_map(|track| {
                CODEC_REGISTRY
                    .make(&track.codec_params, &Default::default())
                    .ok()
                    .map(|d| (d, track.id))
            })
            .ok_or(anyhow::anyhow!("no compatible track found"))?;

        Ok(Self {
            format,
            decoder,
            track_id,
        })
    }

    pub(crate) fn decode(&mut self) -> Option<AudioBufferRef> {
        loop {
            let Ok(packet) = self.format.next_packet() else {
                break None;
            };

            // If the packet does not belong to the selected track, skip it
            if packet.track_id() != self.track_id {
                continue;
            }

            // Let's try to decode the next one
            if let Err(err) = self.decoder.decode(&packet) {
                warn!("Skipping packet because of decode error: {err:?}");
                continue;
            }

            break Some(self.decoder.last_decoded());
        }
    }
}
