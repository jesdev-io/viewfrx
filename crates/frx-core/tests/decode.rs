use anyhow::Result;
use frx_core::decode::AudioReader;

#[test]
fn mp3_gapless_decode_and_seek() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("tone.mp3");
    std::fs::write(&path, include_bytes!("fixtures/tone.mp3"))?;
    let mut reader = AudioReader::open(&path)?;
    assert_eq!(reader.info().channels, 2);
    assert_eq!(reader.info().sample_rate, 44100);
    let mut frame = [0.0; 2];
    let mut count = 0;
    let mut energy = 0.0;
    while reader.next_frame(&mut frame)? {
        count += 1;
        energy += frame[0] * frame[0];
    }
    assert_eq!(count, 11025, "encoder padding should not extend playback");
    assert!(energy > 1.0);
    reader.seek(0.1)?;
    count = 0;
    while reader.next_frame(&mut frame)? {
        count += 1;
    }
    assert_eq!(count, 6615, "seek should retain 150 ms of gapless audio");
    reader.seek(0.0)?;
    count = 0;
    while reader.next_frame(&mut frame)? {
        count += 1;
    }
    assert_eq!(count, 11025);
    Ok(())
}
