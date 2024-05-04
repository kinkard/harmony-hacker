use anyhow::Result;
use bevy::{
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    },
    sprite::MaterialMesh2dBundle,
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use realfft::RealFftPlanner;
use symphonia::core::audio::{AudioBufferRef, Signal};

mod audio;
mod goertzel;

/// White key dimensions
const WHITE_KEY_SIZE: Vec2 = Vec2 { x: 23.0, y: 135.0 };
/// The space between white keys
const WHITE_KEYS_SPACE: f32 = 1.0;
/// The distance between two white keys centers
const WHITE_KEYS_STEP: f32 = WHITE_KEY_SIZE.x + WHITE_KEYS_SPACE;
/// Number of the white keys in the keyboard
const WHITE_KEYS_COUNT: usize = 52;

// 12 keys fit the octave, 7 white and 5 black
const BLACK_KEYS_SLOT_SIZE: f32 = WHITE_KEYS_STEP * 7.0 / 12.0;
/// Black key dimensions
const BLACK_KEY_SIZE: Vec2 = Vec2 {
    x: BLACK_KEYS_SLOT_SIZE,
    y: 90.0,
};

/// The size of the keyboard
const KEYBOARD_SIZE: Vec2 = Vec2 {
    x: WHITE_KEY_SIZE.x + (WHITE_KEYS_COUNT - 1) as f32 * (WHITE_KEY_SIZE.x + WHITE_KEYS_SPACE),
    y: WHITE_KEY_SIZE.y,
};

/// The frequency of the highest note in the piano, C8
const MAX_FREQ: f32 = 4186.01;
/// The frequency of the lowest note in the piano, A0
const _MIN_FREQ: f32 = 27.5000;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin)
        .init_resource::<FftSource>()
        .init_resource::<FftConfig>()
        .add_event::<PlayNote>()
        .add_event::<UpdateSpectrum>()
        .add_systems(Startup, (setup, setup_piano_keys))
        .add_systems(
            Update,
            (
                file_drop,
                egui_ui,
                update_spectrum,
                piano_keyboard,
                play_note,
            ),
        )
        .run();
}

#[derive(Component)]
struct Spectrum;

