use crate::herdr_tests::Fake;
use crate::voice::*;
use std::path::Path;

#[test]
fn clean_keeps_the_words_and_drops_whisper_markers() {
    assert_eq!(clean("\n Review the diff and report only actionable findings."), "Review the diff and report only actionable findings.");
    assert_eq!(clean("\n [BLANK_AUDIO]"), "");
    assert_eq!(clean(" [BLANK_AUDIO] run the tests (typing) please\n"), "run the tests please");
    assert_eq!(clean("one\n two   three\n"), "one two three");
    assert_eq!(clean("say (hello) to [them] now"), "say to now", "markers are dropped even mid-sentence");
    assert_eq!(clean("a ) b"), "a b", "a stray close is ignored");
    assert_eq!(clean("open ( then words"), "open", "an unclosed marker drops the rest");
}

#[test]
fn transcribe_runs_whisper_cli_with_the_probed_flags() {
    let f = Fake::new(vec![("whisper-cli", Ok("\n Hello there."))]);
    let text = transcribe(&f, Path::new("/m/base.bin"), Path::new("/s/voice.wav"), "en").unwrap();
    assert_eq!(text, "Hello there.");
    assert_eq!(f.calls(), ["whisper-cli -m /m/base.bin -f /s/voice.wav -nt -np -l en"]);
}

#[test]
fn transcribe_surfaces_whisper_failure() {
    let f = Fake::new(vec![("whisper-cli", Err("error: failed to read audio"))]);
    assert_eq!(transcribe(&f, Path::new("/m"), Path::new("/w"), "en").unwrap_err(), "error: failed to read audio");
}

#[test]
fn recordings_under_300_ms_are_too_short() {
    assert!(!long_enough(0));
    assert!(!long_enough(14_399));
    assert!(long_enough(14_400));
}

#[test]
fn write_wav_writes_16_bit_mono_at_the_capture_rate() {
    let dir = std::env::temp_dir().join(format!("herd-gamepad-wav-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.wav");
    let samples: Vec<f32> = (0..4800).map(|i| ((i as f32) * 0.01).sin()).collect();
    write_wav(&path, &samples).unwrap();
    let r = hound::WavReader::open(&path).unwrap();
    assert_eq!(r.spec(), hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int });
    assert_eq!(r.len(), 4800);

    let path = dir.join("levels.wav");
    write_wav(&path, &[0.0, 0.5, 1.0, -1.0, 2.0]).unwrap();
    let got: Vec<i16> = hound::WavReader::open(&path).unwrap().samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(got.len(), 5, "{got:?}");
    assert_eq!(got[0], 0, "{got:?}");
    assert!((16_383..=16_384).contains(&got[1]), "{got:?}");
    assert_eq!(got[2], 32_767, "{got:?}");
    assert!((-32_768..=-32_767).contains(&got[3]), "{got:?}");
    assert_eq!(got[4], 32_767, "out-of-range input saturates: {got:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn append_capped_keeps_at_most_max_samples() {
    let mut buf = vec![0.0; 5];
    append_capped(&mut buf, &[1.0, 2.0, 3.0, 4.0], 7);
    assert_eq!(buf, [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0]);
    append_capped(&mut buf, &[9.0, 9.0], 7);
    assert_eq!(buf.len(), 7, "nothing is appended once the cap is reached");
}
