const FIVE_MS: u64 = 5_000;

#[derive(Debug, Default)]
pub(crate) struct Samples {
    points: Vec<(u64, u64)>,
}

impl Samples {
    pub(crate) fn clear(&mut self) {
        self.points.clear();
    }

    pub(crate) fn push(&mut self, at: u64, done: u64) {
        if let Some(last) = self.points.last_mut()
            && last.0 == at
        {
            last.1 = done;
            return;
        }
        self.points.push((at, done));
        self.prune();
    }

    pub(crate) fn rate(&self) -> Option<f64> {
        if self.points.len() < 2 {
            return None;
        }
        let (now, done_now) = *self.points.last()?;
        let (prev, _) = self.points[self.points.len() - 2];
        let window = now.saturating_sub(prev).max(FIVE_MS);
        let start = now.saturating_sub(window);
        let anchor = self
            .points
            .iter()
            .rev()
            .find(|point| point.0 <= start)
            .unwrap_or(&self.points[0]);
        let elapsed = now.saturating_sub(anchor.0);
        if elapsed == 0 {
            return None;
        }
        let delta = done_now as f64 - anchor.1 as f64;
        let rate = delta / (elapsed as f64 / 1_000.0);
        rate.is_finite().then_some(rate)
    }

    fn prune(&mut self) {
        if self.points.len() < 2 {
            return;
        }
        let now = self.points[self.points.len() - 1].0;
        let prev = self.points[self.points.len() - 2].0;
        let start = now.saturating_sub(now.saturating_sub(prev).max(FIVE_MS));
        if let Some(index) = self.points.iter().rposition(|point| point.0 <= start) {
            self.points.drain(..index);
        }
    }
}