fn setup(mut commands: Commands, windows: Query<&Window, With<PrimaryWindow>>) {
    commands.spawn(Camera2dBundle::default());

    let window_height = windows.single().height();
    commands
        .spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(
                0.0,
                0.5 + KEYBOARD_SIZE.y / 2.0,
                0.0,
            )),
            sprite: Sprite {
                flip_y: true,
                // todo: fix the size of the spectrum
                custom_size: Some(Vec2::new(
                    KEYBOARD_SIZE.x,
                    window_height - KEYBOARD_SIZE.y - 1.0,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
        .insert(Spectrum);
}

#[derive(Component)]
struct Keyboard;

fn setup_piano_keys(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window_height = windows.single().height();
    let keyboard = commands
        .spawn(SpatialBundle::from_transform(Transform::from_translation(
            Vec3::new(
                0.0,
                // set the keyboard to the bottom of the screen
                // todo: update position on window resize
                -window_height / 2.0 + KEYBOARD_SIZE.y / 2.0,
                0.0,
            ),
        )))
        .insert(Keyboard)
        .insert(Name::new("Keyboard"))
        .id();

    let white_key_shape = meshes.add(Rectangle::from_size(WHITE_KEY_SIZE));
    let white_key_material = materials.add(Color::WHITE);

    let mut key_pos = -KEYBOARD_SIZE.x / 2.0 + WHITE_KEY_SIZE.x / 2.0;
    for _ in 0..WHITE_KEYS_COUNT {
        commands
            .spawn(MaterialMesh2dBundle {
                mesh: white_key_shape.clone().into(),
                transform: Transform::from_translation(Vec3::new(key_pos, 0.0, 0.0)),
                material: white_key_material.clone(),
                ..default()
            })
            .set_parent(keyboard);
        key_pos += WHITE_KEYS_STEP;
    }

    // For black keys we split the octave (7 white keys) into 12 slots and fill them according to the mask
    // https://bootcamp.uxdesign.cc/drawing-a-flat-piano-keyboard-in-illustrator-de07c74a64c6
    let octave_mask_black = [
        false, true, false, true, false, false, true, false, true, false, true, false,
    ];
    let black_key_shape = meshes.add(Rectangle::from_size(BLACK_KEY_SIZE));
    let black_key_material = materials.add(Color::BLACK);

    // Octaves have a slight offset by 2 white keys where sub-contra octave lives,
    // which we achieve by offsetting the iteration over mask.
    let start_pos = 2.0 * WHITE_KEYS_STEP - KEYBOARD_SIZE.x / 2.0 + BLACK_KEYS_SLOT_SIZE / 2.0;
    for i in -3..83 {
        let octave_key = (octave_mask_black.len() as isize + i) as usize % octave_mask_black.len();
        if octave_mask_black[octave_key] {
            commands
                .spawn(MaterialMesh2dBundle {
                    mesh: black_key_shape.clone().into(),
                    transform: Transform::from_translation(Vec3::new(
                        start_pos + i as f32 * BLACK_KEYS_SLOT_SIZE,
                        // the offset to align keys by the top side
                        (WHITE_KEY_SIZE.y - BLACK_KEY_SIZE.y) / 2.0,
                        // Draw black keys on top of the white
                        1.0,
                    )),
                    material: black_key_material.clone(),
                    ..default()
                })
                .set_parent(keyboard);
        }
    }
}

/// Resolve a position on the keyboard (in keyboard coordinates) to a key number 0..88
fn keyboard_pos_to_key(pos: Vec2) -> Option<u8> {
    if pos.x.abs() > KEYBOARD_SIZE.x / 2.0 || pos.y.abs() > KEYBOARD_SIZE.y / 2.0 {
        return None;
    }

    // The keyboard starts from A0 key in sub-contra octave. Each octave has 7 white keys and 5 black keys.
    // In lower part of the keyboard only white keys, whilte in the upper part we have white and black keys.
    let white_and_black = pos.y + KEYBOARD_SIZE.y / 2.0 > WHITE_KEY_SIZE.y - BLACK_KEY_SIZE.y;
    // For simplicity we offset the position to the imaginary beginning of the sub-contra octave and then find the key
    let pos = pos.x + KEYBOARD_SIZE.x / 2.0 + 5.0 * WHITE_KEYS_STEP;

    // find the octave and don't forget about the offset by 2 white keys
    let octave = (pos / (7.0 * WHITE_KEYS_STEP)) as u8;
    let pos_in_octave = pos - octave as f32 * 7.0 * WHITE_KEYS_STEP;

    // then find a key in the octave
    let key_in_octave = if white_and_black {
        (pos_in_octave / BLACK_KEYS_SLOT_SIZE) as u8
    } else {
        let white_key_idx = (pos_in_octave / WHITE_KEYS_STEP) as usize;
        let white_keys_map = [0, 2, 4, 5, 7, 9, 11];
        white_keys_map[white_key_idx]
    };
    // Key was counted with the offset and real piano keyboard misses leading and trailing black keys
    let key = (key_in_octave + octave * 12).clamp(9, 96) - 9;
    Some(key)
}

#[derive(Event)]
struct PlayNote {
    key: u8,
}

fn piano_keyboard(
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    keyboard: Query<&Transform, With<Keyboard>>,
    mut ev_play_note: EventWriter<PlayNote>,
) {
    if mouse_button_input.just_pressed(MouseButton::Left) {
        let window = windows.single();
        if let Some(cursor_pos) = window.cursor_position() {
            // Transform from (0,0) in top right corner to the world coordinates with (0,0) in the center
            let cursor_pos = Vec2::new(
                cursor_pos.x - window.width() / 2.0,
                window.height() / 2.0 - cursor_pos.y,
            );

            // Check if the cursor is in the keyboard
            for transform in keyboard.iter() {
                let cursor_pos = cursor_pos - transform.translation.xy();
                if let Some(key) = keyboard_pos_to_key(cursor_pos) {
                    ev_play_note.send(PlayNote { key });
                }
            }

            // todo: update frequency on key press
        }
    }
}

fn play_note(
    mut ev_play_note: EventReader<PlayNote>,
    mut fft_source: ResMut<FftSource>,
    mut ev_update_spectrum: EventWriter<UpdateSpectrum>,
) {
    for ev in ev_play_note.read() {
        // The key number 49 (48 with zero-based index) is the A4 key with 440 Hz frequency
        let freq = 440.0 * 2.0f64.powf((ev.key as f64 - 48.0) / 12.0);
        info!("Playing note: {} with frequency: {}", ev.key, freq);

        fft_source.name = format!("Note: {freq:.2} Hz");
        fft_source.sample_rate = 48000;

        // Reuse the buffer for the new data
        let samples_to_take = fft_source.sample_rate as usize * 120;
        fft_source.data.resize(samples_to_take, 0.0);
        for (i, sample) in fft_source.data.iter_mut().enumerate() {
            *sample = (i as f64 * freq * 2.0 * std::f64::consts::PI / 48000.0).sin() as f32;
        }

        ev_update_spectrum.send(UpdateSpectrum);
    }
}

#[derive(Resource)]
struct FftSource {
    name: String,
    sample_rate: u32,
    data: Vec<f32>,
}

impl Default for FftSource {
    fn default() -> Self {
        Self {
            name: Default::default(),
            sample_rate: 48000,
            data: Vec::with_capacity(48000 * 120),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Algorithm {
    Fft,
    Goertzel,
}

#[derive(Resource)]
struct FftConfig {
    resolution_hz: f32,
    duration_sec: u32,
    algorithm: Algorithm,
}

impl Default for FftConfig {
    fn default() -> Self {
        Self {
            resolution_hz: 50.0,
            duration_sec: 90,
            algorithm: Algorithm::Fft,
        }
    }
}

#[derive(Event)]
struct UpdateSpectrum;

fn file_drop(
    mut dnd_evr: EventReader<FileDragAndDrop>,
    mut fft_source: ResMut<FftSource>,
    mut ev_update_spectrum: EventWriter<UpdateSpectrum>,
) {
    for ev in dnd_evr.read() {
        if let FileDragAndDrop::DroppedFile {
            window: _,
            path_buf,
        } = ev
        {
            match audio::Decoder::new(path_buf) {
                Ok(mut decoder) => {
                    fft_source.name = path_buf
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned();

                    fft_source.sample_rate = decoder.sample_rate();

                    // take first 2m of the audio
                    let samples_to_take = fft_source.sample_rate as usize * 120;
                    fft_source.data.clear();
                    fft_source.data.reserve(samples_to_take);

                    while let Some(audio_buf) = decoder.decode() {
                        let AudioBufferRef::F32(audio_buf) = audio_buf else {
                            // return Err(anyhow::anyhow!("Only f32 format is currently supported"));
                            error!("Only f32 format is currently supported");
                            return;
                        };

                        if fft_source.data.len() + audio_buf.frames() as usize > samples_to_take {
                            break;
                        }

                        fft_source.data.extend_from_slice(audio_buf.chan(0));
                    }

                    ev_update_spectrum.send(UpdateSpectrum);
                }
                Err(err) => {
                    error!("Failed to open the file {path_buf:?}: {err:?}");
                }
            }
        }
    }
}

fn egui_ui(
    mut contexts: EguiContexts,
    mut fft_config: ResMut<FftConfig>,
    fft_source: Res<FftSource>,
    mut ev_update_spectrum: EventWriter<UpdateSpectrum>,
) {
    let resolution_hz = fft_config.resolution_hz;
    let duration_sec = fft_config.duration_sec;
    let algorithm = fft_config.algorithm;

    egui::Window::new("FFT Config").show(contexts.ctx_mut(), |ui| {
        ui.label(format!("Source: {}", fft_source.name));
        ui.label("Resolution (Hz):");
        ui.add(egui::Slider::new(&mut fft_config.resolution_hz, 1.0..=50.0));
        ui.label("Duration (sec):");
        ui.add(egui::Slider::new(&mut fft_config.duration_sec, 1..=120));
        ui.label("Algorithm:");
        ui.radio_value(&mut fft_config.algorithm, Algorithm::Fft, "FFT");
        ui.radio_value(&mut fft_config.algorithm, Algorithm::Goertzel, "Goertzel");
    });

    if resolution_hz != fft_config.resolution_hz
        || duration_sec != fft_config.duration_sec
        || algorithm != fft_config.algorithm
    {
        ev_update_spectrum.send(UpdateSpectrum);
    }
}

fn update_spectrum(
    mut ev_update_spectrum: EventReader<UpdateSpectrum>,
    fft_source: Res<FftSource>,
    fft_config: Res<FftConfig>,
    mut images: ResMut<Assets<Image>>,
    mut spectrum_spties: Query<&mut Handle<Image>, With<Spectrum>>,
) {
    for _ in ev_update_spectrum.read() {
        for mut handle in spectrum_spties.iter_mut() {
            *handle = match fft_config.algorithm {
                Algorithm::Fft => build_spectrum_fft(&fft_source, &fft_config),
                Algorithm::Goertzel => build_spectrum_goertzel(&fft_source, &fft_config),
            }
            .map(|image| images.add(image))
            .inspect_err(|err| error!("Failed to build spectrum: {:?}", err))
            .unwrap_or_default();
        }
    }
}

fn build_spectrum_fft(source: &FftSource, config: &FftConfig) -> Result<Image> {
    let fft_window_size = (source.sample_rate as f32 / config.resolution_hz as f32) as usize;
    info!("FFT window size: {}", fft_window_size);

    let mut real_planner = RealFftPlanner::<f32>::new();
    let r2c = real_planner.plan_fft_forward(fft_window_size);
    // let mut input_buf = Vec::<f32>::with_capacity(fft_window_size);
    let mut input_buf = r2c.make_input_vec();
    let mut output_buf = r2c.make_output_vec();
    let mut scratch_buf = r2c.make_scratch_vec();

    // image related stuff
    let bins_to_take = 1 + (MAX_FREQ / source.sample_rate as f32 * fft_window_size as f32) as u32;
    let spectrum_rows = source.sample_rate * config.duration_sec / fft_window_size as u32;
    let size = Extent3d {
        width: bins_to_take,
        height: spectrum_rows,
        ..default()
    };
    let mut image = Image {
        data: Vec::with_capacity(size.width as usize * size.height as usize),
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        ..default()
    };

    for row in 0..spectrum_rows as usize {
        let start = row * fft_window_size;
        if start + fft_window_size > source.data.len() {
            break;
        }
        input_buf.copy_from_slice(&source.data[start..start + fft_window_size]);

        r2c.process_with_scratch(&mut input_buf, &mut output_buf, &mut scratch_buf)
            .unwrap();
        for value in output_buf.iter().take(bins_to_take as usize) {
            let s = value.norm();
            let s = s.max(1e-10); // Avoid taking the logarithm of zero
            let s = (s.log10() / 3.0).min(1.0); // convert to 0..60db range in 0..1
            let s = (s * 255.0) as u8;
            image.data.push(s);
        }
    }

    // Fill the rest of the image with zeros
    image.data.resize(image.data.capacity(), 0);

    Ok(image)
}

#[inline(never)]
fn build_spectrum_goertzel(source: &FftSource, config: &FftConfig) -> Result<Image> {
    let window_size = (source.sample_rate as f32 / config.resolution_hz as f32) as usize;
    info!("FFT window size: {}", window_size);

    // image related stuff
    let spectrum_rows = source.sample_rate * config.duration_sec / window_size as u32;
    let size = Extent3d {
        width: 88 * 3 + 5,
        height: spectrum_rows,
        ..default()
    };
    let mut image = Image {
        data: Vec::with_capacity(size.width as usize * size.height as usize),
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        ..default()
    };

    let mut key_states = (0..88)
        .map(|key| 440.0 * 2.0f64.powf((key as f64 - 48.0) / 12.0) as f32)
        .map(|frequency| goertzel::Goertzel::new(source.sample_rate, frequency))
        .collect::<Vec<_>>();
    for chunk in source.data.chunks(window_size).take(spectrum_rows as usize) {
        for sample in chunk {
            for state in key_states.iter_mut() {
                state.process(*sample)
            }
        }

        image.data.push(0);
        image.data.push(0);
        for state in key_states.iter_mut() {
            let s = state.magnitude(window_size as u32);
            let s = (s.sqrt() * 255.0) as u8;
            for _ in 0..3 {
                image.data.push(s);
            }
            state.reset();
        }
        image.data.push(0);
        image.data.push(0);
        image.data.push(0);
    }

    // Fill the rest of the image with zeros
    image.data.resize(image.data.capacity(), 0);

    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn keyboard_pos_to_key_test() {
        // Outside the keyboard
        assert_eq!(
            keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0 - Vec2::new(0.1, 0.0)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0 - Vec2::new(0.0, 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0 - Vec2::new(0.1, 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(KEYBOARD_SIZE / 2.0 + Vec2::new(0.1, 0.0)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(KEYBOARD_SIZE / 2.0 + Vec2::new(0.0, 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(KEYBOARD_SIZE / 2.0 + Vec2::new(0.1, 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(Vec2::new(0.0, -KEYBOARD_SIZE.y / 2.0 - 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(Vec2::new(0.0, KEYBOARD_SIZE.y / 2.0 + 0.1)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(Vec2::new(KEYBOARD_SIZE.x + 0.1, 0.0)),
            None
        );
        assert_eq!(
            keyboard_pos_to_key(Vec2::new(-KEYBOARD_SIZE.x - 0.1, 0.0)),
            None
        );

        // The first key
        assert_eq!(keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0), Some(0));
        assert_eq!(
            keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0 + Vec2::new(0.0, KEYBOARD_SIZE.y / 2.0)),
            Some(0)
        );
        assert_eq!(
            keyboard_pos_to_key(-KEYBOARD_SIZE / 2.0 + Vec2::new(0.0, KEYBOARD_SIZE.y)),
            Some(0)
        );

        // The last key
        assert_eq!(keyboard_pos_to_key(KEYBOARD_SIZE / 2.0), Some(87));
        assert_eq!(
            keyboard_pos_to_key(KEYBOARD_SIZE / 2.0 - Vec2::new(0.0, KEYBOARD_SIZE.y / 2.0)),
            Some(87)
        );
        assert_eq!(
            keyboard_pos_to_key(KEYBOARD_SIZE / 2.0 - Vec2::new(0.0, KEYBOARD_SIZE.y)),
            Some(87)
        );

        // iterate over white keys jumping by octave at the bottom of the key
        let white_key_step = WHITE_KEY_SIZE.x + WHITE_KEYS_SPACE;
        let mut pos = -KEYBOARD_SIZE / 2.0 + Vec2::new(WHITE_KEY_SIZE.x / 2.0, 0.5);
        for i in 0..8 {
            assert_eq!(keyboard_pos_to_key(pos), Some(i * 12));
            pos.x += 7.0 * white_key_step;
        }

        // iterate over white keys jumping by octave at the middle of the D, G and A keys
        let mut pos = Vec2::new(
            -KEYBOARD_SIZE.x / 2.0 + WHITE_KEY_SIZE.x / 2.0 + white_key_step * 3.0,
            0.0,
        );
        for i in 0..7 {
            assert_eq!(keyboard_pos_to_key(pos), Some(5 + i * 12));
            assert_eq!(
                keyboard_pos_to_key(pos + Vec2::new(white_key_step * 3.0, 0.0)),
                Some(10 + i * 12)
            );
            assert_eq!(
                keyboard_pos_to_key(pos + Vec2::new(white_key_step * 4.0, 0.0)),
                Some(12 + i * 12)
            );
            pos.x += 7.0 * white_key_step;
        }

        // iterate over all keys, black and white starting from the 1st octave
        let slot_size = white_key_step * 7.0 / 12.0;
        let mut pos = Vec2::new(
            -KEYBOARD_SIZE.x / 2.0 + white_key_step * 2.0 + slot_size / 2.0,
            0.0,
        );
        for i in 0..85 {
            assert_eq!(keyboard_pos_to_key(pos), Some(3 + i));
            pos.x += slot_size;
        }
    }
}
