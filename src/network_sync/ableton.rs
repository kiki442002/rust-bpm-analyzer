use rusty_link::{AblLink, SessionState};
use std::time::{Duration, Instant};

pub struct LinkManager {
    link: AblLink,
    session_state: SessionState,
    last_sync_time: Instant,
}

impl LinkManager {
    pub fn new() -> Self {
        let link = AblLink::new(120.0); // Default BPM
        link.enable(false);
        Self {
            link,
            session_state: SessionState::new(),
            last_sync_time: Instant::now(),
        }
    }

    pub fn update_tempo(&mut self, bpm: f64, is_drop: bool, beat_offset: Option<Duration>) {
        self.link.capture_app_session_state(&mut self.session_state);
        let current_tempo = self.session_state.tempo();

        // Avoid micro-updates to prevent jitter
        if (current_tempo - bpm).abs() > 0.1 {
            let time = self.link.clock_micros();
            self.session_state.set_tempo(bpm, time);
            self.link.commit_app_session_state(&self.session_state);
        }

        // Sync Phase on Drop (with 10s cooldown)
        if let Some(offset) =
            beat_offset.filter(|_| is_drop && self.last_sync_time.elapsed().as_secs() > 10)
        {
            self.sync_downbeat(offset);
            self.last_sync_time = Instant::now();
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

    #[allow(dead_code)]
    pub fn num_peers(&self) -> usize {
        self.link.num_peers() as usize
    }
}
