//! Battery status, polled on a background thread. Changes reach the event loop
//! as `UserEvent::Battery`; computers without a battery report `None`.

use std::time::Duration;

use flipclock_core::battery::BatteryStatus;
use starship_battery::{Manager, State};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// Short enough that plugging in shows within a few seconds; a read costs well under a millisecond.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Start polling. The thread ends when the event loop is gone.
pub fn watch(proxy: EventLoopProxy<UserEvent>) {
    let spawned = std::thread::Builder::new()
        .name("battery".into())
        .spawn(move || {
            let manager = match Manager::new() {
                Ok(m) => m,
                Err(e) => {
                    log::info!("battery status unavailable: {e}");
                    let _ = proxy.send_event(UserEvent::Battery(None));
                    return;
                }
            };
            let mut last = None;
            loop {
                let status = read(&manager);
                if last != Some(status) {
                    last = Some(status);
                    if proxy.send_event(UserEvent::Battery(status)).is_err() {
                        return;
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        log::warn!("cannot start the battery thread: {e}");
    }
}

/// All batteries together, weighted by capacity; `None` without any battery.
fn read(manager: &Manager) -> Option<BatteryStatus> {
    let batteries: Vec<_> = manager
        .batteries()
        .inspect_err(|e| log::debug!("battery read failed: {e}"))
        .ok()?
        .filter_map(Result::ok)
        .collect();
    if batteries.is_empty() {
        return None;
    }
    let full: f32 = batteries.iter().map(|b| b.energy_full().value).sum();
    let fraction = if full > 0.0 {
        batteries.iter().map(|b| b.energy().value).sum::<f32>() / full
    } else {
        batteries
            .iter()
            .map(|b| b.state_of_charge().value)
            .sum::<f32>()
            / batteries.len() as f32
    };
    let states: Vec<State> = batteries.iter().map(|b| b.state()).collect();
    Some(BatteryStatus {
        percent: (fraction.clamp(0.0, 1.0) * 100.0).round() as u8,
        // Unknown and Paused (charge limit reached) also happen on mains power.
        plugged_in: states
            .iter()
            .all(|s| !matches!(s, State::Discharging | State::Empty)),
        charging: states.contains(&State::Charging),
    })
}
