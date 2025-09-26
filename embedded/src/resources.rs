use alloc::vec;
use alloc::vec::Vec;
use core::iter::Cycle;
use core::iter::Peekable;
use embassy_time::{Duration, Instant};
use interface::Resource;
use tinyqoi::Qoi;

#[derive(Debug, thiserror::Error)]
pub enum SpriteError {
    #[error("Failed to get next sprite frame")]
    IteratorFail,
    #[error("Qoi failed to parse the image: {0:#?}")]
    ImageParseError(tinyqoi::Error),
}

pub struct BakedResource {
    iter: Peekable<Cycle<alloc::vec::IntoIter<Vec<u8>>>>,
    last_iteration: Instant,
    frame_time: Duration,
}

pub fn bake(res: Resource) -> BakedResource {
    BakedResource {
        iter: res.frames.into_iter().cycle().peekable(),
        last_iteration: Instant::now(),
        frame_time: Duration::from_millis(res.frame_time_ms as u64),
    }
}

impl BakedResource {
    pub fn get_image(&mut self, time: Instant) -> Result<Qoi, SpriteError> {
        let current_frame = if self.needs_update(time) {
            self.last_iteration = time;
            self.iter.next();
            self.iter.peek()
        } else {
            self.iter.peek()
        };
        if let Some(frame) = current_frame {
            return Qoi::new(frame).map_err(SpriteError::ImageParseError);
        }
        Err(SpriteError::IteratorFail)
    }

    pub fn needs_update(&self, time: Instant) -> bool {
        self.last_iteration + self.frame_time < time
    }
}

pub fn get_wifi_sprite() -> BakedResource {
    bake(Resource {
        frames: vec![
            include_bytes!("../sprites/wifi1.qoi").to_vec(),
            include_bytes!("../sprites/wifi2.qoi").to_vec(),
            include_bytes!("../sprites/wifi3.qoi").to_vec(),
        ],
        frame_time_ms: 500,
    })
}

pub fn get_dino_sprite() -> BakedResource {
    bake(Resource {
        frames: vec![
            include_bytes!("../sprites/Dino1.qoi").to_vec(),
            include_bytes!("../sprites/Dino2.qoi").to_vec(),
            include_bytes!("../sprites/Dino3.qoi").to_vec(),
            include_bytes!("../sprites/Dino4.qoi").to_vec(),
        ],
        frame_time_ms: 700,
    })
}

pub fn get_no_image_sprite() -> BakedResource {
    bake(Resource {
        frames: vec![include_bytes!("../sprites/no_image.qoi").to_vec()],
        frame_time_ms: 0,
    })
}
