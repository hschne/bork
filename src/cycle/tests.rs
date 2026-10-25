use super::*;

#[test]
fn suspend_is_conservative_and_a_full_break_survives_immediate_input() {
    let curve = GraceCurve::default();
    let mut cycle = BreakCycle::new(0);
    cycle.observe_suspend(1500, 299);
    assert_eq!(
        cycle.advance(1500, curve),
        DesiredPhase::TestCard { remaining: 2 }
    );
    cycle.observe_activity(1500);
    assert_eq!(
        cycle.advance(1500, curve),
        DesiredPhase::TestCard { remaining: 300 }
    );
    cycle.observe_suspend(1800, 300);
    cycle.observe_activity(1800);
    assert_eq!(cycle.advance(2999, curve), DesiredPhase::Work);
    assert_eq!(
        cycle.advance(3000, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
}

#[test]
fn repeated_idle_observations_do_not_restart_the_break() {
    let mut cycle = BreakCycle::new(0);
    cycle.observe_idle_since(1300);
    cycle.observe_idle_since(1400);
    assert_eq!(
        cycle.advance(1599, GraceCurve::default()),
        DesiredPhase::TestCard { remaining: 1 }
    );
    assert_eq!(
        cycle.advance(1600, GraceCurve::default()),
        DesiredPhase::Work
    );
}

#[test]
fn retuning_the_grace_curve_does_not_change_work_debt_or_deadline() {
    let mut cycle = BreakCycle::new(0);
    assert_eq!(
        cycle.advance(1290, GraceCurve::default()),
        DesiredPhase::Interference(GraceCurve::default().level(0.5))
    );
    let mut points = GraceCurve::default().points();
    points[0].intensity = 0.0;
    points[1].intensity = 10.0;
    let curve = GraceCurve::try_from(points).unwrap();
    assert_eq!(
        cycle.advance(1290, curve),
        DesiredPhase::Interference(curve.level(0.5))
    );
    assert_eq!(
        cycle.advance(1380, curve),
        DesiredPhase::TestCard { remaining: 300 }
    );
}

#[test]
fn boundaries_and_acceleration() {
    let mut cycle = BreakCycle::new(10);
    assert_eq!(
        cycle.advance(1209, GraceCurve::default()),
        DesiredPhase::Work
    );
    assert_eq!(
        cycle.advance(1210, GraceCurve::default()),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
    assert_eq!(
        cycle.advance(1300, GraceCurve::default()),
        DesiredPhase::Interference(GraceCurve::default().level(0.5))
    );
    assert_eq!(
        cycle.advance(1390, GraceCurve::default()),
        DesiredPhase::TestCard { remaining: 300 }
    );
}

#[test]
fn inactivity_crosses_deadline_and_resume_restarts_countdown() {
    let mut cycle = BreakCycle::new(0);
    cycle.observe_idle_since(1300);
    assert_eq!(
        cycle.advance(1380, GraceCurve::default()),
        DesiredPhase::TestCard { remaining: 220 }
    );
    cycle.observe_activity(1400);
    assert_eq!(
        cycle.advance(1400, GraceCurve::default()),
        DesiredPhase::TestCard { remaining: 300 }
    );
    cycle.observe_idle_since(1401);
    assert_eq!(
        cycle.advance(1700, GraceCurve::default()),
        DesiredPhase::TestCard { remaining: 1 }
    );
    assert_eq!(
        cycle.advance(1701, GraceCurve::default()),
        DesiredPhase::Work
    );
    assert_eq!(
        cycle.advance(9000, GraceCurve::default()),
        DesiredPhase::Work
    );
    cycle.observe_activity(9000);
    assert_eq!(
        cycle.advance(10199, GraceCurve::default()),
        DesiredPhase::Work
    );
    assert_eq!(
        cycle.advance(10200, GraceCurve::default()),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
}

#[test]
fn short_pauses_do_not_erase_debt() {
    let mut cycle = BreakCycle::new(0);
    cycle.observe_idle_since(1000);
    cycle.observe_activity(1299);
    assert!(matches!(
        cycle.advance(1300, GraceCurve::default()),
        DesiredPhase::Interference(_)
    ));
}

#[test]
fn idle_startup_and_suspend_complete_break_without_tick() {
    let mut cycle = BreakCycle::new(1000);
    cycle.observe_idle_since(800);
    cycle.observe_activity(1100);
    assert_eq!(
        cycle.advance(2299, GraceCurve::default()),
        DesiredPhase::Work
    );
    assert_eq!(
        cycle.advance(2300, GraceCurve::default()),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
    cycle.observe_idle_since(2400);
    cycle.observe_activity(5000);
    assert_eq!(
        cycle.advance(5000, GraceCurve::default()),
        DesiredPhase::Work
    );
}

#[test]
fn pause_freezes_debt_hides_effects_and_resume_preserves_the_deadline() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.pause(1290);
    cycle.pause(2000); // Idempotent: must not move the frozen timestamp.
    cycle.observe_idle_since(1300);
    cycle.observe_suspend(4000, 2000);
    cycle.observe_activity(4000);
    assert_eq!(cycle.advance(5000, curve), DesiredPhase::Work);
    let status = cycle.snapshot(5000, curve);
    assert_eq!(status.mode, Mode::Paused);
    assert_eq!(status.worked, 1290);
    assert_eq!(status.phase_remaining, 90);
    cycle.resume(5000);
    cycle.resume(5010); // Idempotent: no extra time may disappear.
    assert_eq!(
        cycle.advance(5000, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.5))
    );
    assert_eq!(
        cycle.advance(5090, curve),
        DesiredPhase::TestCard { remaining: 300 }
    );
}

#[test]
fn pause_in_overtime_keeps_debt_but_restarts_continuous_idle_observation() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_idle_since(1300);
    cycle.pause(1400);
    cycle.resume(9000);
    // The input adapter can still report its old idle start after manual control.
    cycle.observe_idle_since(1300);
    assert_eq!(
        cycle.advance(9299, curve),
        DesiredPhase::TestCard { remaining: 1 }
    );
    assert_eq!(cycle.advance(9300, curve), DesiredPhase::Work);
}

#[test]
fn reset_clears_overtime_and_pause_without_accepting_old_idle_history() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.pause(1500);
    cycle.reset(9000);
    cycle.observe_idle_since(1300);
    let status = cycle.snapshot(9000, curve);
    assert_eq!(status.mode, Mode::Running);
    assert_eq!(status.worked, 0);
    assert_eq!(status.phase_remaining, WORK_INTERVAL + GRACE_INTERVAL);
    assert_eq!(status.break_remaining, BREAK_INTERVAL);
    cycle.observe_activity(9001);
    assert_eq!(cycle.advance(10199, curve), DesiredPhase::Work);
    assert_eq!(
        cycle.advance(10200, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
}

#[test]
fn pausing_a_completed_break_does_not_restart_work_without_input() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_idle_since(1300);
    cycle.pause(1600);
    cycle.resume(9000);
    assert_eq!(cycle.snapshot(9000, curve).mode, Mode::Resting);
    cycle.observe_activity(9100);
    assert_eq!(cycle.snapshot(9100, curve).mode, Mode::Running);
    assert_eq!(
        cycle.advance(10300, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
}

#[test]
fn input_during_a_paused_completed_break_is_not_lost_on_resume() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_idle_since(1300);
    cycle.pause(1600);
    cycle.observe_activity(2000);
    cycle.resume(9000);
    let snapshot = cycle.snapshot(9000, curve);
    assert_eq!(snapshot.mode, Mode::Running);
    assert_eq!(snapshot.worked, 0);
    assert_eq!(
        cycle.advance(10200, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
}

#[test]
fn locked_input_does_not_restart_work_after_a_completed_break() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(1140, true, true, None);
    cycle.observe_input(1142, true, false, Some(1142));
    assert_eq!(cycle.snapshot(1442, curve).mode, Mode::Resting);
    // Wake the display and type a password, but do not unlock yet.
    cycle.observe_input(1500, true, true, None);
    cycle.observe_input(1502, true, false, Some(1502));
    assert_eq!(cycle.snapshot(1600, curve).mode, Mode::Resting);
    cycle.observe_input(1600, false, true, None);
    let snapshot = cycle.snapshot(1600, curve);
    assert_eq!(snapshot.mode, Mode::Running);
    assert_eq!(snapshot.worked, 0);
    assert_eq!(snapshot.phase_remaining, WORK_INTERVAL + GRACE_INTERVAL);
}

#[test]
fn early_unlock_retains_debt_and_unlock_without_input_is_a_return() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(1140, true, true, None);
    cycle.observe_input(1142, true, false, Some(1142));
    cycle.observe_input(1350, false, true, None);
    assert_eq!(cycle.snapshot(1350, curve).worked, 1350);
    // A second, complete break, followed by unlock without an input event.
    cycle.observe_input(1400, true, false, Some(1400));
    assert_eq!(cycle.snapshot(1700, curve).mode, Mode::Resting);
    cycle.observe_input(1800, false, false, Some(1400));
    let snapshot = cycle.snapshot(1800, curve);
    assert_eq!(snapshot.mode, Mode::Running);
    assert_eq!(snapshot.worked, 0);
    assert_eq!(snapshot.break_remaining, BREAK_INTERVAL);
}

#[test]
fn any_completed_idle_break_waits_for_return() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(302, false, false, Some(302));
    assert_eq!(cycle.snapshot(662, curve).mode, Mode::Resting);
    cycle.observe_input(700, false, true, None);
    assert_eq!(cycle.snapshot(700, curve).worked, 0);
    assert_eq!(cycle.snapshot(700, curve).mode, Mode::Running);
}

#[test]
fn configurable_durations_drive_boundaries_breaks_and_suspend() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    let timing =
        serde_json::from_str(r#"{"work_seconds":60,"grace_seconds":10,"break_seconds":210}"#)
            .unwrap();
    cycle.set_timing(timing);
    assert_eq!(cycle.snapshot(59, curve).phase_remaining, 11);
    assert_eq!(
        cycle.advance(60, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.0))
    );
    assert_eq!(
        cycle.advance(65, curve),
        DesiredPhase::Interference(GraceCurve::default().level(0.5))
    );
    assert_eq!(
        cycle.advance(70, curve),
        DesiredPhase::TestCard { remaining: 210 }
    );
    cycle.observe_input(100, true, false, Some(100));
    assert_eq!(cycle.snapshot(309, curve).break_remaining, 1);
    assert_eq!(cycle.snapshot(310, curve).mode, Mode::Resting);
    cycle.observe_input(400, false, false, Some(100));
    assert_eq!(cycle.snapshot(400, curve).phase_remaining, 70);
    cycle.reset(500);
    assert_eq!(cycle.snapshot(500, curve).phase_remaining, 70);
    cycle.observe_suspend(710, 210);
    assert_eq!(cycle.snapshot(710, curve).mode, Mode::Resting);
}

#[test]
fn zero_grace_goes_directly_to_the_card_and_retuning_keeps_debt() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    assert_eq!(cycle.snapshot(100, curve).worked, 100);
    cycle.set_timing(
        serde_json::from_str(r#"{"work_seconds":100,"grace_seconds":0,"break_seconds":30}"#)
            .unwrap(),
    );
    assert_eq!(
        cycle.advance(100, curve),
        DesiredPhase::TestCard { remaining: 30 }
    );
    assert_eq!(cycle.snapshot(100, curve).worked, 100);
    cycle.observe_idle_since(100);
    cycle.pause(110);
    cycle.resume(200);
    assert_eq!(cycle.snapshot(200, curve).worked, 110);
    cycle.observe_idle_since(100);
    assert_eq!(cycle.snapshot(229, curve).break_remaining, 1);
    assert_eq!(cycle.snapshot(230, curve).mode, Mode::Resting);
}

#[test]
fn locked_activity_interrupts_an_incomplete_break() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(1142, true, false, Some(1142));
    cycle.observe_input(1400, true, true, None);
    cycle.observe_input(1402, true, false, Some(1402));
    assert_eq!(cycle.snapshot(1442, curve).mode, Mode::Running);
    assert_eq!(cycle.snapshot(1442, curve).break_remaining, 260);
    cycle.observe_input(1500, false, true, None);
    assert_eq!(cycle.snapshot(1500, curve).worked, 1500);
}

