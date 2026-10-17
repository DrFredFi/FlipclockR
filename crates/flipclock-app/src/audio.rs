//! Tick playback. Ticks are decoded once into memory and mixed, so overlapping
//! ticks from a cascade or a roll all play.

use std::num::NonZero;
use std::path::Path;

use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, MixerDeviceSink, Source};

/// Longest custom tick kept, in seconds.
const MAX_TICK_SECONDS: usize = 2;

pub struct Audio {
    sink: Option<MixerDeviceSink>,
    tick: SamplesBuffer,
    custom_path: Option<String>,
}

impl Audio {
    /// Open the default output device; without one, sound is silently disabled.
    pub fn new() -> Self {
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .inspect_err(|e| log::warn!("no audio output: {e}"))
            .ok()
            .map(|mut s| {
                s.log_on_drop(false);
                s
            });
        Self {
            sink,
            tick: default_tick(),
            custom_path: None,
        }
    }

    /// Use the `.mp3` at `path`, or the bundled tick when `None` or unreadable.
    pub fn set_custom_tick(&mut self, path: Option<&str>) {
        if self.custom_path.as_deref() == path {
            return;
        }
        self.custom_path = path.map(str::to_owned);
        self.tick = path
            .and_then(|p| load_tick(Path::new(p)))
            .unwrap_or_else(default_tick);
    }

    pub fn play_ticks(&self, count: u32, volume: f32) {
        let Some(sink) = &self.sink else { return };
        for _ in 0..count {
            sink.mixer()
                .add(self.tick.clone().amplify(volume.clamp(0.0, 1.0)));
        }
    }
}

fn load_tick(path: &Path) -> Option<SamplesBuffer> {
    let file = std::fs::File::open(path)
        .inspect_err(|e| log::warn!("cannot open tick sound {}: {e}", path.display()))
        .ok()?;
    let decoder = Decoder::try_from(file)
        .inspect_err(|e| log::warn!("cannot decode tick sound {}: {e}", path.display()))
        .ok()?;
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    let max = MAX_TICK_SECONDS * rate.get() as usize * channels.get() as usize;
    let samples: Vec<f32> = decoder.take(max).collect();
    (!samples.is_empty()).then(|| SamplesBuffer::new(channels, rate, samples))
}

/// A short mechanical click: a noise burst through two decaying resonances.
fn default_tick() -> SamplesBuffer {
    const RATE: u32 = 48_000;
    let len = (RATE as f32 * 0.05) as usize;
    let mut seed: u32 = 0x1234_5678;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let samples: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let click = noise() * (-t * 900.0).exp();
            let body = (t * 2.0 * std::f32::consts::PI * 1850.0).sin() * (-t * 160.0).exp()
                + 0.6 * (t * 2.0 * std::f32::consts::PI * 3400.0).sin() * (-t * 260.0).exp();
            (0.55 * click + 0.45 * body) * 0.8
        })
        .collect();
    SamplesBuffer::new(
        NonZero::new(1).unwrap(),
        NonZero::new(RATE).unwrap(),
        samples,
    )
}
