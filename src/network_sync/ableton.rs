use rusty_link::{AblLink, SessionState};
use std::time::Duration;

pub struct LinkManager {
    link: AblLink,
    session_state: SessionState,
}

impl LinkManager {
    pub fn new() -> Self {
        let link = AblLink::new(120.0); // Default BPM
        link.enable(false);
        Self {
            link,
            session_state: SessionState::new(),
        }
    }

    pub fn update_tempo(&mut self, bpm: f64) {
        self.link.capture_app_session_state(&mut self.session_state);
        let current_tempo = self.session_state.tempo();

        // Avoid micro-updates to prevent jitter
        if (current_tempo - bpm).abs() > 0.1 {
            let time = self.link.clock_micros();
            self.session_state.set_tempo(bpm, time);
            self.link.commit_app_session_state(&self.session_state);
        }
    }

    pub fn sync_downbeat(&mut self, latency: Duration) {
        self.link.capture_app_session_state(&mut self.session_state);
        let time = self.link.clock_micros();

        let latency_micros = latency.as_micros() as i64;
        let target_time = time - latency_micros;

        self.session_state
            .request_beat_at_time(0.0, target_time, 4.0);
        self.link.commit_app_session_state(&self.session_state);
    }

    pub fn link_state(&mut self, enable: bool) {
        self.link.enable(enable);
    }

    pub fn num_peers(&self) -> usize {
        self.link.num_peers() as usize
    }
}
