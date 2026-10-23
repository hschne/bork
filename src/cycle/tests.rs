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
        DesiredPhase::Interference(Glitchiness::ZERO)
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
        DesiredPhase::Interference(Glitchiness::try_from(25.0).unwrap())
    );
    assert_eq!(
        cycle.advance(1290, GraceCurve::try_from(3.0).unwrap()),
        DesiredPhase::Interference(Glitchiness::try_from(12.5).unwrap())
    );
    assert_eq!(
        cycle.advance(1380, GraceCurve::try_from(5.0).unwrap()),
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
        DesiredPhase::Interference(Glitchiness::ZERO)
    );
    assert_eq!(
        cycle.advance(1300, GraceCurve::default()),
        DesiredPhase::Interference(Glitchiness::try_from(25.0).unwrap())
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
        DesiredPhase::Interference(Glitchiness::ZERO)
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
        DesiredPhase::Interference(Glitchiness::ZERO)
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
        DesiredPhase::Interference(Glitchiness::try_from(25.0).unwrap())
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
    assert_eq!(status.phase_remaining, WORK_INTERVAL);
    assert_eq!(status.break_remaining, BREAK_INTERVAL);
    cycle.observe_activity(9001);
    assert_eq!(cycle.advance(10199, curve), DesiredPhase::Work);
    assert_eq!(
        cycle.advance(10200, curve),
        DesiredPhase::Interference(Glitchiness::ZERO)
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
        DesiredPhase::Interference(Glitchiness::ZERO)
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
        DesiredPhase::Interference(Glitchiness::ZERO)
    );
}
