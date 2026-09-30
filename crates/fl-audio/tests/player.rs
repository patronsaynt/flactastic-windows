//! `PlayerState` behaviour: shuffle/unshuffle, the user-queued section,
//! repeat-all and the listening tracker.

use std::time::{Duration, Instant};

use fl_audio::output::null::NullBackend;
use fl_audio::player::{Player, RepeatMode};
use fl_audio::Engine;
use fl_core::{Track, Uid};

mod common;
use common::*;

fn player(rate: u32, speed: f64) -> Player {
    Player::new(Engine::new(NullBackend::paced(rate, speed)))
}

fn ids(v: &[Track]) -> Vec<Uid> {
    v.iter().map(|t| t.id).collect()
}

#[test]
fn shuffle_keeps_current_and_user_queue_then_restores_order() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, _) = split_files(dir.path(), 44_100, &[4410, 8820, 13_230, 17_640, 22_050], 26_460);
    let mut p = player(44_100, 1.0);
    p.start_fresh_queue(tracks.clone(), 2, Some("Album".into()));
    let extra = tracks[0].clone();
    p.add_to_queue(&[extra]);
    let queued = p.engine.queue()[3].clone();
    assert!(p.is_user_queued(&queued));

    p.toggle_shuffle();
    let q = p.engine.queue();
    assert_eq!(q.len(), 7);
    assert_eq!(p.engine.current_index(), 0);
    assert_eq!(q[0].id, tracks[2].id);
    assert_eq!(q[1].id, queued.id, "user-queued stays next up");
    let mut rest: Vec<Uid> = ids(&q[2..]);
    rest.sort();
    let mut expect: Vec<Uid> = tracks.iter().enumerate().filter(|(i, _)| *i != 2).map(|(_, t)| t.id).collect();
    expect.sort();
    assert_eq!(rest, expect, "played and upcoming source tracks all stay in the pool");

    p.toggle_shuffle();
    let q = p.engine.queue();
    assert_eq!(p.engine.current_index(), 2);
    let mut want = ids(&tracks[..3]);
    want.push(queued.id);
    want.extend(ids(&tracks[3..]));
    assert_eq!(ids(&q), want);
}

#[test]
fn add_to_queue_appends_after_user_section_and_play_next_goes_first() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, _) = split_files(dir.path(), 44_100, &[4410, 8820], 13_230);
    let mut p = player(44_100, 1.0);
    p.start_fresh_queue(tracks.clone(), 0, None);
    p.add_to_queue(&[tracks[2].clone()]);
    p.add_to_queue(&[tracks[1].clone()]);
    p.play_next(&[tracks[0].clone()]);
    let q = p.engine.queue();
    let paths: Vec<_> = q.iter().map(|t| t.path.clone()).collect();
    let tp: Vec<_> = tracks.iter().map(|t| t.path.clone()).collect();
    assert_eq!(paths, vec![tp[0].clone(), tp[0].clone(), tp[2].clone(), tp[1].clone(), tp[1].clone(), tp[2].clone()]);
    assert_eq!(p.user_queued.len(), 3);

    // Drag the last user-queued track to the front of the section.
    p.move_track(q[3].id, q[1].id);
    let q2 = p.engine.queue();
    assert_eq!(q2[1].id, q[3].id);
    assert_eq!(p.engine.current_index(), 0);

    p.remove_from_queue(1);
    assert_eq!(p.engine.queue().len(), 5);
    assert_eq!(p.user_queued.len(), 2);
    p.remove_from_queue(0); // the current track can't be removed
    assert_eq!(p.engine.queue().len(), 5);
}

fn tick_until(p: &mut Player, timeout: Duration, mut done: impl FnMut(&Player) -> bool) {
    let start = Instant::now();
    while !done(p) {
        p.tick();
        assert!(start.elapsed() < timeout, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn repeat_all_wraps_and_listens_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 44_100;
    let (tracks, _) = split_files(dir.path(), rate, &[132_300], 264_600);
    let mut p = player(rate as u32, 8.0);
    p.set_repeat_mode(RepeatMode::All);
    p.start_fresh_queue(tracks.clone(), 0, None);
    p.engine.play();
    p.tick();
    // Wait until the queue wraps back to the first track.
    let mut saw_second = false;
    tick_until(&mut p, Duration::from_secs(10), |p| {
        if p.engine.current_index() == 1 {
            saw_second = true;
        }
        saw_second && p.engine.current_index() == 0 && p.engine.is_playing
    });
    let plays = p.take_plays();
    assert_eq!(plays.len(), 2, "{plays:?}");
    assert!(plays.iter().all(|r| r.counted), "{:?}", plays.iter().map(|r| r.seconds_listened).collect::<Vec<_>>());
    assert_eq!(plays[0].track.id, tracks[0].id);
    assert_eq!(plays[1].track.id, tracks[1].id);
}

#[test]
fn skipping_does_not_count_a_play() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, _) = split_files(dir.path(), 44_100, &[132_300], 264_600);
    let mut p = player(44_100, 1.0);
    p.start_fresh_queue(tracks.clone(), 0, None);
    p.engine.play();
    for _ in 0..5 {
        p.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    p.engine.seek(2.5); // a scrub isn't credited
    p.tick();
    p.next();
    p.tick();
    let plays = p.take_plays();
    assert_eq!(plays.len(), 1);
    assert!(!plays[0].counted);
    assert!(plays[0].seconds_listened < 0.5);
}
