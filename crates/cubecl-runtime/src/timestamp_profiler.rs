use cubecl_common::profile::ProfileDuration;
use hashbrown::HashMap;
use web_time::Instant;

use crate::server::{ProfileError, ProfilingToken};

#[derive(Default, Debug)]
/// A simple struct to keep track of timestamps for kernel execution.
/// This should be used for servers that do not have native device profiling.
pub struct TimestampProfiler {
    state: HashMap<ProfilingToken, State>,
    counter: u64,
}

#[derive(Debug)]
enum State {
    Start(Instant),
    Error(ProfileError),
}

impl TimestampProfiler {
    /// If there is some profiling registered.
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }
    /// Start measuring
    pub fn start(&mut self) -> ProfilingToken {
        let token = ProfilingToken { id: self.counter };
        self.counter += 1;
        self.state
            .insert(token, State::Start(std::time::Instant::now()));
        token
    }

    /// Stop measuring
    pub fn stop(&mut self, token: ProfilingToken) -> Result<ProfileDuration, ProfileError> {
        let state = self.state.remove(&token);
        let start = match state {
            Some(val) => match val {
                State::Start(instant) => instant,
                State::Error(profile_error) => return Err(profile_error),
            },
            None => return Err(ProfileError::NotRegistered),
        };

        Ok(ProfileDuration::from_duration(start.elapsed()))
    }

    /// Register an error during profiling.
    pub fn error(&mut self, error: ProfileError) {
        self.state
            .iter_mut()
            .for_each(|(_, state)| *state = State::Error(error.clone()));
    }
}
