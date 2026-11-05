use super::*;
use std::{path::Path, time::Instant};

fn fixture(path: &Path, value: i16, seconds: u32) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..8000 * seconds {
        writer.write_sample(value)?;
    }
    writer.finalize()?;
    Ok(())
}

#[test]
fn play_seek_switch_stop_and_recovery_invalidate_old_audio() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let a = dir.path().join("a.wav");
    let b = dir.path().join("b.wav");
    fixture(&a, 8192, 2)?;
    fixture(&b, -8192, 2)?;
    let shared = Shared::default();
    let mut renderer = None;
    apply(
        Command::Play { path: a.clone() },
        &mut renderer,
        &shared,
        8000,
    )?;
    assert_eq!(status(&shared).state, "playing");
    assert_eq!(renderer.as_mut().unwrap().next_frame()?.unwrap(), [0.25; 2]);
    let first = status(&shared).generation;
    shared.paused.store(true, Ordering::Release);
    apply(Command::Seek { seconds: 1.0 }, &mut renderer, &shared, 8000)?;
    assert_eq!(status(&shared).state, "paused");
    assert!(status(&shared).generation > first);
    assert_eq!(renderer.as_ref().unwrap().position(), 1.0);
    apply(Command::Play { path: b }, &mut renderer, &shared, 8000)?;
    assert_eq!(
        status(&shared).state,
        "paused",
        "selection must not undo a newer pause"
    );
    assert_eq!(
        renderer.as_mut().unwrap().next_frame()?.unwrap(),
        [-0.25; 2]
    );
    apply(Command::Stop, &mut renderer, &shared, 8000)?;
    assert!(renderer.is_none());
    assert_eq!(status(&shared).state, "stopped");
    let error = apply(
        Command::Play {
            path: dir.path().join("missing.wav"),
        },
        &mut renderer,
        &shared,
        8000,
    )
    .unwrap_err();
    fail(&shared, &mut renderer, error);
    assert_eq!(status(&shared).state, "error");
    apply(Command::Play { path: a }, &mut renderer, &shared, 8000)?;
    assert!(status(&shared).error.is_none());
    assert!(renderer.is_some());
    Ok(())
}

#[test]
fn rate_change_restarts_at_rendered_not_prefetched_position() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.wav");
    fixture(&path, 8192, 2)?;
    let shared = Shared::default();
    let mut renderer = None;
    apply(Command::Play { path }, &mut renderer, &shared, 8000)?;
    for _ in 0..4000 {
        renderer.as_mut().unwrap().next_frame()?;
    }
    shared.position.store(0.1f64.to_bits(), Ordering::Release);
    apply(Command::Rate { value: 2.0 }, &mut renderer, &shared, 8000)?;
    assert_eq!(renderer.as_ref().unwrap().position(), 0.1);
    assert_eq!(status(&shared).rate, 2.0);
    Ok(())
}

#[test]
fn worker_handles_rapid_seeks_and_shutdown_with_full_ring() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.wav");
    fixture(&path, 8192, 4)?;
    let shared = Arc::new(Shared::default());
    let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
    let (producer, mut consumer) = rtrb::RingBuffer::new(64);
    let worker_shared = shared.clone();
    let handle = thread::spawn(move || worker(receiver, producer, worker_shared, 8000));
    sender.send(Command::Play { path })?;
    for seconds in [0.1, 0.5, 2.0, 1.0] {
        sender.send(Command::Seek { seconds })?;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut observed = false;
    while Instant::now() < deadline {
        if let Ok(frame) = consumer.pop() {
            if frame.position >= 1.0
                && frame.position < 1.1
                && frame.generation == shared.generation.load(Ordering::Acquire)
            {
                observed = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    shared.shutdown.store(true, Ordering::Release);
    handle.join().unwrap();
    assert!(
        observed,
        "worker must reach the final seek without an output device"
    );
    Ok(())
}

#[test]
fn command_queue_is_bounded() {
    let (sender, _receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
    for _ in 0..COMMAND_CAPACITY {
        sender.try_send(Command::Stop).unwrap();
    }
    assert!(sender.try_send(Command::Stop).is_err());
}
