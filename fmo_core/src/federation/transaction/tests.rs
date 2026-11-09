use super::*;

fn date(day: &str) -> NaiveDate {
    day.parse().unwrap()
}

fn entry(day: &str, count: i64, amount: i64) -> HistogramEntry {
    HistogramEntry {
        date: date(day),
        count,
        amount,
    }
}

fn options(average_days: usize, exclude_outliers: bool) -> HistogramOptions {
    HistogramOptions {
        average_days,
        exclude_outliers,
    }
}

fn params(average: Option<&str>, outliers: Option<&str>) -> HistogramParams {
    HistogramParams {
        average: average.map(ToOwned::to_owned),
        outliers: outliers.map(ToOwned::to_owned),
    }
}

#[test]
fn histogram_params_default_and_reject_bad_values() {
    assert_eq!(params(None, None).options().unwrap(), options(0, false));
    assert_eq!(
        params(Some("30"), Some("exclude")).options().unwrap(),
        options(30, true)
    );
    assert_eq!(
        params(Some("7"), Some("include")).options().unwrap(),
        options(7, false)
    );
    for bad in [
        params(Some("5"), None),
        params(Some("x"), None),
        params(None, Some("drop")),
    ] {
        assert_eq!(
            bad.options().unwrap_err().status(),
            axum::http::StatusCode::BAD_REQUEST
        );
    }
}

#[test]
fn histogram_days_are_contiguous_through_today() {
    let histogram = build_histogram(
        &[entry("2024-02-28", 2, 100), entry("2024-03-01", 1, 50)],
        date("2024-03-03"),
        options(0, false),
    );
    let days = histogram
        .days
        .iter()
        .map(|day| (day.date.to_string(), day.count, day.volume_msat))
        .collect::<Vec<_>>();
    // 2024 is a leap year
    assert_eq!(
        days,
        [
            ("2024-02-28".to_owned(), 2, 100),
            ("2024-02-29".to_owned(), 0, 0),
            ("2024-03-01".to_owned(), 1, 50),
            ("2024-03-02".to_owned(), 0, 0),
            ("2024-03-03".to_owned(), 0, 0),
        ]
    );
    assert!(histogram
        .days
        .iter()
        .all(|day| day.avg_count.is_none() && !day.outlier));
    assert_eq!(
        (histogram.total_count, histogram.total_volume_msat),
        (3, 150)
    );

    assert_eq!(
        build_histogram(&[], date("2024-03-03"), options(7, true)),
        TransactionHistogram {
            days: vec![],
            total_count: 0,
            total_volume_msat: 0
        }
    );
}

#[test]
fn moving_averages_include_empty_days_and_shrink_at_the_start() {
    let histogram = build_histogram(
        &[entry("2024-01-01", 3, 300), entry("2024-01-03", 6, 600)],
        date("2024-01-05"),
        options(3, false),
    );
    let averages = histogram
        .days
        .iter()
        .map(|day| (day.avg_count.unwrap(), day.avg_volume_msat.unwrap()))
        .collect::<Vec<_>>();
    // 3/1, 3/2, 9/3, 6/3, 6/3
    assert_eq!(
        averages,
        [
            (3.0, 300.0),
            (1.5, 150.0),
            (3.0, 300.0),
            (2.0, 200.0),
            (2.0, 200.0)
        ]
    );
}

#[test]
fn excluded_outliers_leave_averages_and_totals() {
    // 20 regular days and one huge one
    let mut entries = (1..=20)
        .map(|day| entry(&format!("2024-01-{day:02}"), 1, 100))
        .collect::<Vec<_>>();
    entries.push(entry("2024-01-21", 1, 1_000_000));
    // Of 21 sorted volumes, index floor(21 * 0.95) = 19 is a regular 100: the
    // threshold is 1000
    let histogram = build_histogram(&entries, date("2024-01-22"), options(7, true));
    let outliers = histogram
        .days
        .iter()
        .filter(|day| day.outlier)
        .map(|day| day.date.to_string())
        .collect::<Vec<_>>();
    assert_eq!(outliers, ["2024-01-21"]);
    assert_eq!(
        (histogram.total_count, histogram.total_volume_msat),
        (20, 2000)
    );
    // The empty last day averages the 5 regular days and itself
    let last = histogram.days.last().unwrap();
    assert_eq!(last.date, date("2024-01-22"));
    assert!(!last.outlier);
    assert_eq!(last.avg_volume_msat, Some(500.0 / 6.0));

    // Included, nothing is flagged and everything counts
    let histogram = build_histogram(&entries, date("2024-01-22"), options(0, false));
    assert!(histogram.days.iter().all(|day| !day.outlier));
    assert_eq!(histogram.total_volume_msat, 1_002_000);
}

#[test]
fn outlier_threshold_is_inclusive_and_ignores_empty_days() {
    // As in the frontend, which kept volumes below the threshold: a day at it
    // is an outlier
    let histogram = build_histogram(
        &[entry("2024-01-01", 1, 0)],
        date("2024-01-03"),
        options(0, true),
    );
    // A threshold of 0 flags the zero-volume active day, not the empty days
    let flags = histogram
        .days
        .iter()
        .map(|day| day.outlier)
        .collect::<Vec<_>>();
    assert_eq!(flags, [true, false, false]);
}
