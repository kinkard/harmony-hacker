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

/// White key dimensions
const WHITE_KEY_SIZE: Vec2 = Vec2 { x: 23.0, y: 135.0 };
/// Black key dimensions
const BLACK_KEY_SIZE: Vec2 = Vec2 { x: 14.0, y: 90.0 };
/// The space between white keys
const WHITE_KEYS_SPACE: f32 = 1.0;
/// Number of the white keys in the keyboard
const WHITE_KEYS_COUNT: usize = 52;
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
        .add_event::<UpdateSpectrum>()
        .add_systems(Startup, (setup, setup_piano_keys))
        .add_systems(Update, (file_drop, egui_ui, update_spectrum))
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
        .insert(Name::new("Keyboard"))
        .id();

    let white_key_shape = meshes.add(Rectangle::from_size(WHITE_KEY_SIZE));
    let white_key_material = materials.add(Color::WHITE);

    let mut key_pos = -KEYBOARD_SIZE.x / 2.0 + WHITE_KEY_SIZE.x / 2.0;
    let white_key_step = WHITE_KEY_SIZE.x + WHITE_KEYS_SPACE;
    for _ in 0..WHITE_KEYS_COUNT {
        commands
            .spawn(MaterialMesh2dBundle {
                mesh: white_key_shape.clone().into(),
                transform: Transform::from_translation(Vec3::new(key_pos, 0.0, 0.0)),
                material: white_key_material.clone(),
                ..default()
            })
            .set_parent(keyboard);
        key_pos += white_key_step;
    }

    // For black keys we split the octave (7 white keys) into 12 slots and fill them according to the mask
    // https://bootcamp.uxdesign.cc/drawing-a-flat-piano-keyboard-in-illustrator-de07c74a64c6
    let slot_size = white_key_step * 7.0 / 12.0;
    let mask = [
        false, true, false, true, false, false, true, false, true, false, true, false,
    ];
    let black_key_shape = meshes.add(Rectangle::from_size(BLACK_KEY_SIZE));
    let black_key_material = materials.add(Color::BLACK);

    // Octaves have a slight offset by 2 white keys where sub-contra octave lives,
    // which we achieve by offsetting the iteration over mask.
    let start_pos = 2.0 * white_key_step - KEYBOARD_SIZE.x / 2.0 + slot_size / 2.0;
    for i in -3..83 {
        if mask[(mask.len() as isize + i) as usize % mask.len()] {
            commands
                .spawn(MaterialMesh2dBundle {
                    mesh: black_key_shape.clone().into(),
                    transform: Transform::from_translation(Vec3::new(
                        start_pos + i as f32 * slot_size,
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

#[derive(Resource, Default)]
struct FftSource {
    name: String,
    sample_rate: u32,
    data: Vec<f32>,
}

#[derive(Resource)]
struct FftConfig {
    resolution_hz: f32,
    duration_sec: u32,
}

impl Default for FftConfig {
    fn default() -> Self {
        Self {
            resolution_hz: 50.0,
            duration_sec: 90,
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

                    let samples_to_take = fft_source.sample_rate * 120;
                    let mut data = Vec::with_capacity(samples_to_take as usize);

                    while let Some(audio_buf) = decoder.decode() {
                        let AudioBufferRef::F32(audio_buf) = audio_buf else {
                            // return Err(anyhow::anyhow!("Only f32 format is currently supported"));
                            error!("Only f32 format is currently supported");
                            return;
                        };

                        if data.len() + audio_buf.frames() as usize > data.capacity() {
                            break;
                        }

                        data.extend_from_slice(audio_buf.chan(0));
                    }
                    fft_source.data = data;

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

    egui::Window::new("FFT Config").show(contexts.ctx_mut(), |ui| {
        ui.label(format!("Source: {}", fft_source.name));
        ui.label("Resolution (Hz):");
        ui.add(egui::Slider::new(&mut fft_config.resolution_hz, 2.0..=50.0));
        ui.label("Duration (sec):");
        ui.add(egui::Slider::new(&mut fft_config.duration_sec, 1..=120));
    });

    if resolution_hz != fft_config.resolution_hz || duration_sec != fft_config.duration_sec {
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
            *handle = build_spectrum(&fft_source, &fft_config)
                .map(|image| images.add(image))
                .inspect_err(|err| error!("Failed to build spectrum: {:?}", err))
                .unwrap_or_default();
        }
    }
}

fn build_spectrum(source: &FftSource, config: &FftConfig) -> Result<Image> {
    let fft_window_size = (source.sample_rate as f32 / config.resolution_hz as f32) as usize;
    info!("FFT window size: {}", fft_window_size);

    let mut real_planner = RealFftPlanner::<f32>::new();
    let r2c = real_planner.plan_fft_forward(fft_window_size);
    // let mut input_buf = Vec::<f32>::with_capacity(fft_window_size);
    let mut input_buf = r2c.make_input_vec();
    let mut output_buf = r2c.make_output_vec();

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

        r2c.process(&mut input_buf, &mut output_buf).unwrap();
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
