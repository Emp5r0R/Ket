use std::fs;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemSnapshot {
    pub cpu_load_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
}

pub(crate) fn snapshot() -> SystemSnapshot {
    let memory = memory();
    SystemSnapshot {
        cpu_load_percent: cpu_load_percent(),
        memory_used_bytes: memory.map(|(used, _)| used),
        memory_total_bytes: memory.map(|(_, total)| total),
        uptime_seconds: uptime_seconds(),
    }
}

fn cpu_load_percent() -> Option<f32> {
    let content = fs::read_to_string("/proc/loadavg").ok()?;
    let processors = std::thread::available_parallelism().ok()?.get();
    parse_cpu_load_percent(&content, processors)
}

fn memory() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    parse_memory(&content)
}

fn parse_cpu_load_percent(content: &str, processors: usize) -> Option<f32> {
    if processors == 0 {
        return None;
    }
    let load_average = content.split_whitespace().next()?.parse::<f32>().ok()?;
    if !load_average.is_finite() || load_average.is_sign_negative() {
        return None;
    }
    Some((load_average / processors as f32 * 100.0).clamp(0.0, 100.0))
}

fn parse_memory(content: &str) -> Option<(u64, u64)> {
    let mut total_kib: Option<u64> = None;
    let mut available_kib: Option<u64> = None;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next() else {
            continue;
        };
        match key {
            "MemTotal:" => total_kib = fields.next().and_then(|value| value.parse().ok()),
            "MemAvailable:" => available_kib = fields.next().and_then(|value| value.parse().ok()),
            _ => {}
        }
    }
    let total = total_kib?.saturating_mul(1024);
    let available = available_kib?.saturating_mul(1024);
    Some((total.saturating_sub(available), total))
}

fn uptime_seconds() -> Option<u64> {
    parse_uptime_seconds(&fs::read_to_string("/proc/uptime").ok()?)
}

fn parse_uptime_seconds(content: &str) -> Option<u64> {
    let value = content.split_whitespace().next()?.parse::<f64>().ok()?;
    if !value.is_finite() || value.is_sign_negative() || value > u64::MAX as f64 {
        return None;
    }
    Some(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_parser_normalizes_by_processor_count_and_clamps() {
        assert_eq!(
            parse_cpu_load_percent("2.00 1.00 0.50 1/2 3", 4),
            Some(50.0)
        );
        assert_eq!(parse_cpu_load_percent("99.0 1.0 1.0", 2), Some(100.0));
        assert_eq!(parse_cpu_load_percent("0.0", 8), Some(0.0));
        assert_eq!(parse_cpu_load_percent("invalid", 4), None);
        assert_eq!(parse_cpu_load_percent("NaN", 4), None);
        assert_eq!(parse_cpu_load_percent("-1.0", 4), None);
        assert_eq!(parse_cpu_load_percent("1.0", 0), None);
    }

    #[test]
    fn memory_parser_uses_available_memory_and_saturates_inconsistent_samples() {
        let sample = "MemTotal:       16384 kB\n\nUnrelated: invalid\nMemFree: 100 kB\nMemAvailable:    4096 kB\n";
        assert_eq!(parse_memory(sample), Some((12_288 * 1_024, 16_384 * 1_024)));

        let inconsistent = "MemTotal: 1024 kB\nMemAvailable: 2048 kB\n";
        assert_eq!(parse_memory(inconsistent), Some((0, 1_024 * 1_024)));
        assert_eq!(parse_memory("MemTotal: 1024 kB\n"), None);
        assert_eq!(
            parse_memory("MemTotal: nope kB\nMemAvailable: 1 kB\n"),
            None
        );
    }

    #[test]
    fn uptime_parser_accepts_fractional_seconds_and_rejects_invalid_values() {
        assert_eq!(parse_uptime_seconds("1234.99 55.0\n"), Some(1_234));
        assert_eq!(parse_uptime_seconds("0.0 0.0"), Some(0));
        assert_eq!(parse_uptime_seconds("-1.0 0.0"), None);
        assert_eq!(parse_uptime_seconds("NaN"), None);
        assert_eq!(parse_uptime_seconds("inf"), None);
        assert_eq!(parse_uptime_seconds(""), None);
    }

    #[test]
    fn live_snapshot_never_reports_out_of_range_values() {
        let snapshot = snapshot();
        assert!(
            snapshot
                .cpu_load_percent
                .is_none_or(|value| (0.0..=100.0).contains(&value))
        );
        assert!(
            snapshot
                .memory_used_bytes
                .zip(snapshot.memory_total_bytes)
                .is_none_or(|(used, total)| used <= total)
        );
    }
}
