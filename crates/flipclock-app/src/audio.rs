//! Tick playback. Ticks are decoded once into memory and mixed, so overlapping
//! ticks from a cascade or a roll all play. The output follows the default
//! device: when it changes (headphones plugged in, Bluetooth connected) or the
//! stream fails, it is reopened before the next sound.

use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rodio::buffer::SamplesBuffer;
use rodio::cpal::DeviceId;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

/// Longest custom tick kept, in seconds.
const MAX_TICK_SECONDS: usize = 2;
/// How often the default output device is compared with the open one.
const DEVICE_CHECK: Duration = Duration::from_secs(2);

struct Output {
    sink: MixerDeviceSink,
    device: Option<DeviceId>,
}

pub struct Audio {
    output: Option<Output>,
    /// Set by the stream's error callback; the stream is reopened before the next sound.
    failed: Arc<AtomicBool>,
    checked: Option<Instant>,
    tick: SamplesBuffer,
    custom_path: Option<String>,
}

impl Audio {
    /// Open the default output device; without one, sound is skipped until one appears.
    pub fn new() -> Self {
        let mut audio = Self {
            output: None,
            failed: Arc::new(AtomicBool::new(false)),
            checked: None,
            tick: default_tick(),
            custom_path: None,
        };
        audio.ensure_output();
        audio
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

    pub fn play_ticks(&mut self, count: u32, volume: f32) {
        self.ensure_output();
        let Some(output) = &self.output else { return };
        for _ in 0..count {
            output
                .sink
                .mixer()
                .add(self.tick.clone().amplify(volume.clamp(0.0, 1.0)));
        }
    }

    /// (Re)open the stream if it failed or the default device changed since the last check.
    fn ensure_output(&mut self) {
        let failed = self.failed.swap(false, Ordering::Relaxed);
        let now = Instant::now();
        if !failed && self.checked.is_some_and(|at| now - at < DEVICE_CHECK) {
            return;
        }
        self.checked = Some(now);
        let device = rodio::cpal::default_host().default_output_device();
        let id = device.as_ref().and_then(|d| d.id().ok());
        if !failed && id.is_some() && self.output.as_ref().is_some_and(|o| o.device == id) {
            return;
        }
        // Drop the old stream before opening a new one on the same device.
        let had_output = self.output.take().is_some();
        let Some(device) = device else {
            if had_output {
                log::info!("audio output gone");
            }
            return;
        };
        let flag = self.failed.clone();
        let sink = DeviceSinkBuilder::from_device(device)
            .map(|b| {
                b.with_error_callback(move |e| {
                    log::warn!("audio stream error: {e}");
                    flag.store(true, Ordering::Relaxed);
                })
            })
            .and_then(|b| b.open_stream());
        match sink {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                log::info!("audio output opened: {id:?}");
                self.output = Some(Output { sink, device: id });
            }
            Err(e) => log::warn!("no audio output: {e}"),
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
