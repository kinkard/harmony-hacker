use std::path::Path;

use anyhow::Result;
use realfft::RealFftPlanner;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, DecoderOptions};
use symphonia::core::errors::Error;
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
    let mut format = probe_data.format;

    let codecs = {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<symphonia_opus::OpusDecoder>();
        registry
    };

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
    let (mut decoder, track_id) = decoder.ok_or(anyhow::anyhow!("no compatible track found"))?;

    let codec = decoder.codec_params();
    println!("Codec: {codec:?}");

    let mut sample_count = 0;
    let mut sample_buf = None;

    // FFT related stuff
    let mut real_planner = RealFftPlanner::<f32>::new();
    let r2c = real_planner.plan_fft_forward(960);
    let mut spectrum = r2c.make_output_vec();

    let mut spectrum_image = None;
    let max_width = 12000;
    let mut curr_width = 0;

    let max_magnitude: f32 = 2.5;

    loop {
        let Ok(packet) = format.next_packet() else {
            println!("EOF");
            break;
        };

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples, ignoring any decode errors.
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                // If this is the *first* decoded packet, create a sample buffer matching the
                // decoded audio buffer format.
                if sample_buf.is_none() {
                    let spec = *audio_buf.spec();
                    let duration = audio_buf.capacity() as u64;

                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));

                    // also init image
                    spectrum_image = Some(image::ImageBuffer::<image::Rgb<u8>, Vec<_>>::new(
                        max_width,
                        audio_buf.frames() as u32 / 2 + 1,
                    ));
                }

                // Copy the decoded audio buffer into the sample buffer in an interleaved format.
                if let Some(buf) = &mut sample_buf {
                    // audio_buf.spec().channels.count();
                    let frames = audio_buf.frames();
                    let _channels = audio_buf.spec().channels.count();
                    let rate = audio_buf.spec().rate;
                    let _duration = frames as f64 / rate as f64;
                    // actually this is redundant as audio_buf is already in f32 planar format
                    buf.copy_planar_ref(audio_buf);
                    let _samples = buf.samples().len();

                    // The samples may now be access via the `samples()` function.
                    sample_count += buf.samples().len();

                    if curr_width < max_width {
                        r2c.process(&mut buf.samples_mut()[..frames], &mut spectrum)
                            .unwrap();

                        for (pos, value) in spectrum.iter().enumerate() {
                            let s = value.norm();
                            let s = s.max(1e-10); // Avoid taking the logarithm of zero
                            let s = s.log10(); // Take the logarithm
                            let s = (s / max_magnitude * 255.0) as u8;
                            let pixel = image::Rgb([s, s, s]);
                            let h = spectrum_image.as_mut().unwrap().height();
                            spectrum_image.as_mut().unwrap().put_pixel(
                                curr_width,
                                h - pos as u32 - 1,
                                pixel,
                            );
                        }
                        curr_width += 1;
                    }
                }
            }
            Err(Error::DecodeError(_)) => (),
            Err(_) => break,
        }
    }
    println!("Decoded {} samples", sample_count);

    spectrum_image.unwrap().save("spectrum.png")?;

    Ok(())
}
