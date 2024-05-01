use std::path::Path;

use anyhow::Result;
use bevy::{
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    },
    sprite::MaterialMesh2dBundle,
    window::PrimaryWindow,
};
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
        .add_systems(Startup, (setup, setup_piano_keys))
        .add_systems(Update, file_drop)
        .run();
}

#[derive(Component)]
struct Spectrum;

fn setup(mut commands: Commands, windows: Query<&Window, With<PrimaryWindow>>) {
    commands.spawn(Camera2dBundle::default());

    let window_height = windows.single().height();
    commands
        .spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(0.0, 1.0 + KEYBOARD_SIZE.y, 0.0)),
            sprite: Sprite {
                flip_y: true,
                // todo: fix the size of the spectrum
                custom_size: Some(Vec2::new(KEYBOARD_SIZE.x, window_height)),
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

    let mut key_pos = -KEYBOARD_SIZE.x / 2.0 + WHITE_KEY_SIZE.x / 2.0;
    let white_key_step = WHITE_KEY_SIZE.x + WHITE_KEYS_SPACE;
    for _ in 0..WHITE_KEYS_COUNT {
        commands
            .spawn(MaterialMesh2dBundle {
                mesh: white_key_shape.clone().into(),
                transform: Transform::from_translation(Vec3::new(key_pos, 0.0, 0.0)),
                material: materials.add(Color::WHITE),
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
                    material: materials.add(Color::BLACK),
                    ..default()
                })
                .set_parent(keyboard);
        }
    }
}

fn file_drop(
    mut dnd_evr: EventReader<FileDragAndDrop>,
    mut spectrum_spties: Query<&mut Handle<Image>, With<Spectrum>>,
    mut images: ResMut<Assets<Image>>,
) {
    for ev in dnd_evr.read() {
        if let FileDragAndDrop::DroppedFile {
            window: _,
            path_buf,
        } = ev
        {
            // todo: move it into a background task
            let spectrum = build_spectrum(&path_buf).unwrap();
            *spectrum_spties.single_mut() = images.add(spectrum);
        }
    }
}

fn build_spectrum(path: &Path) -> Result<Image> {
    let mut decoder = audio::Decoder::new(path)?;

    let desired_resolution_hz = 5;
    let sample_rate = decoder.sample_rate();
    let number_of_samples = sample_rate as usize / desired_resolution_hz;

    let mut real_planner = RealFftPlanner::<f32>::new();
    let r2c = real_planner.plan_fft_forward(number_of_samples);
    let mut input_buf = Vec::<f32>::with_capacity(number_of_samples);
    let mut output_buf = r2c.make_output_vec();

    // image related stuff
    let bins_to_take = 1 + (MAX_FREQ / sample_rate as f32 * number_of_samples as f32) as u32;
    // todo: deduce from the duration of the audio file
    let spectrum_rows = 500;
    let size = Extent3d {
        // width: audio_buf.frames() as u32 / 2 + 1,
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

    // progress related stuff
    let mut curr_row_idx = 0;
    let max_magnitude: f32 = 2.5;

    while let Some(audio_buf) = decoder.decode() {
        let AudioBufferRef::F32(audio_buf) = audio_buf else {
            return Err(anyhow::anyhow!("Only f32 format is currently supported"));
        };

        let input_buf_space = input_buf.capacity() - input_buf.len();
        if audio_buf.frames() < input_buf_space {
            input_buf.extend_from_slice(audio_buf.chan(0));
        } else {
            input_buf.extend_from_slice(&audio_buf.chan(0)[..input_buf_space]);

            r2c.process(&mut input_buf, &mut output_buf).unwrap();
            for value in output_buf.iter().take(bins_to_take as usize) {
                let s = value.norm();
                let s = s.max(1e-10); // Avoid taking the logarithm of zero
                let s = s.log10(); // Take the logarithm
                let s = (s / max_magnitude * 255.0) as u8;

                image.data.push(s);
            }

            curr_row_idx += 1;
            // no need to process more
            if curr_row_idx == spectrum_rows {
                break;
            }
            // And now take the rest of the audio_buf
            input_buf.clear();
            input_buf.extend_from_slice(&audio_buf.chan(0)[input_buf_space..]);
        }
    }

    Ok(image)
}
