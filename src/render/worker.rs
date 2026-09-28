use super::{PreparedFrame, RenderRequest, prepare};
use crate::diff::Comparison;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};
type ResultMessage = (u64, anyhow::Result<PreparedFrame>);
pub struct Worker {
    requests: Sender<RenderRequest>,
    results: Receiver<ResultMessage>,
    generation: AtomicU64,
}
impl Worker {
    pub fn spawn(c: Arc<Comparison>) -> Self {
        let (tx, rx) = mpsc::channel::<RenderRequest>();
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(mut r) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    r = newer;
                }
                let result = prepare(&c, &r);
                if result_tx.send((r.generation, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            requests: tx,
            results: result_rx,
            generation: AtomicU64::new(0),
        }
    }
    pub fn submit(&self, r: RenderRequest) -> anyhow::Result<()> {
        self.generation.store(r.generation, Ordering::Relaxed);
        self.requests
            .send(r)
            .map_err(|_| anyhow::anyhow!("描画ワーカーが終了しました"))
    }
    pub fn poll(&self) -> Option<ResultMessage> {
        match self.results.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((
                self.generation.load(Ordering::Relaxed),
                Err(anyhow::anyhow!("描画ワーカーが終了しました")),
            )),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{Mode, Viewport},
        diff::compare,
    };
    use image::{Rgba, RgbaImage};
    use std::time::{Duration, Instant};
    #[test]
    fn worker_reports_errors_with_generation() {
        let c = Arc::new(
            compare(
                None,
                Some(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))),
            )
            .unwrap(),
        );
        let w = Worker::spawn(c);
        w.submit(RenderRequest {
            generation: 7,
            mode: Mode::Highlight,
            viewport: Viewport {
                zoom: 1.0,
                x: 0.0,
                y: 0.0,
                width: 1,
                height: 1,
            },
            cell_pixels: (1, 1),
            image_ids: vec![],
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some((g, r)) = w.poll() {
                assert_eq!(g, 7);
                assert!(r.is_err());
                break;
            }
            assert!(Instant::now() < deadline, "worker did not return a result");
            std::thread::yield_now();
        }
    }
}
