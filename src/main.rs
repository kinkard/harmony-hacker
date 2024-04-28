use std::path::Path;

use anyhow::Result;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

fn main() -> Result<()> {
    // Open provided media file
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("file path not provided");
    let path = Path::new(path);

    let src = std::fs::File::open(path).expect("failed to open media");
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the media source.
    let probe_data = symphonia::default::get_probe()
        .format(&hint, mss, &Default::default(), &Default::default())
        .expect("unsupported format");
    let format = probe_data.format;

    let codecs = symphonia::default::get_codecs();

    // if default track exists, try to make a decoder
    // if that fails, linear scan and take first that succeeds
    let decoder = format
        .default_track()
        .and_then(|track| {
            codecs
                .make(&track.codec_params, &DecoderOptions::default())
                .ok()
                .map(|d| (d, track.id))
        })
        .or_else(|| {
            format.tracks().iter().find_map(|track| {
                codecs
                    .make(&track.codec_params, &DecoderOptions::default())
                    .ok()
                    .map(|d| (d, track.id))
            })
        });

    // No tracks is a playout error, a bad default track is also possible.
    // These are probably malformed? We could go best-effort, and fall back to tracks[0]
    // but drop such tracks for now.
    let (decoder, track_id) = decoder.ok_or(anyhow::anyhow!("no compatible track found"))?;

    Ok(())
}