#[test]
fn suspended_break_with_password_input_waits_for_unlock() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(1140, true, true, None);
    cycle.observe_suspend(1740, 600);
    cycle.observe_input(1740, true, true, None);
    assert_eq!(cycle.snapshot(1740, curve).mode, Mode::Resting);
    cycle.observe_input(1800, false, false, None);
    assert_eq!(cycle.snapshot(1800, curve).mode, Mode::Running);
    assert_eq!(cycle.snapshot(1800, curve).worked, 0);
}

#[test]
fn countdown_runs_continuously_from_work_plus_grace_to_the_card() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    for (now, remaining) in [(0, 1380), (1199, 181), (1200, 180), (1379, 1)] {
        assert_eq!(cycle.snapshot(now, curve).phase_remaining, remaining);
    }
    assert_eq!(cycle.snapshot(1380, curve).phase_remaining, 300);
    cycle.observe_idle_since(1380);
    assert_eq!(cycle.snapshot(1400, curve).phase_remaining, 280);
}

#[test]
fn resting_counts_from_break_completion_not_the_tick_and_survives_locked_input() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_input(302, true, false, Some(302));
    assert_eq!(cycle.snapshot(602, curve).resting_seconds, 0);
    assert_eq!(cycle.snapshot(662, curve).resting_seconds, 60);
    cycle.observe_input(700, true, true, None);
    assert_eq!(cycle.snapshot(710, curve).resting_seconds, 108);
    cycle.observe_input(720, true, false, Some(720));
    assert_eq!(cycle.snapshot(1100, curve).resting_seconds, 498);
    cycle.observe_input(1200, false, true, None);
    assert_eq!(cycle.snapshot(1200, curve).resting_seconds, 0);
    assert_eq!(cycle.snapshot(1200, curve).phase_remaining, 1380);
}

#[test]
fn pausing_freezes_resting_elapsed_time_too() {
    let mut cycle = BreakCycle::new(0);
    let curve = GraceCurve::default();
    cycle.observe_idle_since(100);
    cycle.pause(450);
    assert_eq!(cycle.snapshot(900, curve).resting_seconds, 50);
    cycle.resume(900);
    assert_eq!(cycle.snapshot(910, curve).resting_seconds, 60);
}

#[test]
fn suspended_resting_time_includes_the_proven_time_after_break_completion() {
    let mut cycle = BreakCycle::new(0);
    cycle.observe_input(100, true, true, None);
    cycle.observe_suspend(1000, 900);
    assert_eq!(
        cycle.snapshot(1000, GraceCurve::default()).resting_seconds,
        600
    );
}
