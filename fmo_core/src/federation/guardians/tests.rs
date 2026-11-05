use super::*;

#[test]
fn health_polls_back_off_only_while_offline() {
    let minutes = |interval: Duration| interval.as_secs() / 60;

    assert_eq!(minutes(health_poll_interval(None)), 1);
    // Online again within the current hour, see `health_offline_since`
    assert_eq!(
        minutes(health_poll_interval(Some(TimeDelta::minutes(-20)))),
        1
    );
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::hours(23)))), 1);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(1)))), 10);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(6)))), 10);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(7)))), 30);
    assert_eq!(
        minutes(health_poll_interval(Some(TimeDelta::days(200)))),
        30
    );
}
